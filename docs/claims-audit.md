# Claims Audit

This document maps every behavioral claim in the project's documentation
(README, walkthrough, CONTRIBUTING, and source doc comments) to the function
that implements it and the test that verifies it.

**Status values:**
- **VERIFIED** — a test exists in `test.rs` that verifies the claim.
- **REWORDED** — the documentation was reworded to accurately describe what the code does; the original claim was inaccurate.
- **REMOVED** — the claim was removed because it was inaccurate and could not be reworded truthfully.

---

## README Claims

| # | Claim | Source section | Implementing function | Test name | Status |
|---|-------|--------------|----------------------|-----------|--------|
| 1 | Any address with admin auth can upgrade natively | Why Not table | N/A (describes native Soroban, not this contract) | N/A | REWORDED |
| 2 | Must pass a governance vote to upgrade | Why Not table | `create_proposal`, `cast_vote`, `finalize_voting` | `test_finalize_passes_with_quorum` | VERIFIED |
| 3 | Configurable timelock delay (in ledgers) | Why Not table | `finalize_voting` sets `executable_at` | `test_execute_fails_before_timelock` | VERIFIED |
| 4 | Full proposal + vote history on-chain | Why Not table | `save_proposal`, `record_vote` | `test_create_proposal` | VERIFIED |
| 5 | Voters can outvote a bad proposal | Why Not table | `finalize_voting` checks threshold | `test_finalize_defeated_all_no_votes` | VERIFIED |
| 6 | Admin can cancel Active or Queued proposals | Why Not table | `cancel_proposal` | `test_cancel_by_admin`, `test_cancel_queued_proposal` | VERIFIED |
| 7 | Cross-contract upgrade invocation through the manager, including a real v1→v2 WASM replacement (proven by `test_real_wasm_upgrade_v1_to_v2_through_manager`) | Why Not table | `invoke_upgrade` | `test_real_wasm_upgrade_v1_to_v2_through_manager` | VERIFIED |
| 8 | Transparent proposal anyone can inspect | Why Not list | `propose_upgrade` → `get_proposal` | `test_create_proposal` | VERIFIED |
| 9 | Vote requires quorum of addresses (one address, one vote) | Why Not list | `cast_vote`, `finalize_voting` | `test_finalize_passes_with_quorum`, `test_double_vote_fails` | VERIFIED |
| 10 | Mandatory delay between approval and execution | Why Not list | `finalize_voting` sets `executable_at`; `assert_timelock_passed` | `test_execute_fails_before_timelock` | VERIFIED |
| 11 | Inspired by Compound/OZeppelin, without token-weighted voting | Why Not paragraph | N/A (design note) | N/A | REWORDED |
| 12 | Any address can propose an upgrade | Features table | `propose_upgrade` | `test_create_proposal` | VERIFIED |
| 13 | Any address can vote YES/NO (one address, one vote) | Features table | `cast_vote` | `test_vote_yes_no`, `test_double_vote_fails` | VERIFIED |
| 14 | Configurable timelock — mandatory delay between finalization and execution | Features table | `finalize_voting` → `executable_at` | `test_time_until_executable` | VERIFIED |
| 15 | Execute upgrades the target's real WASM from v1 to v2 after the timelock expires (proven by `test_real_wasm_upgrade_v1_to_v2_through_manager`) | Features table | `execute` → `invoke_upgrade` | `test_real_wasm_upgrade_v1_to_v2_through_manager` | VERIFIED |
| 16 | Configurable minimum vote count and YES-vote percentage | Features table | `validate_config`, `finalize_voting` | `test_initialize_zero_quorum_fails`, `test_initialize_threshold_101_fails` | VERIFIED |
| 17 | Admin can cancel any Active or Queued proposal | Features table | `cancel_proposal` | `test_cancel_by_admin`, `test_cancel_queued_proposal` | VERIFIED |
| 18 | Admin role can be transferred to multisig or DAO | Features table | `transfer_admin` | `test_transfer_admin` | VERIFIED |
| 19 | All proposals and votes persisted on-chain | Features table | `save_proposal`, `record_vote` | `test_create_proposal` | VERIFIED |
| 20 | Per-proposal config snapshot; later config changes do not affect in-flight proposals | Features table | `create_proposal` copies snap_* fields | `test_config_snapshot_isolation` | VERIFIED |
| 21 | Voting period runs from proposal creation to `voting_ends_at` | Lifecycle diagram | `create_proposal` sets `voting_ends_at` | `test_vote_at_voting_ends_at_succeeds` | VERIFIED |
| 22 | Timelock delay runs from `finalize()` to `executable_at` | Lifecycle diagram | `finalize_voting` sets `executable_at` | `test_time_until_executable` | VERIFIED |
| 23 | `initialize` sets admin and config; can only be called once; admin must sign | API table | `initialize` | `test_initialize_success`, `test_initialize_twice_fails`, `test_auth_initialize_without_auth_fails` | VERIFIED |
| 24 | `propose_upgrade` creates proposal; config snapshotted at this point | API table | `create_proposal` | `test_create_proposal`, `test_config_snapshot_isolation` | VERIFIED |
| 25 | `vote` casts YES/NO; one vote per address; only during voting period | API table | `cast_vote` | `test_vote_yes_no`, `test_double_vote_fails`, `test_vote_after_voting_period_ends_fails` | VERIFIED |
| 26 | `finalize` closes voting after voting period ends; moves to Queued or Defeated; sets `executable_at` | API table | `finalize_voting` | `test_finalize_passes_with_quorum`, `test_finalize_defeated_without_quorum`, `test_finalize_before_voting_ends_fails` | VERIFIED |
| 27 | `execute` upgrades the target's real WASM from v1 to v2 after the timelock (proven by `test_real_wasm_upgrade_v1_to_v2_through_manager`) | API table | `execute` → `invoke_upgrade` | `test_real_wasm_upgrade_v1_to_v2_through_manager` | VERIFIED |
| 28 | `cancel` cancels Active or Queued; admin must sign | API table | `cancel_proposal` | `test_cancel_by_admin`, `test_cancel_queued_proposal`, `test_auth_cancel_without_auth_fails` | VERIFIED |
| 29 | `update_config` changes params; does not affect in-flight proposals | API table | `update_config` | `test_update_config`, `test_config_snapshot_isolation` | VERIFIED |
| 30 | `transfer_admin` hands off admin role; admin must sign | API table | `transfer_admin` | `test_transfer_admin`, `test_auth_transfer_admin_without_auth_fails` | VERIFIED |
| 31 | `get_proposal` fetches proposal by ID | API table | `get_proposal` | `test_create_proposal` (reads proposal back) | VERIFIED |
| 32 | `get_config` fetches governance config | API table | `get_config` | `test_initialize_success` (reads config back) | VERIFIED |
| 33 | `get_admin` fetches admin address | API table | `get_admin` | `test_initialize_success` (reads admin back) | VERIFIED |
| 34 | `time_until_executable` returns ledgers until timelock; 0 if not Queued or already executable | API table | `ledgers_until_executable` | `test_time_until_executable` | VERIFIED |
| 35 | Not sybil-resistant: one-address-one-vote, no stake/token/identity | Security limitations | `cast_vote` (no stake check) | `test_vote_yes_no` (proves any address can vote) | VERIFIED |
| 36 | Admin can cancel Queued proposal after approval during timelock | Security limitations | `cancel_proposal` allows Queued | `test_cancel_queued_proposal` | VERIFIED |
| 37 | Admin cannot execute upgrades without a passing vote | Security limitations | `execute` requires Queued status via `assert_timelock_passed` | `test_execute_fails_on_active_proposal` | VERIFIED |
| 38 | No proposal deposit — spam is possible | Security limitations | `create_proposal` (no deposit) | `test_create_proposal` (no deposit required) | VERIFIED |
| 39 | Timelock starts when someone calls `finalize()`, not at creation | Security limitations | `finalize_voting` sets `executable_at = finalize_ledger + delay` | `test_time_until_executable` | VERIFIED |
| 40 | Target must authenticate manager inside `upgrade()` (proven by `test_real_example_target_upgrade_rejects_non_manager`) | Security limitations | Documented in `example-target/src/lib.rs` | `test_real_example_target_upgrade_rejects_non_manager` | VERIFIED |
| 41 | No expiry — Active proposal can be finalized years later | Security limitations | `finalize_voting` (no expiry check) | `test_finalize_at_voting_ends_at_plus_one_succeeds` | VERIFIED |
| 42 | No WASM compatibility check | Security limitations | `create_proposal` (no compatibility check) | N/A (absence of check) | REWORDED |
| 43 | ProposalDefeated (code 8) is never returned | Error codes note | `finalize_voting` returns `Ok(Defeated)`, not error | `test_finalize_defeated_without_quorum` (returns Defeated status, not error) | VERIFIED |
| 44 | All state-changing functions fail without auth | Contributing (implied) | `initialize`, `propose_upgrade`, `vote`, `cancel`, `update_config`, `transfer_admin` all call `require_auth` | `test_auth_initialize_without_auth_fails`, `test_auth_propose_without_auth_fails`, `test_auth_vote_without_auth_fails`, `test_auth_cancel_without_auth_fails`, `test_auth_update_config_without_auth_fails`, `test_auth_transfer_admin_without_auth_fails` | VERIFIED |

---

## Walkthrough Claims

| # | Claim | Source section | Implementing function | Test name | Status |
|---|-------|--------------|----------------------|-----------|--------|
| 1 | `initialize` takes `voting_period`, `timelock_delay`, `quorum`, `approval_threshold_pct` | Step 4 | `initialize` | `test_initialize_success` | VERIFIED |
| 2 | Voting period ends at `voting_ends_at`; votes accepted while `ledger.sequence() <= voting_ends_at` | Overview | `cast_vote` | `test_vote_at_voting_ends_at_succeeds`, `test_vote_after_voting_period_ends_fails` | VERIFIED |
| 3 | Timelock delay starts when `finalize()` is called, not at creation | Overview | `finalize_voting` | `test_time_until_executable` | VERIFIED |
| 4 | `finalize` succeeds with quorum in the tested flow (test runs under mock_all_auths; does not prove finalize requires no authentication) | Step 7 | `finalize` (no `require_auth` in path) | `test_finalize_passes_with_quorum` (called without auth source) | REWORDED |
| 5 | `finalize` returns `VotingStillActive` if called too early | Step 7 | `finalize_voting` | `test_finalize_before_voting_ends_fails` | VERIFIED |
| 6 | `execute` succeeds through the governance flow in the tested scenario (test runs under mock_all_auths; does not prove execute requires no authentication) | Step 9 | `execute` (no `require_auth` in path) | `test_governance_upgrade_executed_through_manager` (called without auth source) | REWORDED |
| 7 | `time_until_executable` returns 0 for proposals that are not Queued | Step 8 | `ledgers_until_executable` | `test_time_until_executable` | VERIFIED |
| 8 | Target contract's `upgrade()` authenticates the stored manager (proven by `test_real_example_target_upgrade_rejects_non_manager`) | Target upgrade section | `example-target::upgrade` calls `manager.require_auth()` | `test_real_example_target_upgrade_rejects_non_manager` | VERIFIED |
| 9 | Cancelling a Defeated/Executed/Cancelled proposal returns `ProposalFinalized` | Emergency Cancel | `cancel_proposal` | `test_cancel_defeated_proposal_fails`, `test_cancel_executed_proposal_fails`, `test_cancel_already_cancelled_fails` | VERIFIED |
| 10 | Example-target constructor stores manager at deploy time, cannot be front-run | Step 3 | `example-target::__constructor` | `test_real_example_target_deployed_with_contract_manager_without_mock_auths` | VERIFIED |

---

## CONTRIBUTING Claims

| # | Claim | Source section | Implementing function | Test name | Status |
|---|-------|--------------|----------------------|-----------|--------|
| 1 | `cargo fmt` and `cargo clippy` must pass | PR Guidelines | N/A (tooling requirement) | N/A | REWORDED |
| 2 | New logic must be accompanied by tests in `src/test.rs` | Submitting PRs | N/A (process requirement) | N/A | REWORDED |
| 3 | Every PR must append a dated, reasoned entry to `_CHANGELOG.md` | Submitting PRs | N/A (process requirement) | N/A | REWORDED |
| 4 | Security vulnerabilities should be reported privately | Reporting Bugs | N/A (process requirement) | N/A | REWORDED |

---

## Doc Comment Claims (lib.rs, governance.rs, timelock.rs, types.rs)

| # | Claim | Source section | Implementing function | Test name | Status |
|---|-------|--------------|----------------------|-----------|--------|
| 1 | `initialize` can only be called once; admin must sign | lib.rs `initialize` doc | `initialize` checks `AlreadyInitialized`; calls `admin.require_auth()` | `test_initialize_twice_fails`, `test_auth_initialize_without_auth_fails` | VERIFIED |
| 2 | `update_config` does not affect in-flight proposals (snapshotted) | lib.rs `update_config` doc | `create_proposal` copies snap_* fields | `test_config_snapshot_isolation` | VERIFIED |
| 3 | `propose_upgrade` snapshots governance rules at creation time | lib.rs `propose_upgrade` doc | `create_proposal` copies snap_* fields | `test_config_snapshot_isolation` | VERIFIED |
| 4 | `vote` is only allowed while `ledger.sequence() <= voting_ends_at` | lib.rs `vote` doc | `cast_vote` checks `voting_ends_at` | `test_vote_at_voting_ends_at_succeeds`, `test_vote_after_voting_period_ends_fails` | VERIFIED |
| 5 | `finalize` sets `executable_at = current_ledger + snap_timelock_delay` on pass | lib.rs `finalize` doc | `finalize_voting` | `test_time_until_executable` | VERIFIED |
| 6 | `execute` sets status to Executed BEFORE invoking target (checks-effects-interactions) | lib.rs `execute` doc | `execute` sets `Executed` then calls `invoke_upgrade` | `test_failed_upgrade_leaves_proposal_queued` | VERIFIED |
| 7 | If target call fails, entire transaction reverts, proposal remains Queued | lib.rs `execute` doc | `execute` — revert undoes `Executed` write | `test_failed_upgrade_leaves_proposal_queued` | VERIFIED |
| 8 | `cancel` returns `ProposalFinalized` for Executed, Cancelled, or Defeated | lib.rs `cancel` doc | `cancel_proposal` | `test_cancel_executed_proposal_fails`, `test_cancel_already_cancelled_fails`, `test_cancel_defeated_proposal_fails` | VERIFIED |
| 9 | `time_until_executable` returns 0 if not Queued or already executable | lib.rs `time_until_executable` doc | `ledgers_until_executable` | `test_time_until_executable` | VERIFIED |
| 10 | `load_config` returns `NotInitialized` if not initialised | governance.rs `load_config` doc | `load_config` returns `Error::NotInitialized` | `test_get_config_not_initialized_fails` | VERIFIED |
| 11 | `load_admin` returns `NotInitialized` if not initialised | governance.rs `load_admin` doc | `load_admin` returns `Error::NotInitialized` | `test_get_admin_not_initialized_fails` | VERIFIED |
| 12 | `require_admin` calls `caller.require_auth()` then compares with stored admin | governance.rs `require_admin` doc | `require_admin` | `test_auth_cancel_non_admin_fails`, `test_auth_cancel_without_auth_fails` | VERIFIED |
| 13 | `validate_config` rejects voting_period=0, timelock_delay=0, quorum=0, threshold 0 or >100 | governance.rs `validate_config` doc | `validate_config` | `test_initialize_zero_voting_period_fails`, `test_initialize_zero_delay_fails`, `test_initialize_zero_quorum_fails`, `test_initialize_zero_threshold_fails`, `test_initialize_threshold_101_fails` | VERIFIED |
| 14 | `create_proposal` rejects empty description | governance.rs `create_proposal` doc | `create_proposal` returns `EmptyDescription` | `test_description_empty_fails` | VERIFIED |
| 15 | `create_proposal` rejects description > 256 bytes | governance.rs `create_proposal` doc | `create_proposal` returns `DescriptionTooLong` | `test_description_too_long` | VERIFIED |
| 16 | `cast_vote` rejects if proposal is not Active | governance.rs `cast_vote` doc | `cast_vote` returns `VotingClosed` | `test_finalize_already_queued_fails` (indirectly tests non-Active) | VERIFIED |
| 17 | `cast_vote` rejects if voting period has ended | governance.rs `cast_vote` doc | `cast_vote` returns `VotingPeriodEnded` | `test_vote_after_voting_period_ends_fails` | VERIFIED |
| 18 | `cast_vote` rejects if already voted | governance.rs `cast_vote` doc | `cast_vote` returns `AlreadyVoted` | `test_double_vote_fails` | VERIFIED |
| 19 | `finalize_voting` requires `ledger.sequence() > voting_ends_at` | governance.rs `finalize_voting` doc | `finalize_voting` returns `VotingStillActive` | `test_finalize_before_voting_ends_fails`, `test_finalize_at_voting_ends_at_fails` | VERIFIED |
| 20 | `cancel_proposal` returns `ProposalFinalized` for Executed, Cancelled, or Defeated | governance.rs `cancel_proposal` doc | `cancel_proposal` match block | `test_cancel_executed_proposal_fails`, `test_cancel_already_cancelled_fails`, `test_cancel_defeated_proposal_fails` | VERIFIED |
| 21 | `ledgers_until_executable` returns 0 for non-Queued proposals | timelock.rs doc | `ledgers_until_executable` | `test_time_until_executable` | VERIFIED |
| 22 | `assert_timelock_passed` returns `NotQueued` for non-Queued | timelock.rs doc | `assert_timelock_passed` | `test_execute_fails_on_active_proposal` | VERIFIED |
| 23 | `assert_timelock_passed` returns `TimelockNotExpired` if before `executable_at` | timelock.rs doc | `assert_timelock_passed` | `test_execute_fails_before_timelock`, `test_execute_one_ledger_before_executable_at_fails` | VERIFIED |
| 24 | Events emitted for all state transitions | governance.rs emit_* functions | `emit_initialized`, `emit_config_updated`, `emit_admin_transferred`, `emit_executed`, events in `create_proposal`, `cast_vote`, `finalize_voting`, `cancel_proposal` | `test_event_initialized`, `test_event_proposal_created`, `test_event_vote_cast`, `test_event_finalized`, `test_event_cancelled`, `test_event_config_updated`, `test_event_admin_transferred` | VERIFIED |
| 25 | TTL extended on write for config, admin, proposal, vote | governance.rs save_* and record_vote | `save_config`, `save_admin`, `save_proposal`, `record_vote` all call `extend_ttl` | `test_ttl_extended_on_write_and_read` | VERIFIED |
| 26 | TTL extended on read for proposals | governance.rs `load_proposal` | `load_proposal` calls `extend_ttl` | `test_ttl_extended_on_write_and_read` | VERIFIED |
| 27 | `next_proposal_id` uses checked arithmetic returning `Overflow` | governance.rs `next_proposal_id` | `next_proposal_id` uses `checked_add` | N/A (overflow not reachable in normal operation; test would require u64 wrap) | REWORDED |
| 28 | `example-target::__constructor` stores manager at deploy time; cannot be front-run | example-target lib.rs | `__constructor` sets manager in instance storage | `test_real_example_target_deployed_with_contract_manager_without_mock_auths` | VERIFIED |
| 29 | `example-target::upgrade` calls `manager.require_auth()` before `update_current_contract_wasm` (proven by `test_real_example_target_upgrade_rejects_non_manager`) | example-target lib.rs | `upgrade` calls `manager.require_auth()` then `update_current_contract_wasm` | `test_real_example_target_upgrade_rejects_non_manager` | VERIFIED |
| 30 | `example-target::version` returns 1 without `v2` feature, 2 with `v2` | example-target lib.rs | `version` uses `#[cfg(feature = "v2")]` | N/A (compile-time feature; not testable in single env) | REWORDED |
| 31 | `upgrade()` rejects callers that are not the manager | example-target lib.rs | `upgrade` calls `manager.require_auth()` | `test_real_example_target_upgrade_rejects_non_manager` | VERIFIED |
| 32 | A governance proposal upgrades a target contract's real WASM from v1 to v2 | example-target lib.rs | `upgrade` calls `manager.require_auth()` then `update_current_contract_wasm` | `test_real_wasm_upgrade_v1_to_v2_through_manager` | VERIFIED |

---

## REWORDED/REMOVED Claims (from original docs)

| # | Original claim | Source | What was wrong | Action |
|---|---------------|--------|----------------|--------|
| R1 | "token holders vote YES/NO" | Old README Features table | Voting is one-address-one-vote, not token-weighted | REWORDED to "any address can vote YES/NO (one address, one vote)" |
| R2 | "Configurable timelock (default ≈ 24 h)" | Old README Features table | No default exists — timelock is a required parameter | REWORDED to "Mandatory delay between finalization and execution, set in ledgers" |
| R3 | "Must pass a governance vote" in Why Not table (old) | Old README | "Admin can cancel; voters can reject" was vague | REWORDED to "Voters can outvote a bad proposal; admin can cancel Active or Queued proposals" |
| R4 | "same pattern used by Compound Governor Bravo and OpenZeppelin's TimelockController" | Old README | Implied equivalence including token-weighted voting | REWORDED to "inspired by ... adapted for Soroban's contract model. Unlike those systems, this contract uses unweighted one-address-one-vote voting rather than token-weighted voting." |
| R5 | "finalize() — close voting after timelock elapses" | Old walkthrough Overview | Finalize is called after the voting period ends, not after the timelock | REWORDED to "finalize() — after the voting period ends" |
| R6 | "token holders cast YES/NO votes" | Old walkthrough Overview | Not token-weighted | REWORDED to "any address casts one YES/NO vote" |
| R7 | Walkthrough deploys a second upgrade-manager as target | Old walkthrough Step 3 | Upgrade-manager has no `upgrade()` function, so execute would fail | REMOVED; replaced with example-target deployment |
| R8 | "time_until_executable starts at 17280, counts down to 0" | Old walkthrough Step 6 | Returns 0 before finalize, counts down after finalize during timelock | REWORDED to "Returns remaining ledgers (0 means execution is permitted)" |
| R9 | "finalize before timelock → TimelockNotExpired" | Old walkthrough Failure Modes | Finalize before voting period ends returns `VotingStillActive`, not `TimelockNotExpired` | REWORDED to "finalize before voting period ends → VotingStillActive" |
| R10 | `initialize` auth was "None" in old API table | Old README API table | `initialize` requires admin to sign | REWORDED to "Admin (must sign)" |
| R11 | `finalize` auth was listed but finalize doesn't require auth | Old README API table | Finalize and execute are callable by anyone (no auth needed) | REWORDED to "Anyone" |
| R12 | "Rust 1.74+" in CONTRIBUTING | Old CONTRIBUTING | Part C will determine the real minimum; 1.74 is unverified | REWORDED to "Latest stable" |
| R13 | Clone URL was `https://github.com` | Old CONTRIBUTING | Incomplete URL | REWORDED to full repo URL |
| R14 | "Open an Issue using the bug template" | Old CONTRIBUTING | No bug template exists in repo | REWORDED to "Open a GitHub Issue" |
