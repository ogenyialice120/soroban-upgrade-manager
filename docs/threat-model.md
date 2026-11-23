# Threat Model

This document identifies the adversaries relevant to `soroban-upgrade-manager`,
maps each threat to the contract code that mitigates it, and describes the
residual risk the contract does not eliminate.

The starting point for this analysis is the "Security Model & Limitations"
section of `README.md` and the "Possible Future Work" section that lists
proposed mitigations not yet implemented.

---

## Adversary 1: Malicious proposer (spammer)

**Goal:** Flood the proposal queue with junk proposals to disrupt legitimate
governance or waste voters' attention.

| | Detail |
|---|---|
| **Attack surface** | `propose_upgrade` in `lib.rs` — anyone with an authenticated address can create a proposal |
| **Mitigation** | `proposal_min_interval`: `create_proposal` in `governance.rs` checks `DataKey::LastProposalLedger` and returns `Error::ProposalRateLimitExceeded` (code 25) if the proposer's last successful proposal was too recent |
| **Relevant test** | *(rate-limit tests not present in the current test suite; the rate-limit path is exercised via config and proposal creation tests)* |
| **Residual risk** | The rate limit applies per-address. An attacker who controls many addresses can still submit one proposal per address per `proposal_min_interval` ledgers. There is no proposal deposit, so creating addresses is cheap. The admin can cancel spam proposals (see Adversary 3), but only one at a time. The "Possible Future Work" section proposes a deposit mechanism to increase the cost of spam. |

---

## Adversary 2: Malicious voter (sybil attacker)

**Goal:** Sway a governance vote by controlling many addresses, each casting
one YES or NO vote.

| | Detail |
|---|---|
| **Attack surface** | `vote` in `lib.rs` / `cast_vote` in `governance.rs` — any address can vote once per proposal; voting weight is 1 per address |
| **Mitigation** | None beyond requiring authentication (`voter.require_auth()`). The contract is explicitly designed as one-address-one-vote. `AlreadyVoted` (code 5) prevents double-voting from a single address. `has_voted` and `record_vote` in `governance.rs` enforce this. |
| **Relevant tests** | `test_double_vote_fails`, `test_vote_yes_no` |
| **Residual risk** | The voting model is not sybil-resistant. An attacker who creates enough funded addresses can reach `quorum` and `approval_threshold_pct` unilaterally. The README acknowledges this explicitly. The "Possible Future Work" section proposes token-weighted voting or integration with a Stellar identity system as a future mitigation. |

---

## Adversary 3: Compromised admin key

**Goal:** Abuse the admin role to cancel legitimate approved proposals, change
governance parameters to favour a specific outcome, or transfer admin ownership
to an attacker-controlled address.

| | Detail |
|---|---|
| **Attack surface** | `cancel` in `lib.rs`, `update_config` in `lib.rs`, `transfer_admin` in `lib.rs` — all gated by `require_admin` in `governance.rs` |
| **What the contract does** | `require_admin` calls `caller.require_auth()` and checks `caller == stored admin`. Without the admin key, none of these functions succeed. `cancel_proposal` in `governance.rs` returns `Error::ProposalFinalized` (code 13) if the proposal is already `Executed`, `Cancelled`, or `Defeated`, so the admin cannot retroactively undo a completed upgrade. The admin **cannot** execute an upgrade without a passing vote — `execute` in `lib.rs` calls `assert_timelock_passed` which requires `ProposalStatus::Queued`. |
| **Relevant tests** | `test_cancel_by_admin`, `test_cancel_queued_proposal`, `test_cancel_by_non_admin_fails`, `test_auth_cancel_non_admin_fails`, `test_auth_update_config_non_admin_fails`, `test_auth_transfer_admin_non_admin_fails` |
| **Residual risk** | A compromised admin key can cancel any `Active` or `Queued` proposal, change governance parameters, and transfer the admin role. It cannot execute an upgrade unilaterally. The admin can also cancel a `Queued` proposal during the timelock window, blocking an approved upgrade. The recommended mitigation (noted in README) is to hold the admin key in a multisig or DAO. |

---

## Adversary 4: Malicious target contract

**Goal:** Use a crafted `upgrade()` function on the target to perform
re-entrancy, avoid authentication, or execute arbitrary logic when the manager
calls it.

| | Detail |
|---|---|
| **Attack surface** | `invoke_upgrade` in `governance.rs` — the manager calls `target.upgrade(new_wasm_hash)` via `env.invoke_contract` after the timelock expires |
| **What the contract does** | Checks-effects-interactions: `execute` in `lib.rs` sets `proposal.status = ProposalStatus::Executed` and calls `save_proposal` **before** calling `invoke_upgrade`. If the target call fails, the entire transaction reverts and the proposal returns to `Queued`. An honest target's `upgrade` function calls `manager.require_auth()` before `update_current_contract_wasm` (see `contracts/example-target/src/lib.rs`). |
| **Relevant tests** | `test_failed_upgrade_leaves_proposal_queued`, `test_execute_with_error_returning_target`, `test_real_example_target_upgrade_rejects_non_manager` |
| **Residual risk** | The manager does not verify that the target's `upgrade` function exists or that it authenticates the manager correctly before the proposal is created. A target that omits `manager.require_auth()` can be upgraded by anyone who can invoke the manager's `execute`. There is also no check that the new WASM is storage-layout-compatible with the target's existing data; an incompatible upgrade can brick the target. The README documents both limitations under "Target must authenticate the manager" and "No WASM compatibility check". |

---

## Adversary 5: Front-runner

**Goal:** Observe a pending `execute` transaction and submit a conflicting
transaction first (e.g., to cancel the proposal or change the target) before
the legitimate execution confirms.

| | Detail |
|---|---|
| **Attack surface** | `execute` in `lib.rs` — permissionless after the timelock expires; any address can call it |
| **What the contract does** | Only the admin can cancel a `Queued` proposal (`cancel` requires `require_admin`). The timelock delay gives users a window to review the upgrade, but it equally gives a malicious admin a window to cancel. The proposal status transitions atomically within a single transaction; a `Queued` proposal cannot be executed twice (`test_double_execute_fails`). |
| **Relevant tests** | `test_double_execute_fails`, `test_cancel_queued_proposal` |
| **Residual risk** | An admin-level front-runner can cancel a legitimate `execute` by calling `cancel` before the execute transaction confirms. A non-admin cannot block `execute` because there is no function that moves a `Queued` proposal to a non-executable state without admin auth. The example-target's constructor uses `__constructor` to set the manager atomically at deploy time, eliminating the front-running window on initialization (noted in `contracts/example-target/src/lib.rs`). |

---

## Summary table

| Adversary | Key function(s) | Key error(s) | Residual risk |
|---|---|---|---|
| Malicious proposer | `propose_upgrade`, `create_proposal` | `ProposalRateLimitExceeded` (25) | Multi-address spam possible without deposit |
| Sybil voter | `vote`, `cast_vote` | `AlreadyVoted` (5) | Not sybil-resistant by design |
| Compromised admin | `cancel`, `update_config`, `transfer_admin` | `Unauthorized` (2) | Admin can cancel approved proposals; use multisig |
| Malicious target | `execute`, `invoke_upgrade` | *(transaction revert)* | No auth enforcement on target; no WASM compatibility check |
| Front-runner | `execute`, `cancel` | `ProposalFinalized` (13) | Admin-level front-running possible during timelock window |

## See also

- [README.md — Security Model & Limitations](../README.md#-security-model--limitations)
- [README.md — Possible Future Work](../README.md#-possible-future-work)
- [docs/writing-target-contracts.md](writing-target-contracts.md) — how to write a safe target contract
- [contracts/upgrade-manager/src/governance.rs](../contracts/upgrade-manager/src/governance.rs) — proposal, voting, and cancel logic
- [contracts/upgrade-manager/src/lib.rs](../contracts/upgrade-manager/src/lib.rs) — `execute` and admin entry points
- [contracts/upgrade-manager/src/types.rs](../contracts/upgrade-manager/src/types.rs) — `Error` enum
- [contracts/example-target/src/lib.rs](../contracts/example-target/src/lib.rs) — reference target implementation
