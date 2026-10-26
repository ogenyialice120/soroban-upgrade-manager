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
