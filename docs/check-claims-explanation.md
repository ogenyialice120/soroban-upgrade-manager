# What `scripts/check_claims.sh` Does and Does Not Prove

This document explains how [`scripts/check_claims.sh`](../scripts/check_claims.sh)
validates [`docs/claims-audit.md`](claims-audit.md) in CI, and—from a security
perspective—what a passing check **does** and **does not** prove about the
`soroban-upgrade-manager` contract.

---

## 1. What `scripts/check_claims.sh` Checks

`scripts/check_claims.sh` is a static shell-based traceability check that parses the
6-column markdown tables in `docs/claims-audit.md` and cross-references them against
`contracts/upgrade-manager/src/test.rs`.

Specifically, it verifies three structural invariants:

1. **Valid status labels:** Every row in a 6-column table has a status equal to
   `VERIFIED`, `REWORDED`, or `REMOVED`.
2. **Non-empty test references for `VERIFIED` claims:** Every row marked `VERIFIED`
   contains at least one backtick-quoted test function name matching `` `test_\w+` ``
   (and is not `N/A`).
3. **Function name existence in `test.rs`:** Every backtick-quoted `test_*` name
   referenced by a `VERIFIED` row exists as a Rust function signature matching
   `fn <test_name>\b` inside `contracts/upgrade-manager/src/test.rs`.

---

## 2. Security Perspective: What `check_claims.sh` Does NOT Prove

**A passing `bash scripts/check_claims.sh` run does NOT prove that the contract is
secure, nor does it prove that the referenced tests actually verify the claims.**

Specifically, `scripts/check_claims.sh` has the following security limitations:

1. **It does not execute the test suite:**
   `check_claims.sh` only runs `grep -qE "fn ${tname}\b"` on `src/test.rs`. It does
   not invoke `cargo test`. A test function that is `#[ignore]`d, commented out around
   its body, or failing would still pass `check_claims.sh` as long as `fn test_...`
   appears in `src/test.rs`.
2. **It does not inspect test assertions or semantics:**
   An empty function `fn test_auth_cancel_without_auth_fails() {}`—or a test that
   calls `env.mock_all_auths()` when claiming to test unauthenticated rejection—would
   satisfy `check_claims.sh`. The script verifies **name existence**, not behavioral
   equivalence between the markdown claim and the Rust assertions.
3. **It does not detect missing security claims:**
   If a security property or edge case is omitted from `docs/claims-audit.md`
   entirely, `check_claims.sh` will still exit `0`.
4. **It does not verify contract implementation code (`lib.rs`, `governance.rs`, `timelock.rs`):**
   The `"Implementing function"` column in `docs/claims-audit.md` is informational and
   is not parsed or AST-checked against the contract source files.

---

## 3. Security-Critical Claims Requiring Manual Assertion Review

When auditing the contract or reviewing pull requests that touch `contracts/upgrade-manager/src/`,
reviewers must manually inspect both the contract implementation and the exact
assertions in `contracts/upgrade-manager/src/test.rs` for the following
security-critical claim categories in [`docs/claims-audit.md`](claims-audit.md):

### A. Authentication & Access Control (`require_auth` & `require_admin`)

Verify that tests for these claims **do not** use `env.mock_all_auths()` (or that they
explicitly test unauthorized/unauthenticated failure paths):

- **README Claim #23 / Doc Claim #1:** `initialize` can only be called once and requires `admin.require_auth()` (`test_initialize_twice_fails`, `test_auth_initialize_without_auth_fails`).
- **README Claim #28 & #30 / Doc Claim #12:** Admin-only functions (`cancel`, `update_config`, `transfer_admin`) call `caller.require_auth()` and verify `caller == load_admin(&env)` (`test_auth_cancel_non_admin_fails`, `test_auth_cancel_without_auth_fails`, `test_auth_update_config_without_auth_fails`, `test_auth_transfer_admin_without_auth_fails`).
- **README Claim #44:** All state-changing entrypoints requiring signatures (`initialize`, `propose_upgrade`, `vote`, `cancel`, `update_config`, `transfer_admin`) fail when invoked without valid auth (`test_auth_propose_without_auth_fails`, `test_auth_vote_without_auth_fails`).
- **README Claim #40 / Walkthrough Claim #8 & #10 / Doc Claim #28, #29, #31:** Target contract sets `manager` in `__constructor` at deploy time and calls `manager.require_auth()` inside `upgrade()` before `update_current_contract_wasm` (`test_real_example_target_deployed_with_contract_manager_without_mock_auths`, `test_real_example_target_upgrade_rejects_non_manager`).

### B. Governance Gate, Timelock Enforcement & Config Snapshot Isolation

Verify that tests check exact ledger sequence boundaries (`voting_ends_at`, `executable_at`)
and status transitions:

- **README Claim #2, #9, #13, #25 / Doc Claim #16–#19:** One-vote-per-address enforcement (`AlreadyVoted`), voting window bounds (`ledger.sequence() <= voting_ends_at`), and post-window finalization (`ledger.sequence() > voting_ends_at`) (`test_double_vote_fails`, `test_vote_at_voting_ends_at_succeeds`, `test_vote_after_voting_period_ends_fails`, `test_finalize_before_voting_ends_fails`, `test_finalize_at_voting_ends_at_fails`).
- **README Claim #3, #10, #37 / Doc Claim #22, #23:** Neither the admin nor any caller can execute an upgrade unless `proposal.status == ProposalStatus::Queued` and `ledger.sequence() >= proposal.executable_at` (`test_execute_fails_on_active_proposal`, `test_execute_fails_before_timelock`, `test_execute_one_ledger_before_executable_at_fails`).
- **README Claim #20, #24, #29 / Doc Claim #2, #3:** Per-proposal governance parameter snapshots (`snap_voting_period`, `snap_timelock_delay`, `snap_quorum`, `snap_threshold_pct`) are isolated from subsequent `update_config` calls (`test_config_snapshot_isolation`).

### C. Checks-Effects-Interactions & Cross-Contract Upgrade Safety

Verify that state updates occur before external contract invocation and that reverts
preserve invariants:

- **Doc Claim #6 & #7:** `execute()` sets `proposal.status = ProposalStatus::Executed` and persists it **before** invoking `invoke_upgrade(&env, &proposal.target, &proposal.new_wasm_hash)`, and if the target contract panics/reverts, the transaction rollbacks leave the proposal `Queued` (`test_failed_upgrade_leaves_proposal_queued`).
- **README Claim #7, #15, #27 / Doc Claim #32:** End-to-end cross-contract WASM replacement from v1 to v2 through the manager (`test_real_wasm_upgrade_v1_to_v2_through_manager`).

### D. Integer Overflow & Input Validation

Verify that arithmetic operations use checked math (`checked_add`) and boundary
inputs are rejected:

- **Doc Claim #13, #14, #15:** `validate_config` rejects zero values for `voting_period`, `timelock_delay`, `quorum`, and `approval_threshold_pct` (`0` or `> 100`), and `create_proposal` enforces `1..=256` byte descriptions (`test_initialize_zero_voting_period_fails`, `test_initialize_zero_delay_fails`, `test_initialize_zero_quorum_fails`, `test_initialize_zero_threshold_fails`, `test_initialize_threshold_101_fails`, `test_description_empty_fails`, `test_description_too_long`).
- **Doc Claim #27 (marked `REWORDED`):** `next_proposal_id`, `voting_ends_at`, `executable_at`, `yes_votes`, `no_votes`, and `total_votes` use `checked_add(...).ok_or(Error::Overflow)?` and `u64` widening in quorum threshold math (`(proposal.yes_votes as u64 * 100) >= (total_votes as u64 * proposal.snap_threshold_pct as u64)`). Because `REWORDED` rows are not checked for test functions by `check_claims.sh`, reviewers must manually verify checked arithmetic in `contracts/upgrade-manager/src/governance.rs`.

### E. Storage TTL Extension

Verify that both instance and persistent storage entries have their TTL extended so
governance state cannot silently expire prematurely:

- **Doc Claim #25 & #26:** `save_config`, `save_admin`, `save_proposal`, `record_vote`, and `load_proposal` extend instance/persistent storage TTLs (`INSTANCE_TTL_BUMP`, `PERSISTENT_TTL_BUMP`) (`test_ttl_extended_on_write_and_read`).

---

## 4. Recommended Review Checklist for Contributors

Whenever modifying contract code, tests, or `docs/claims-audit.md`:

1. Run `bash scripts/check_claims.sh` to verify structural table-to-test links.
2. Run `cargo test --workspace` to ensure all referenced tests actually execute and pass.
3. For every `VERIFIED` security claim touched by a change, open `contracts/upgrade-manager/src/test.rs` at the referenced `fn test_*` and manually confirm that:
   - The test asserts the exact error variant (`Error::...`) or state post-condition stated in the claim.
   - Auth-rejection tests do not bypass the check with `mock_all_auths()`.
