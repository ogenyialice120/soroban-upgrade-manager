# Design: Optional Proposal Deposit Mechanism

**Issue:** #105  
**Branch:** `design/proposal-deposit-105`  
**Status:** Design proposal — awaiting maintainer approval before code is written.

---

## Problem

The contract currently imposes no cost on proposal creation. Any eligible address can create proposals without staking anything. The README "Security Model & Limitations" section explicitly lists "No proposal deposit" as a known limitation.

This is a denial-of-service and spam vector: a voter with enough weight to meet the proposer eligibility threshold can flood the contract with low-quality or malicious proposals. While `proposal_min_interval` rate-limits proposals *per address*, it does not create any economic disincentive. Deployers running governance over high-value contracts may want to require proposers to put something at stake.

---

## Proposed Design

### New `Config` fields

Add two fields to `Config` in `types.rs`:

```rust
pub struct Config {
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
    pub proposal_min_interval: u32,
    /// Optional SEP-41 token address for proposal deposits.
    /// None = no deposit required (backward-compatible default).
    pub deposit_token: Option<Address>,
    /// Amount of `deposit_token` locked when a proposal is created.
    /// Ignored when `deposit_token` is None. 0 effectively disables the deposit
    /// without removing the token address. Must be >= 0.
    pub deposit_amount: i128,
}
```

Both fields default to disabled: existing deployments that do not populate `deposit_token` or `deposit_amount` behave identically to today.

### New `DataKey` variant

Add a per-proposal deposit record to `DataKey` in `types.rs`:

```rust
pub enum DataKey {
    Admin,
    Config,
    Proposal(u64),
    ProposalCount,
    Vote(u64, Address),
    Delegation(Address),
    DelegationCount(Address),
    LastProposalLedger(Address),
    /// Deposit held on behalf of a proposal: proposal_id → i128.
    /// Written on creation; removed when the deposit is transferred out.
    Deposit(u64),
}
```

Stored in persistent storage alongside the proposal itself, so its TTL is bumped whenever the proposal entry is bumped.

### New error code

Add to the error enum in `types.rs`:

```rust
/// deposit_amount is negative, which is not permitted.
InvalidDepositAmount = 30,
```

(Exact numeric assignment is TBD if codes 26–29 are reserved by other in-flight designs; the name is stable.)

### Config validation

In `validate_config` (or wherever config fields are range-checked), add:

```rust
if config.deposit_token.is_some() && config.deposit_amount < 0 {
    return Err(ContractError::InvalidDepositAmount);
}
```

A negative amount is never meaningful and indicates a configuration mistake. `deposit_amount == 0` is explicitly allowed as a way to soft-disable the deposit without clearing the token address.

### `create_proposal` — taking the deposit

After all existing validations pass and the new proposal struct is written to storage, transfer the deposit before returning the proposal ID:

```rust
// --- deposit logic ---
let cfg = get_config(&env);
if let Some(token_addr) = cfg.deposit_token {
    if cfg.deposit_amount > 0 {
        token::Client::new(&env, &token_addr).transfer(
            &proposer,                        // from: the proposer
            &env.current_contract_address(),  // to: this contract
            &cfg.deposit_amount,
        );
        env.storage().persistent().set(
            &DataKey::Deposit(proposal_id),
            &cfg.deposit_amount,
        );
    }
}
```

The `transfer` call uses the standard SEP-41 interface. If the proposer has insufficient balance or has not approved the contract as a spender (for tokens that require allowances), the token contract will trap and the entire `create_proposal` transaction reverts. No proposal is recorded and no deposit is lost.

The deposit amount stored under `DataKey::Deposit(id)` is the amount actually transferred at creation time. This decouples the stored deposit from any subsequent `update_config` changes.

### `finalize_voting` — forfeiture on defeat

When voting closes and the proposal transitions to `ProposalStatus::Defeated`, transfer the deposit to the admin. When the proposal is queued, the deposit stays locked:

```rust
fn release_deposit_on_finalize(env: &Env, proposal_id: u64, status: &ProposalStatus) {
    let key = DataKey::Deposit(proposal_id);
    let amount: Option<i128> = env.storage().persistent().get(&key);
    let amount = match amount {
        Some(a) if a > 0 => a,
        _ => return, // no deposit was taken
    };

    let cfg = get_config(env);
    let token_addr = match cfg.deposit_token {
        Some(a) => a,
        None => return, // token was removed from config after creation; skip silently
    };
    let token = token::Client::new(env, &token_addr);

    match status {
        ProposalStatus::Defeated => {
            // Deposit is forfeit. Send to admin.
            let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
            token.transfer(&env.current_contract_address(), &admin, &amount);
            env.storage().persistent().remove(&key);
        }
        ProposalStatus::Queued => {
            // Deposit is still locked. Do nothing.
        }
        _ => {} // unreachable from finalize_voting, but exhaustive
    }
}
```

Call `release_deposit_on_finalize` at the end of `finalize_voting`, after the proposal status is written.

### `execute` — return deposit on success

After a successful upgrade execution:

```rust
fn return_deposit_to_proposer(env: &Env, proposal: &Proposal) {
    let key = DataKey::Deposit(proposal.id);
    let amount: Option<i128> = env.storage().persistent().get(&key);
    let amount = match amount {
        Some(a) if a > 0 => a,
        _ => return,
    };

    let cfg = get_config(env);
    let token_addr = match cfg.deposit_token {
        Some(a) => a,
        None => return,
    };
    token::Client::new(env, &token_addr).transfer(
        &env.current_contract_address(),
        &proposal.proposer,
        &amount,
    );
    env.storage().persistent().remove(&key);
}
```

Call at the end of `execute()`, after the WASM upgrade succeeds.

### `cancel_proposal` — return deposit on admin cancellation

Admin cancellation is not the proposer's fault, so the deposit is returned to the proposer regardless of whether the proposal was `Active` or `Queued`:

```rust
// Inside cancel_proposal, after status is set to Cancelled:
return_deposit_to_proposer(&env, &proposal);
```

`return_deposit_to_proposer` is the same helper used by `execute`. Reusing it keeps the return path in one place.

### Deposit lifecycle summary

| Event | Proposal status after | Deposit outcome |
|---|---|---|
| `create_proposal` succeeds | `Active` | Locked in contract; `DataKey::Deposit(id)` written |
| `finalize_voting` → quorum not met or threshold not met | `Defeated` | Transferred to admin; key removed |
| `finalize_voting` → passes | `Queued` | Held; no change |
| `execute` succeeds | `Executed` | Returned to proposer; key removed |
| `cancel_proposal` (admin) | `Cancelled` | Returned to proposer; key removed |
| `deposit_token = None` or `deposit_amount == 0` | any | No deposit taken; key never written |

### Snapshot isolation

`deposit_token` and `deposit_amount` are **not** snapshotted into the `Proposal` struct. The deposit is taken at creation time against the current config and stored independently under `DataKey::Deposit(id)`. If `update_config` changes `deposit_amount` or replaces `deposit_token` while proposals are in flight:

- In-flight deposits are unaffected — they were already locked and the amount is recorded in `DataKey::Deposit`.
- The return/forfeit path reads `deposit_token` from the current config to construct the token client. If the token address is changed between creation and resolution, the helper returns silently without transferring. This is a known edge case. If the deployer needs to change the token address, they should ensure no proposals with deposits are outstanding, or accept that those deposits cannot be automatically resolved. The deployer is responsible for this operational constraint. A future design could snapshot `deposit_token` per proposal if this proves problematic in practice.

---

## Security implications

The contract now holds token balances. This is a new attack surface that is worth naming explicitly.

**Malicious token contract.** If `deposit_token` is set to an adversarial address, its `transfer` function could re-enter the upgrade-manager or cause unexpected side-effects. Mitigation: the deployer is responsible for choosing a trustworthy token. The contract cannot validate that the token is well-behaved. This is the same trust assumption that applies to any SEP-41 token integration on Soroban.

**Admin draining deposits.** An admin who controls `finalize_voting` timing cannot steal deposits. `finalize_voting` is callable by anyone once `voting_ends_at` has passed, so the admin has no exclusive control over when it runs. Defeated-proposal deposits go to the admin, but the admin cannot manufacture a defeat — the vote outcome is determined by voters. Admin-cancelled proposals return deposits to proposers, so the admin cannot cancel proposals to steal deposits.

**Bounded exposure.** With `proposal_min_interval` limiting each address to one proposal per interval, the number of simultaneously active proposals (and therefore the total token balance held by the contract) is bounded by `total_eligible_voters * 1`. Deployers who set a high `deposit_amount` effectively bound the maximum locked balance.

**Token address change after creation.** Covered above under snapshot isolation. The edge case is operational rather than a theft vector: a misconfigured token change would cause deposits to be silently skipped rather than misdirected.

**Zero-value deposit key.** `DataKey::Deposit` is only written when `deposit_amount > 0`. Helpers check for a positive stored amount before attempting a transfer, so a misconfigured zero-amount entry (which should not occur) would be a no-op.

---

## Existing guarantee touched

> **"Proposals are free to create."**

This guarantee changes to:

> **"Proposals are free to create when `deposit_token` is `None` or `deposit_amount` is `0`. When a deposit is configured, `create_proposal` requires the proposer to transfer `deposit_amount` tokens to the contract. The deposit is returned on execution or admin cancellation, and forfeited to the admin on defeat."**

No other existing guarantees are affected. Deposits are transferred atomically within the same transaction as proposal creation, so there is no partial-state risk.

---

## Tests needed

| Test name | What it verifies |
|---|---|
| `test_deposit_required_on_create` | With deposit configured, `create_proposal` transfers `deposit_amount` from proposer to contract; contract token balance increases by `deposit_amount` |
| `test_deposit_key_stored_on_create` | `DataKey::Deposit(id)` exists in persistent storage after a successful `create_proposal` with a configured deposit |
| `test_deposit_returned_on_execute` | After a proposal executes successfully, the proposer's token balance is restored and `DataKey::Deposit(id)` is removed |
| `test_deposit_forfeited_on_defeat` | After `finalize_voting` marks a proposal `Defeated`, the admin's token balance increases by `deposit_amount` and `DataKey::Deposit(id)` is removed |
| `test_deposit_returned_on_admin_cancel_active` | Admin cancels an `Active` proposal; deposit is returned to proposer, not to admin |
| `test_deposit_returned_on_admin_cancel_queued` | Admin cancels a `Queued` proposal; deposit is returned to proposer |
| `test_deposit_not_required_when_disabled` | `deposit_token = None`; `create_proposal` succeeds with the proposer holding zero token balance — no transfer attempted |
| `test_deposit_zero_amount_not_required` | `deposit_token = Some(addr)`, `deposit_amount = 0`; `create_proposal` succeeds without transferring tokens; `DataKey::Deposit(id)` is not written |
| `test_deposit_negative_amount_config_fails` | `deposit_amount = -1` causes `initialize` (or `update_config`) to return `InvalidDepositAmount` |
| `test_deposit_insufficient_balance_fails` | Proposer holds no tokens; `create_proposal` panics or returns a token-error (the SEP-41 `transfer` call traps); no proposal is stored |
| `test_deposit_update_config_does_not_affect_inflight` | `update_config` raises `deposit_amount`; existing in-flight proposal deposit is unaffected; new proposals use the new amount |

---

## Out of scope

- **Partial refunds.** Returning a fraction of the deposit based on how close a defeated proposal came to passing. Adds significant complexity for unclear benefit.
- **Community treasury destination.** Routing forfeited deposits somewhere other than the admin. Requires introducing a treasury address concept that does not exist in the current contract.
- **Burn on forfeit.** Calling `burn`/`burnFrom` on defeated deposits. Not all SEP-41 tokens expose a `burn` function. Sending to admin is simpler and more universally compatible.
- **Proposer-initiated deposit claim.** A separate `claim_deposit` entrypoint. Automatic transfer on status change is simpler and doesn't require proposers to watch for a claimable state.
- **Native XLM as deposit asset.** Stellar's native asset requires a different interface (via the native token wrapper or Stellar operations). This design restricts deposits to SEP-41 tokens only.
- **Snapshotting `deposit_token` into proposals.** Mitigates the token-address-change edge case but adds struct size and migration complexity. Left for a follow-up if the operational concern proves real.
- **Per-proposal deposit override.** Allowing a proposer to post a higher deposit for faster queuing or other perks. Out of scope for the initial mechanism.
