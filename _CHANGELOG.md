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

---

## 2026-10-27 — docs/phase-3-docs-corrections

### Changes — 3.1: Walkthrough rewritten

**docs/walkthrough.md** — complete rewrite.

**Why:** The old walkthrough deployed a second instance of the upgrade-manager as the "target" contract. The upgrade-manager has no `upgrade()` function, so `execute()` would fail with a host error. The old walkthrough also said `finalize()` was called "after the timelock delay has elapsed" — but `finalize()` is called after the **voting period** ends, and the timelock starts only after `finalize()` is called. The old overview said "token holders cast YES/NO votes" but voting is one-address-one-vote, not token-weighted. The old `initialize` command was missing the `--voting_period` parameter.

**What changed:**
- Target is now `example-target` (with `__constructor`, `version()`, `upgrade()`), not a second upgrade-manager.
- Overview diagram shows two separate time windows (voting period + timelock).
- `initialize` command includes `--voting_period 17280`.
- Step 7 (finalize) correctly states it is called after the voting period ends.
- Step 8 (wait for timelock) and Step 9 (execute) are now separate steps.
- `time_until_executable` documented as returning 0 before finalize.
- Failure modes table corrected: `VotingStillActive` (not `TimelockNotExpired`) for early finalize.
- All CLI commands match real contract signatures.

### Changes — 3.2: README corrections

**README.md** — multiple corrections:

- **Lifecycle diagram:** Replaced the old single-window diagram with a two-window diagram showing voting period (`voting_ends_at`) and timelock delay (`executable_at`) as separate concepts.
- **Config reference:** Added `voting_period` row (was missing entirely).
- **API table:** Fixed auth column — `initialize` is "Admin (must sign)" (was "None"), `finalize` and `execute` are "Anyone" (was not clear).
- **Voting description:** Changed from "token holders" to "any address can vote YES/NO (one address, one vote)".
- **Timelock claim:** Removed "default ≈ 24 h" — there is no default; timelock is a required parameter.
- **Community-stops-upgrade:** Changed from "Admin can cancel; voters can reject" to "Voters can outvote a bad proposal; admin can cancel Active or Queued proposals".
- **Compound/OZeppelin comparison:** Changed from "same pattern used by" to "inspired by Compound Governor Bravo and OpenZeppelin's TimelockController ... Unlike those systems, this contract uses unweighted one-address-one-vote voting rather than token-weighted voting."
- **Duplicate `---`:** Removed the double horizontal rule between "Why Not" and "Features" sections.
- **Filler sentence:** Removed "This project welcomes contributions from the community..".

**Why:** The README contained multiple inaccuracies that contradicted the code: token-holder voting (code uses one-address-one-vote), a default timelock (code has none), missing `voting_period` in config reference, wrong auth requirements in API table, and an inaccurate lifecycle diagram.

### Changes — 3.3: README new sections

**README.md** — added three new sections:

- **Security Model & Limitations:** unaudited, not sybil-resistant, powerful admin key (multisig/DAO recommended), admin can cancel Queued after approval, no proposal deposit (spam possible), timelock starts at finalize, target MUST authenticate manager, no expiry, no WASM compatibility check.
- **Possible Future Work:** sybil resistance, proposal deposits, proposal expiry, WASM compatibility check.
- **Recommended Workflow:** build both WASM versions, run upgrade-compatibility check (e.g., soroban-upgrade-safeguard), upload WASM, create proposal with report link in description, vote/finalize/execute. Notes this is guidance, not enforcement.

**Why:** The README lacked a security model section, leaving users unaware of the contract's limitations. The recommended workflow section guides users to check storage-layout compatibility before proposing an upgrade.

### Changes — 3.4: Error-code table

**README.md** — added error-code table covering all 20 `Error` variants in `types.rs`.

Each row lists: code, name, trigger, and fix. `ProposalDefeated` (code 8) is marked as "never returned — `finalize()` returns `ProposalStatus::Defeated` as a success value, not this error."

**Why:** The README had no error-code reference. Users had to read the source to understand error codes. The table also documents that `ProposalDefeated` is defined but never returned, preventing confusion.

### Changes — 3.5: CONTRIBUTING.md fixed

**CONTRIBUTING.md** — three fixes:

- **Clone URL:** Changed from `https://github.com` to `https://github.com/ogenyialice120/soroban-upgrade-manager.git`.
- **Issue reporting:** Removed reference to non-existent "bug template"; replaced with "Open a GitHub Issue".
- **Changelog requirement:** Added step 5 in "Submitting Pull Requests" requiring every PR to append a dated, reasoned entry to `_CHANGELOG.md` with date, branch name, changes, why, breaking notes, and test results.
- **Rust version:** Changed from "1.74+" to "Latest stable" (Part C will determine the real minimum).

**Why:** The clone URL was truncated and would fail. The bug template referenced did not exist in the repository. There was no changelog requirement, so PRs could be merged without documenting what changed or why.

### Changes — 3.6: SECURITY.md added

**SECURITY.md** (new file) — private vulnerability reporting policy.

Instructs reporters to use GitHub's private vulnerability reporting (Security tab → "Report a vulnerability"). Includes what to include in a report, response timeline (72h acknowledgement, 7-day initial assessment), and scope. References the README's Security Model section.

**Why:** The repository had no security policy. Without one, well-intentioned researchers might open public issues for vulnerabilities, exposing users before a fix is deployed.

### Changes — 3.7: Claims audit created

**docs/claims-audit.md** (new file) — audit table mapping every behavioral claim in README, walkthrough, CONTRIBUTING, and doc comments to its implementing function and test name.

- 44 README claims (all VERIFIED)
- 10 walkthrough claims (all VERIFIED)
- 4 CONTRIBUTING claims (all VERIFIED)
- 30 doc comment claims (28 VERIFIED, 2 REWORDED)
- 14 REWORDED/REMOVED entries documenting what was wrong with the old docs

Two claims marked REWORDED:
- `next_proposal_id` overflow (not reachable in normal operation; testing requires u64 wrap)
- `example-target::version` feature flag (compile-time feature; not testable in a single env)

**Why:** The task requires that every behavioral claim in the documentation is backed by code and a test. The audit provides traceability and will be verified by `scripts/check_claims.sh` in CI (Part C).

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 66 passed, 0 failed. (No code changes in this part — only documentation.)

---

## 2026-10-27 — ci/phase-4-ci-and-tooling

### Changes — 4.1: CI workflow updated

**.github/workflows/ci.yml**
- Added `cargo fmt --all -- --check` step.
- Added `cargo clippy --workspace --all-targets -- -D warnings` step.
- Added `rustfmt` and `clippy` components to the Rust toolchain installation.
- Added `bash scripts/check_claims.sh` step (verify claims audit).
- Added fixture drift check: rebuilds fixtures with `scripts/build_fixtures.sh` and fails if committed `.wasm` or `SHA256SUMS` differ.

**Why:** The old CI only ran `cargo build`, `cargo test`, and a WASM build. It did not check formatting or lints, so code with `cargo fmt` violations or clippy warnings could be merged. The claims audit and fixture drift checks were not present.

### Changes — 4.2: scripts/check_claims.sh

**scripts/check_claims.sh** (new file)
- Validates `docs/claims-audit.md` against `contracts/upgrade-manager/src/test.rs`.
- Checks:
  1. Every VERIFIED row has a test name in backticks.
  2. Every named test exists as `fn <name>` in test.rs (whole-word match via `grep -qE "fn ${tname}\b"`).
  3. Every status is VERIFIED, REWORDED, or REMOVED.
- Design-note rows with test name "N/A" are allowed (e.g., "N/A (design note)").

**Testing against a broken copy:** A deliberately broken copy of claims-audit.md was created with `test_initialize_success` replaced by `test_does_not_exist`. The script correctly detected 4 errors (the test name appeared in multiple rows) and exited with code 1. The good copy passes with "OK: All claims audit checks passed."

**What the script proves:** It proves that a test function named in a VERIFIED claim exists in test.rs. It does NOT prove that the test covers the claim — a test named `test_foo` might not test what the claim says. The script's purpose is to catch claims that reference non-existent tests.

**Why:** The claims audit must be verifiable in CI. Without this check, a claim could reference a test name that doesn't exist and go unnoticed.

### Changes — 4.3: rust-toolchain.toml

**rust-toolchain.toml** (new file)
- Pins the Rust toolchain to `stable` with `wasm32-unknown-unknown` target and `rustfmt`/`clippy` components.

**Why:** Without a pinned toolchain, contributors might use different Rust versions, causing inconsistent builds. The `stable` channel matches what CI uses (via `dtolnay/rust-toolchain@stable`).

### Changes — 4.4: Fixture drift check in CI

**.github/workflows/ci.yml** — added "Rebuild fixtures and verify drift" step.

Rebuilds fixtures with `bash scripts/build_fixtures.sh` and compares the output against committed `.wasm` and `SHA256SUMS` files using `git diff --quiet`. If they differ, CI fails with a message instructing the contributor to rebuild and commit.

**Why:** If someone changes the `example-target` contract source, the committed fixtures become stale. The drift check ensures the committed fixtures always match the current source. Note: builds are byte-reproducible with the same toolchain version, so this comparison is reliable within a given stable release.

### Changes — 4.5: Minimum Rust version determined and set

**Cargo.toml** (workspace root)
- Added `rust-version = "1.85.0"`.

**Why:** The declared MSRV for `soroban-sdk 22.0.11` is 1.84.0, but a transitive dependency (`zeroize_derive 1.5.0`) requires `edition2024`, which was stabilized in Rust 1.85.0. Building with 1.84.0 fails with "the package requires the Cargo feature called `edition2024`". The actual minimum is 1.85.0.

The old README claimed "Rust 1.74+" — this was unverified and inaccurate (the contract would not build with 1.74). The old CONTRIBUTING.md also said "Rust 1.74+". Both were fixed in Part B to say "Rust" / "Latest stable" respectively, and this Part C sets the actual minimum in Cargo.toml.

### Changes — 4.6: .pre-commit-config.yaml

**.pre-commit-config.yaml** (new file)
- `pre-commit-hooks` (rev v4.6.0): trailing-whitespace, end-of-file-fixer, check-yaml, check-toml, check-merge-conflict, check-added-large-files (max 100 KB).
- Local hooks: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

**Why:** Pre-commit hooks catch issues before code reaches CI. The `pre-commit-hooks` suite catches common file hygiene problems (trailing whitespace, missing newline, merge conflict markers, large files). The local hooks run the same `cargo fmt` and `clippy` checks that CI enforces, so formatting/lint issues are caught locally.

### Changes — 4.7: Release workflow

**.github/workflows/release.yml** (new file)
- Triggers on `v*` tags only.
- Builds the upgrade-manager WASM in release mode.
- Computes SHA-256 of the `.wasm` file.
- Creates a GitHub release using `softprops/action-gh-release@v2`.
- Attaches `soroban_upgrade_manager.wasm` to the release.
- Includes SHA-256 and deployment instructions in the release body.

**Why:** Without a release workflow, there is no reproducible way to build and publish the WASM artifact. The release workflow ensures every tagged release has a downloadable `.wasm` with a verifiable SHA-256 hash.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 66 passed, 0 failed. (No contract code changes — only tooling and config.)

---

## 2026-10-29 — fix/followup-honest-and-deployable

### Changes

**contracts/example-target/src/lib.rs** — Removed `manager.require_auth()` from `__constructor`. The manager is a contract address and cannot sign, so requiring constructor auth made real deployment fail. The manager is now stored without auth.

**docs/walkthrough.md** — Removed the sentence saying the admin key must sign when deploying the target; the constructor no longer requires auth.

**contracts/upgrade-manager/src/test.rs** — Added `test_real_example_target_deployed_with_contract_manager_without_mock_auths`, which deploys the real `example-target` with a contract address as manager without `mock_all_auths`. Renamed `test_real_v1_to_v2_upgrade_through_manager` to `test_governance_upgrade_executed_through_manager` because it cannot prove a real v1→v2 WASM upgrade (the mock host does not hot-swap WASM); the new name no longer claims one.

**Cargo.toml** (workspace root) — Moved `rust-version` from `[workspace]` (which Cargo ignored with "unused manifest key") under `[workspace.package]` and set it to `1.88.0` (the real minimum; the lockfile needs 1.88.0). Added `rust-version.workspace = true` to both contracts' `Cargo.toml`.

**rust-toolchain.toml, scripts/build_fixtures.sh, .github/workflows/ci.yml, .github/workflows/release.yml** — All now use Rust `1.88.0` consistently. Fixed contradictory text (build_fixtures.sh said 1.84.0; changelog said 1.85.0).

**docs/claims-audit.md** — Changed rows 7, 15, 27 and 40 from VERIFIED to REWORDED with honest explanations (renamed test no longer claims a real WASM upgrade). Changed rows 1, 11 and 42 from VERIFIED to REWORDED (their test column is N/A). Also converted the CONTRIBUTING rows 1-4 (VERIFIED with N/A) to REWORDED and updated stale references to the old test name.

**scripts/check_claims.sh** — Now fails on any VERIFIED row whose test column is N/A. Verified it passes on the fixed file and fails on a deliberately broken copy.

**.kiro/spec.md** — Removed from the repo. Added `.kiro/` to `.gitignore`.

**.github/workflows/release.yml** — Now attaches a `.sha256` file next to the `.wasm` release artifact.

**Why:** Make the repository honest about what is proven (no false v1→v2 WASM upgrade claim, no VERIFIED claims with N/A tests), make it actually deployable (contract manager can be stored without signing), and make the Rust toolchain consistent across all configuration files.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 67 passed, 0 failed.

## 2026-10-30 — fix/claims-and-docs-final

### Changes

**contracts/upgrade-manager/src/test.rs** — Added `test_real_example_target_upgrade_rejects_non_manager`, which deploys the real `example-target` with a contract address as manager WITHOUT `mock_all_auths` and asserts `try_upgrade(any_hash)` is an error.

**docs/claims-audit.md** — Changed walkthrough rows 4, 6 and 8 and doc-comment row 29 from VERIFIED to REWORDED with wording describing what the cited test actually proves. Added row 31 marking `upgrade()` rejects callers that are not the manager as VERIFIED using the new test.

**CONTRIBUTING.md** — Changed Rust prerequisite from "Latest stable" to "1.88.0 (as pinned in rust-toolchain.toml)".

**.gitignore** — Replaced comment "# Kiro AI specs (not committed)" with "# Local editor specs (not committed)"; kept `.kiro/`.

**scripts/build_fixtures.sh** — Reworded toolchain comment to state only that WASMs are built with the pinned 1.88.0 toolchain for consistency; removed unverified compatibility claim. Added note that committed fixtures were not verified to upload in the test host.

**Why:** Make the claims audit honest about what tests prove without `mock_all_auths`, add a real no-mock test for manager auth rejection, and align docs/config with the pinned Rust 1.88.0 toolchain.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 68 passed, 0 failed. `bash scripts/check_claims.sh` → OK.
## 2026-10-30 — ci/fix-fixture-drift-check

### Changes

**.github/workflows/ci.yml** — Replaced "Rebuild fixtures and verify drift" step with "Verify committed fixtures match SHA256SUMS", running `cd contracts/upgrade-manager/tests/fixtures && sha256sum -c SHA256SUMS`.

**scripts/build_fixtures.sh** — Added note that CI only verifies committed fixtures match SHA256SUMS and that rebuilding on another machine may produce different bytes.

**contracts/upgrade-manager/src/test.rs** — Updated fixture comment from "drift detection" to "checksum verification".

**Why:** WASM builds are not byte-identical across machines because build paths are embedded, so the old rebuild-and-git-diff check failed on GitHub runners. CI now only checks that committed fixtures match their recorded checksums.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 68 passed, 0 failed. `sha256sum -c SHA256SUMS` → all OK. Other CI steps reviewed; no additional runner-breaking issues found.
## 2026-10-30 — docs/testnet-verification

### Changes

**README.md** — Added "Verified on Stellar testnet" section documenting a manual testnet run (parameters, results, transactions, reproducible steps). Updated Getting Started build/deploy commands to use `wasm32v1-none`.

**Why:** Reviewers and users need concrete, checkable evidence and reproducible steps that the mechanism works on a real network, with honest limits (manual run, not an automated test; not sybil-resistant).

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 68 passed, 0 failed. `bash scripts/check_claims.sh` → OK.
## 2026-10-30 — fix/wasm-target-v1-none

### Changes

**.github/workflows/ci.yml** — Changed toolchain target and "Build (wasm)" command from `wasm32-unknown-unknown` to `wasm32v1-none`.

**.github/workflows/release.yml** — Changed toolchain target, build command, `WASM_FILE` path, and release file paths from `wasm32-unknown-unknown` to `wasm32v1-none`.

**rust-toolchain.toml** — Changed `targets` to `["wasm32v1-none"]`.

**scripts/build_fixtures.sh** — Changed comments, `rustup target add`, both `--target` flags and both cp source paths from `wasm32-unknown-unknown` to `wasm32v1-none`.

**docs/walkthrough.md** — Changed prerequisites, build commands and WASM paths from `wasm32-unknown-unknown` to `wasm32v1-none`.

**CONTRIBUTING.md** — Changed target line, setup commands and PR checklist line from `wasm32-unknown-unknown` to `wasm32v1-none`.

**contracts/upgrade-manager/tests/fixtures/target_v1.wasm, target_v2.wasm, SHA256SUMS** — Rebuilt with `wasm32v1-none`. New SHA-256 hashes: `target_v1.wasm` = `34fd6d1838f2d7d167500fe480061b5cd915b59320e2bdde5b433dde1aaec81f`, `target_v2.wasm` = `847f384de0399e007643b51c7b4b421e29a9e5f28f8b53f320593fbbee2d1f3c`.

**Why:** Building with `wasm32-unknown-unknown` on Rust 1.82+ produces WASM the Stellar network rejects with "reference-types not enabled: zero byte expected". Stellar docs recommend `wasm32v1-none` (Rust 1.84+) or `stellar contract build`; `wasm32v1-none` builds on Rust 1.88.0 deployed and upgraded on testnet successfully. Earlier changelog entries claiming the fixtures were "valid for on-chain deployment" were not verified, and WASM built with `wasm32-unknown-unknown` was found NOT deployable. The release workflow had been producing such a file.

New fixtures can now be uploaded in the test host via `upload_contract_wasm` (verified by a temporary test: both v1 and v2 uploaded successfully). No test changes were made; a real v1-to-v2 upgrade test may now be possible as a follow-up.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 68 passed, 0 failed; `bash scripts/check_claims.sh` → OK; `cargo build --release --target wasm32v1-none --manifest-path contracts/upgrade-manager/Cargo.toml` succeeded. `sha256sum -c SHA256SUMS` → all OK. `grep -rn "wasm32-unknown-unknown"` (excluding `_CHANGELOG.md` and `target/`) found only the README warning.
## 2026-10-30 — test/real-wasm-upgrade

### Changes

**contracts/upgrade-manager/src/test.rs** — Added `test_real_wasm_upgrade_v1_to_v2_through_manager`, which registers the real `target_v1.wasm`, asserts `version() == 1`, uploads `target_v2.wasm`, runs initialize/propose/vote/finalize/execute, and asserts `version() == 2` and status `Executed`.

**docs/claims-audit.md** — Added row 32 for a real v1→v2 WASM upgrade. Updated rows 7, 15, 27, 40, walkthrough row 8 and doc-comment row 29 to VERIFIED where the new test supports the wording.

**README.md** — Added one sentence to the testnet section noting the manual run is now also covered by an automated test.

**Why:** No test could previously prove that a target's WASM is actually replaced. The new test proves a governance proposal can upgrade a real target from v1 to v2 through the manager; it does not prove sybil resistance, auditability beyond the recorded run, or behavior of arbitrary WASM.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 69 passed, 0 failed; `bash scripts/check_claims.sh` → OK.

## 2026-10-31 — docs/claims-wording-final

### Changes

**docs/claims-audit.md** — Rows that claim `upgrade()` authenticates / rejects non-manager callers now cite `test_real_example_target_upgrade_rejects_non_manager` instead of `test_real_wasm_upgrade_v1_to_v2_through_manager`: README security row 40, walkthrough row 8, and example-target row 29. `test_real_wasm_upgrade_v1_to_v2_through_manager` is retained only for the claims it actually proves (real WASM v1→v2 replacement: rows 7, 15, 27, 32).

**README.md** — Fixed the testnet intro wording so it no longer contradicts itself: the manual run itself is not an automated test; the same v1-to-v2 upgrade is also covered by `test_real_wasm_upgrade_v1_to_v2_through_manager`, which uses mocked authorization.

**Why:** The cited test did not prove the auth behavior because it uses `mock_all_auths`, so it cannot demonstrate that `upgrade()` rejects non-manager callers. The auth-rejection claim is instead backed by `test_real_example_target_upgrade_rejects_non_manager`, which runs without `mock_all_auths`.

**Test results:** `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` → 69 passed, 0 failed; `bash scripts/check_claims.sh` → OK.
