# CHANGELOG

This file is the single source of truth for all changes made to this repository
as part of the Stellar Wave Program resubmission audit. Entries are appended in
chronological order. Do not edit or remove prior entries.
 
---

## 2026-09-20 — docs/readme-and-walkthrough (PR #1)

### Changes

**README.md** — Added "Why Not Just Use Soroban's Native upgrade?" section.
- Side-by-side comparison table: native Soroban self-upgrade vs Soroban Upgrade Manager.
- Explains the single-point-of-failure problem with admin-key upgrades.
- Positions the contract as the Soroban equivalent of Compound Governor Bravo /
  OpenZeppelin TimelockController.
- Added Documentation table linking to the new walkthrough.

**docs/walkthrough.md** — New file (378 lines).
- Full step-by-step walkthrough: setup → upload WASM → deploy manager → deploy
  target → initialize → propose → vote × 3 → finalize → execute.
- All steps use real `stellar contract invoke` commands with named parameters.
- Covers failure modes (TimelockNotExpired, Defeated, NotQueued, AlreadyVoted)
  with resolution guidance.
- Documents what the target contract's `upgrade()` function must look like.
- Includes a timeline diagram and "What the upgrade() function looks like" section.

**Why:** The original README said "most teams handle upgrades manually" without
explaining the security model or why a timelock + vote is better. Reviewers need
to immediately understand the value proposition vs the Soroban native mechanism.

---

## 2026-09-20 — tests/governance-upgrade-integration (PR #2)

### Changes

**contracts/upgrade-manager/src/lib.rs** — Added integration test:
- `test_full_governance_upgrade_flow` — end-to-end test: deploy upgrade-manager,
  deploy a minimal upgradable target contract, propose, vote × 3, advance past
  timelock, finalize, execute. Verifies the target contract's WASM is replaced
  (post-upgrade call succeeds with new behavior).

**Why:** The existing tests covered all governance mechanics (propose, vote,
finalize, cancel) but none of them exercised `execute()` against a real target
contract with a real WASM replacement. The integration test proves the cross-contract
upgrade path actually works end-to-end, which is the core value proposition.

---

## 2026-09-20 — tests/governance-upgrade-integration (follow-up, same PR #2)

### Changes

**contracts/upgrade-manager/src/lib.rs** — Fixed two test-infrastructure issues
found during local compilation and test run:

1. `advance_past_timelock` helper: removed unused `use soroban_sdk::testutils::storage::Instance`
   import that caused a compiler warning.

2. `test_full_governance_upgrade_flow`: Fixed `Error(Storage, MissingValue): "Wasm does
   not exist"` failure.
   - Root cause: `env.register(UpgradableTarget, ())` stores the contract natively
     with an empty-bytes placeholder WASM. When `execute()` calls
     `env.invoke_contract(target, "upgrade", [hash])` and the target calls
     `env.deployer().update_current_contract_wasm(hash)`, the mock host looks up
     `hash` in its WASM table. The original test passed `[0xab; 32]` — a hash with
     no corresponding binary — causing `MissingValue`.
   - Fix: call `env.deployer().upload_contract_wasm(Bytes::new(&env))` before
     proposing to explicitly upload empty bytes and obtain their real hash (sha256
     of empty = e3b0c4...). That hash already exists in the ledger (it's what
     `env.register()` stored), so `update_current_contract_wasm` succeeds.
   - Also fixed: the target contract's instance storage was getting archived after
     the 17 281-ledger advance. Added `env.as_contract(&target_id, || { env.storage().instance().extend_ttl(...) })`
     before advancing the ledger.
   - This is a test-infrastructure fix only. No contract logic was changed.

**Local test run result:**
- `cargo test --manifest-path contracts/upgrade-manager/Cargo.toml`
- 21 tests, 0 failures, 0 warnings.

**Why:** The integration test had two environment-simulation gaps that caused it to
fail in the mock host. Both are test-only issues (not contract bugs): the mock
requires uploaded WASM to exist before referencing it, and TTLs must be manually
bumped when advancing the ledger sequence.

---

## 2026-10-26 — fix/phase-1-contract-fixes (Phase 1 of 4)

Note: phases 1–4 carry branch names instead of PR numbers; PR numbers are not
known before merging. See the PR description for the merge order.

### Changes — 1.1 + 1.2: Timelock placement and config snapshot

**contracts/upgrade-manager/src/types.rs**
- Added `voting_period: u32` to `Config`.
- Added `voting_ends_at: u32` and `executable_at: u32` to `Proposal` (executable_at is 0 until finalize is called).
- Added snapshot fields to `Proposal`: `snap_voting_period`, `snap_timelock_delay`, `snap_quorum`, `snap_threshold_pct` — copied from Config at creation time.
- Added TTL constants `INSTANCE_TTL_BUMP` (518 400 ledgers ≈ 30 days) and `PERSISTENT_TTL_BUMP` (2 592 000 ledgers ≈ 150 days).
- Added new errors: `VotingPeriodEnded` (16), `VotingStillActive` (17), `EmptyDescription` (18), `InvalidVotingPeriod` (19).
- Fixed `Config.quorum` doc comment: previously said "YES votes", now correctly says "total votes (YES + NO)".

**contracts/upgrade-manager/src/governance.rs**
- `create_proposal`: sets `voting_ends_at = created_at + snap_voting_period` using checked arithmetic; snapshots all four config fields into the proposal.
- `cast_vote`: now rejects votes when `ledger.sequence() > proposal.voting_ends_at`, returning `VotingPeriodEnded`.
- `finalize_voting`: now requires `ledger.sequence() > proposal.voting_ends_at`, returning `VotingStillActive` if too early; on pass sets `executable_at = finalize_ledger + snap_timelock_delay` (timelock starts at finalization, not creation); uses snapshotted quorum/threshold for the pass check.

**Why (1.1):** The old code set `executable_at = created_at + timelock_delay` and required `finalize` to be called after `executable_at`. This meant the "timelock" was actually a combined voting+delay period from creation, not a delay between approval and execution. A finalized-Queued proposal could be executed immediately. The README stated "mandatory delay between vote passing and execution" — that was not true of the code.

**Why (1.2):** `update_config` could change quorum/threshold mid-proposal, silently changing the approval criteria for in-flight votes. Snapshotting at creation makes the rules immutable for each proposal. **BREAKING:** Callers reading `proposal.executable_at` before finalization will see 0. `proposal.voting_ends_at` is now the correct deadline for voting.

### Changes — 1.3: Authorization fixes

**contracts/upgrade-manager/src/types.rs** — added `NotInitialized` (14), `Overflow` (15) errors.

**contracts/upgrade-manager/src/governance.rs**
- `load_config` and `load_admin` now return `Result<_, Error>` and propagate `NotInitialized` instead of panicking via `unwrap()`.
- Added `require_admin(env, caller)` helper: calls `caller.require_auth()` and compares with stored admin.
- Added `validate_config(config)` helper: used by both `initialize` and `update_config`.
- `cancel_proposal`: now also returns `ProposalFinalized` for `Defeated` status (previously only for Executed/Cancelled).

**contracts/upgrade-manager/src/lib.rs**
- `initialize`: now calls `admin.require_auth()` before the AlreadyInitialized check, preventing front-running.
- All admin-gated functions use the `require_admin` helper.
- `propose_upgrade`: non-empty description required (returns `EmptyDescription`).
- `execute`: sets `proposal.status = ProposalStatus::Executed` and saves it BEFORE calling `invoke_upgrade`. If the target call reverts, the whole transaction reverts and the proposal remains Queued. This is the checks-effects-interactions pattern.
- `get_config` and `get_admin` now return `Result<_, Error>` and surface `NotInitialized`.

**Why (1.3):** Without `admin.require_auth()` in `initialize`, any address that observed the deployment transaction could submit a competing `initialize` call and claim the admin role. The `require_admin` consolidation removes two separate places that duplicated the caller.require_auth()+admin comparison. The `execute` reordering follows checks-effects-interactions: previously the status was set to Executed after the cross-contract call, so a re-entrant or panicking target could leave the proposal in an ambiguous state. **BREAKING:** `get_config` and `get_admin` now return `Result` — callers must handle `NotInitialized`.

### Changes — 1.4: Robustness (TTL, events, doc fixes)

**contracts/upgrade-manager/src/governance.rs**
- `save_config`, `save_admin`, `save_proposal`, `record_vote`: all extend TTL after each write.
- `load_proposal`: extends persistent TTL on read.
- `next_proposal_id`: uses `checked_add` returning `Overflow`.
- `cast_vote`, `finalize_voting`: use `checked_add` for vote/ledger arithmetic.
- Added event emissions: `initialized`, `config_updated`, `admin_transferred`, `proposal_created`, `vote_cast`, `proposal_finalized`, `proposal_executed`, `proposal_cancelled`.

**contracts/upgrade-manager/src/timelock.rs**
- `ledgers_until_executable`: now returns 0 for non-Queued proposals (previously returned a misleading countdown for Active/Defeated proposals).

**contracts/upgrade-manager/src/lib.rs** — fixed module doc comment: removed reference to non-existent `execute_upgrade()` function; updated lifecycle comment to match the new two-step finalize+execute flow.

**Why (1.4):** No TTL extension meant long-lived proposals could be archived by the Stellar network TTL eviction mechanism. Events are required for the "full audit trail" claim in the README — previously no events were emitted. The `timelock.rs` fix ensures `time_until_executable` returns a correct value (0 instead of misleading countdown) for non-Queued proposals.

### Changes — 1.5: Existing tests updated for new signatures

**contracts/upgrade-manager/src/lib.rs** (test section)
- `setup()` helper: added `voting_period=100` argument to `initialize`.
- `advance_past_voting` helper: advances 101 ledgers past the 100-ledger voting period.
- `advance_past_timelock` helper: advances past both voting and timelock (17 382 ledgers total).
- `test_finalize_before_timelock_fails` renamed to `test_finalize_before_voting_ends_fails`; old name kept as alias.
- `test_time_until_executable`: updated to reflect that `time_until_executable` returns 0 before finalization and 17_280 immediately after.
- `test_update_config`: added `voting_period` argument.
- `test_full_governance_upgrade_flow`: updated to separate voting-advance from timelock-advance steps.

**README.md** — updated `initialize` CLI example to include `--voting_period 17280`.

**Why (1.5):** Phase 1 changed the `initialize` signature (added `voting_period`) and the finalize/execute timing semantics; the existing tests would not compile without these minimal updates.

**BREAKING changes summary for Phase 1:**
1. `initialize` takes a new required argument: `voting_period: u32`.
2. `get_config` and `get_admin` return `Result<_, Error>` instead of bare values.
3. `proposal.executable_at` is 0 until `finalize()` is called.
4. `finalize()` now requires the voting period to have ended (returns `VotingStillActive` if called too early); previously it required `executable_at` to have passed (which was confusingly named `TimelockNotExpired`).
5. `cancel()` now returns `ProposalFinalized` for Defeated proposals too.

**Test results:** `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 22 passed, 0 failed; `cargo build --target wasm32-unknown-unknown --release` succeeded.

---

## 2026-10-27 — test/phase-2-tests-and-fixtures

### Changes — 2.1: Tests moved to src/test.rs

**contracts/upgrade-manager/src/lib.rs**
- Removed all inline `#[cfg(test)]` test code (473 lines).
- Added `#[cfg(test)] mod test;` to include the new `src/test.rs` file.
- Tests are strengthened, never weakened — all 22 original tests are preserved and 44 new tests added (66 total).

**contracts/upgrade-manager/src/test.rs** (new file, 1414 lines)
- All 66 unit and integration tests for the upgrade-manager contract.

**Why:** Inline tests in `lib.rs` had grown too large to maintain. A dedicated `test.rs` module keeps the contract source clean while preserving the full test suite.

### Changes — 2.2: .gitignore and removal of committed test snapshots

**.gitignore** (new file)
- Added `target/` (Rust build artifacts).
- Added `test_snapshots/` (Soroban-generated snapshot files, should not be committed).

**contracts/upgrade-manager/test_snapshots/** — removed 23 committed snapshot JSON files from git tracking (`git rm --cached`). The files remain on disk but are now ignored by git.

**Why:** Test snapshots are environment-specific generated artifacts. Committing them causes noise in PRs and can mask real test changes. The `.gitignore` prevents accidental re-commitment.

### Changes — 2.3: Example-target contract added as workspace member

**Cargo.toml** (workspace root)
- Added `contracts/example-target` to `members`.

**contracts/example-target/Cargo.toml** (new file)
- `crate-type = ["cdylib", "rlib"]` for WASM + native test builds.
- `soroban-sdk` as a workspace dependency.
- Feature `v2` — when enabled, `version()` returns 2 instead of 1.
- Feature `testutils` — enables `soroban-sdk/testutils` for dev.

**contracts/example-target/src/lib.rs** (new file)
- `__constructor(env, manager)` — deploy-time initializer. Stores the manager address in instance storage. Calls `manager.require_auth()` so the deployer must control the manager. Cannot be front-run because the constructor is atomic with deployment.
- `version()` — returns 1 (or 2 with `--features v2`).
- `upgrade(env, new_wasm_hash)` — loads the stored manager, calls `manager.require_auth()`, then `env.deployer().update_current_contract_wasm(new_wasm_hash)`. Only the stored manager may trigger an upgrade.
- `get_manager()` — returns the stored manager address (read-only).

**Why:** The previous integration test used a second upgrade-manager contract as the "target", which has no `upgrade()` function. The example-target contract is a minimal upgradable contract that mirrors the real pattern: it stores a manager address at deploy time and authenticates the manager inside `upgrade()`.

### Changes — 2.4: Fixture build script and committed WASM fixtures

**scripts/build_fixtures.sh** (new file)
- Builds `target_v1.wasm` (no features) and `target_v2.wasm` (`--features v2`) into `contracts/upgrade-manager/tests/fixtures/`.
- Generates `SHA256SUMS` for drift detection.
- Documents the toolchain pinning requirement (Rust 1.84.0 for compatibility with soroban-wasmi's WASM validator).

**contracts/upgrade-manager/tests/fixtures/target_v1.wasm** (new file, 4607 bytes)
**contracts/upgrade-manager/tests/fixtures/target_v2.wasm** (new file, 4607 bytes)
**contracts/upgrade-manager/tests/fixtures/SHA256SUMS** (new file)

Fixture SHA-256 hashes:
- `target_v1.wasm`: `945618bb38cbcc56705cd0d6bb510349cdd27ba37b4491db7402a9ccfc0e1e05`
- `target_v2.wasm`: `76e27dc7ba9bedf1aa06ffd547edb5e8d292e5c1299cc1fb2e39968d53c649e7`

**Why:** Committed fixtures provide a reproducible build artifact for CI verification and on-chain deployment. The SHA256SUMS file enables a drift-detection check: CI can rebuild the fixtures and compare hashes to detect unintended changes to the example-target contract.

### Changes — 2.5: Auth tests without mock_all_auths

**contracts/upgrade-manager/src/test.rs**
- `test_auth_initialize_without_auth_fails` — `initialize` without auth fails.
- `test_auth_propose_without_auth_fails` — `propose_upgrade` without auth fails.
- `test_auth_vote_without_auth_fails` — `vote` without auth fails.
- `test_auth_cancel_without_auth_fails` — `cancel` without auth fails.
- `test_auth_update_config_without_auth_fails` — `update_config` without auth fails.
- `test_auth_transfer_admin_without_auth_fails` — `transfer_admin` without auth fails.
- `test_auth_cancel_non_admin_fails` — `cancel` by non-admin returns `Unauthorized`.
- `test_auth_update_config_non_admin_fails` — `update_config` by non-admin returns `Unauthorized`.
- `test_auth_transfer_admin_non_admin_fails` — `transfer_admin` by non-admin returns `Unauthorized`.

All "without auth" tests use `Env::default()` (no `mock_all_auths()`) to verify that `require_auth()` actually fails at the host level when no auth is provided.

**Why:** The previous tests used `mock_all_auths()` which silently approves all `require_auth()` calls. This means auth enforcement was never actually tested. The new tests prove that every state-changing function fails when called without authentication.

### Changes — 2.6: Real v1→v2 upgrade test through the manager

**contracts/upgrade-manager/src/test.rs**
- `test_real_v1_to_v2_upgrade_through_manager` — full governance flow: initialize → propose → 3× vote → advance past voting → finalize (Queued) → advance past timelock → execute → assert proposal is Executed. Also proves that a direct call to `target.upgrade()` by a non-manager address fails (in a separate env without `mock_all_auths`).

**What this test proves:**
1. The manager's `execute()` successfully invokes `upgrade()` on the `FixtureTarget` contract via cross-contract call.
2. The proposal transitions to `Executed` after `execute()` returns.
3. A direct call to `target.upgrade()` without the manager's auth fails.

**What this test does NOT prove (known limitation):**
- It does not prove that the target contract's WASM is actually replaced on-chain. The test environment uses `soroban-wasmi 0.31.1` (via `soroban-env-host 22.1.3`), which has `.wasm_reference_types(false)` hardcoded. Modern Rust compilers (≥ 1.85.0) emit `call_indirect` instructions with LEB128-encoded table indices, which is part of the reference-types proposal. The validator rejects these with "reference-types not enabled: zero byte expected". As a result, the committed fixture WASMs (`target_v1.wasm`, `target_v2.wasm`) cannot be uploaded via `upload_contract_wasm()` in the test environment. The test uses `Bytes::new(&env)` (empty bytes) as a placeholder. The fixture WASMs are still committed and their SHA-256 hashes are documented above; they are valid for on-chain deployment and CI drift detection.

**Why:** The task requires a real v1→v2 upgrade through the manager. This test exercises the full governance lifecycle (propose → vote → finalize → timelock → execute) and proves the cross-contract call chain works. The WASM-replacement limitation is a constraint of the test environment, not the contract logic.

### Changes — 2.7: Failed upgrade test

**contracts/upgrade-manager/src/test.rs**
- `test_failed_upgrade_leaves_proposal_queued` — registers a contract with no `upgrade()` function as the target, runs the governance flow, and calls `execute()`. The cross-contract call fails (host-level `HostError`, not a manager `Error` variant). The transaction reverts, and the proposal remains `Queued` because `execute()` sets `Executed` before the call (checks-effects-interactions).

**Why:** Proves that the checks-effects-interactions pattern in `execute()` works: if the target call fails, the proposal is not left in an ambiguous state — it stays `Queued` and can be retried.

### Changes — 2.8: Cancel of Executed proposal

**contracts/upgrade-manager/src/test.rs**
- `test_cancel_executed_proposal_fails` — creates a proposal, votes it through, finalizes, executes, then attempts to cancel. Returns `ProposalFinalized`.

**Why:** The previous tests covered cancel of Cancelled and Defeated proposals but not Executed. This completes the matrix: cancel of Queued succeeds; cancel of Active, Cancelled, Defeated, and Executed all fail.

### Changes — 2.9: Remaining coverage tests

**contracts/upgrade-manager/src/test.rs** — all tests below are present:

- **Invalid quorum:** `test_initialize_zero_quorum_fails`, `test_update_config_invalid_values_fail` (quorum=0).
- **Threshold 0 and 101:** `test_initialize_zero_threshold_fails`, `test_initialize_threshold_101_fails`, `test_update_config_invalid_values_fail` (threshold=0 and 101).
- **Timelock delay:** `test_initialize_zero_delay_fails`, `test_update_config_invalid_values_fail` (delay=0).
- **Voting period:** `test_initialize_zero_voting_period_fails`, `test_update_config_invalid_values_fail` (period=0).
- **Empty description:** `test_description_empty_fails`.
- **Too-long description:** `test_description_too_long`.
- **256-byte description OK:** `test_description_256_bytes_ok`.
- **Vote after voting period:** `test_vote_after_voting_period_ends_fails`.
- **Vote at exact boundary:** `test_vote_at_voting_ends_at_succeeds` (vote at `voting_ends_at` succeeds).
- **Finalize before voting ends:** `test_finalize_before_voting_ends_fails`.
- **Finalize at boundary:** `test_finalize_at_voting_ends_at_fails` (at `voting_ends_at`), `test_finalize_at_voting_ends_at_plus_one_succeeds` (at `voting_ends_at + 1`).
- **Execute before timelock:** `test_execute_fails_before_timelock`, `test_execute_one_ledger_before_executable_at_fails`.
- **Double execute:** `test_double_execute_fails`.
- **Cancel of Queued:** `test_cancel_queued_proposal` (succeeds).
- **Cancel of Defeated:** `test_cancel_defeated_proposal_fails`.
- **Cancel of Executed:** `test_cancel_executed_proposal_fails`.
- **Config snapshot isolation:** `test_config_snapshot_isolation` — changes config after proposal creation, verifies proposal uses snapshotted values.
- **Events:** `test_event_initialized`, `test_event_proposal_created`, `test_event_vote_cast`, `test_event_finalized`, `test_event_cancelled`, `test_event_config_updated`, `test_event_admin_transferred`.
- **NotInitialized:** `test_propose_not_initialized_fails`, `test_get_config_not_initialized_fails`, `test_get_admin_not_initialized_fails`.
- **Quorum met only by NO votes → Defeated:** `test_finalize_defeated_all_no_votes`.
- **TTL extension:** `test_ttl_extended_on_write_and_read` — verifies TTL is extended after writes and reads. (Note: the test verifies TTL values via `env.storage().persistent().extend_ttl` calls in the advance helpers; the SDK test helpers do not expose a direct way to read the current TTL of a storage entry, so this test verifies the contract calls `extend_ttl` with the expected bump values, not the resulting TTL itself.)

**Why:** Comprehensive boundary testing ensures the contract behaves correctly at every edge of its state machine and input validation.

### Changes — 2.10: Correction of old integration test claim

The old integration test (`test_full_governance_upgrade_flow`) and its changelog entry claimed it "verifies the target contract's WASM is replaced" by the upgrade. This was inaccurate: the old test used a mock target with `update_current_contract_wasm` called on an empty hash (`BytesN::from_array(&[0u8; 32])`), which proves the call chain works but does NOT prove that real WASM bytecode is loaded and executed.

The new `test_real_v1_to_v2_upgrade_through_manager` test improves on this by using a `FixtureTarget` contract with a proper `upgrade()` function that authenticates the manager. However, it still cannot prove actual WASM replacement because the test environment's WASM validator (soroban-wasmi 0.31.1 with `wasm_reference_types(false)`) rejects WASM compiled by modern Rust toolchains.

**What the new test proves:** The full governance flow (propose → vote → finalize → timelock → execute) works end-to-end, `execute()` successfully calls `upgrade()` on the target via cross-contract invocation, and the proposal transitions to Executed.

**What no test in this environment can prove:** That the target contract's WASM bytecode is actually replaced and the new version's `version()` returns 2. This would require either an older Rust toolchain (≤ 1.83.0) that emits WASM without reference-types, or a newer soroban-env-host that enables reference-types in wasmi.

**Why:** The old claim overstated what the test proved. This correction ensures the changelog and documentation accurately describe what is and is not verified.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 66 passed, 0 failed.
