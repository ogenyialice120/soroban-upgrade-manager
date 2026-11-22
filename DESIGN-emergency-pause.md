# Design: Emergency Pause Switch for Upgrades

**Issue:** #110  
**Branch:** `design/emergency-pause-110`  
**Status:** Design proposal — awaiting maintainer approval before code is written.

---

## Problem

There is currently no way to halt all upgrade executions in a single operation. If a security vulnerability is discovered — in the upgrade manager itself, in a pending proposal's target WASM, or in the broader Soroban environment — the only available response is to cancel each queued proposal individually via `cancel_proposal`. That approach is fragile under time pressure: it requires enumerating every queued proposal, issuing one transaction per proposal, and racing against the timelock expiry. A single missed proposal is enough for a malicious or buggy upgrade to go through.

A global pause switch lets the admin halt all upgrade executions atomically with a single transaction, buying time to investigate and respond without racing against pending timelocks.

---

## Proposed Design

### Storage: `DataKey::Paused`

Add a new variant to `DataKey` in `types.rs`:

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
    /// Global pause flag. True = contract is paused; execute() is blocked.
    Paused,
}
```

The flag is stored as a `bool` in **instance storage**, consistent with `Admin` and `Config`. Instance storage is appropriate because the pause flag is part of the contract's operational state, not a per-proposal record. It is bumped with the same `INSTANCE_TTL_BUMP` constant (518,400 ledgers) as the rest of instance storage.

The flag is **not** added to `Config`. Adding it to `Config` would require going through `update_config`, which is heavier than a dedicated `pause()`/`unpause()` function and conflates an emergency override with a governance configuration parameter.

The flag defaults to absent, which is treated as `false` (not paused). A freshly initialized contract is never paused.

---

### New errors

Add three new error variants in `types.rs`. Exact codes are assigned sequentially after all other merged issues; these values are placeholders based on the next available code being 27:

```rust
/// Contract is paused; execute() is blocked until unpaused by admin.
ContractPaused = 27,
/// Unpause was called but the contract is not currently paused.
NotPaused = 28,
/// Pause was called but the contract is already paused.
AlreadyPaused = 29,
```

---

### New functions in `lib.rs`

#### `pause`

```rust
/// Pause the contract, blocking execute() for all proposals.
/// Only the admin can call this.
/// Returns AlreadyPaused if the contract is already paused.
pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
    caller.require_auth();
    require_admin(&env, &caller)?;

    let storage = env.storage().instance();
    if storage.get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
        return Err(Error::AlreadyPaused);
    }

    storage.set(&DataKey::Paused, &true);
    env.events().publish(("contract", "paused"), ());
    Ok(())
}
```

#### `unpause`

```rust
/// Unpause the contract, re-enabling execute() for all proposals.
/// Only the admin can call this.
/// Returns NotPaused if the contract is not currently paused.
pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
    caller.require_auth();
    require_admin(&env, &caller)?;

    let storage = env.storage().instance();
    if !storage.get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
        return Err(Error::NotPaused);
    }

    storage.set(&DataKey::Paused, &false);
    env.events().publish(("contract", "unpaused"), ());
    Ok(())
}
```

#### `is_paused`

```rust
/// Return whether the contract is currently paused.
/// Returns false if the Paused key has never been set.
pub fn is_paused(env: Env) -> bool {
    env.storage()
        .instance()
        .get::<DataKey, bool>(&DataKey::Paused)
        .unwrap_or(false)
}
```

---

### Change to `execute()`

Add a pause check at the very top of `execute()`, before `load_proposal`. Placing it first keeps the check cheap — it short-circuits without touching proposal storage — and makes the contract's paused state the highest-priority gate.

```rust
pub fn execute(env: Env, proposal_id: u64) -> Result<(), Error> {
    // Emergency pause: block all upgrade executions while paused.
    if env.storage().instance().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
        return Err(Error::ContractPaused);
    }

    let mut proposal = load_proposal(&env, proposal_id)?;
    assert_timelock_passed(&env, &proposal)?;
    proposal.status = ProposalStatus::Executed;
    governance::save_proposal(&env, &proposal);
    emit_executed(&env, proposal_id);
    invoke_upgrade(&env, &proposal.target, &proposal.new_wasm_hash);
    Ok(())
}
```

No other functions — `propose_upgrade`, `vote`, `finalize`, `cancel` — are gated by the pause flag. The pause is the minimal, targeted intervention: only the action that causes irreversible on-chain state changes (a WASM upgrade) is blocked. Governance can continue to operate while paused, allowing the admin and voters to prepare a remediation proposal without starting from scratch after the incident is resolved.

Specifically:
- `propose_upgrade` — **not gated.** Users can submit a fix proposal while paused.
- `vote` — **not gated.** Voting on existing or new proposals continues normally.
- `finalize` — **not gated.** A proposal can reach Queued status while paused, ready to execute once unpaused.
- `cancel` — **not gated.** The admin retains the ability to cancel individual proposals during a pause.

---

### Events

| Function   | Topic 1      | Topic 2      | Data |
|------------|--------------|--------------|------|
| `pause`    | `"contract"` | `"paused"`   | `()` |
| `unpause`  | `"contract"` | `"unpaused"` | `()` |

The events carry no additional data beyond the topics because the state change is binary and fully described by the event name. Off-chain monitors watching for `("contract", "paused")` can alert on a pause without needing to poll `is_paused`.

---

### Idempotency and concurrent transaction safety

`pause` and `unpause` are separate, explicit functions rather than a single toggle. A toggle is ambiguous under concurrent transactions: two rapid calls that both intend to pause could cancel each other out, leaving the contract unpaused. Separate functions make intent unambiguous. Calling `pause` while already paused returns `AlreadyPaused`; calling `unpause` while not paused returns `NotPaused`. Both errors are distinct rather than silent no-ops so that callers can detect double-calls without checking `is_paused` first.

---

### Security implications

A paused contract blocks **all** upgrade executions, including legitimate ones. The admin accepts this trade-off when using the pause as a panic button.

The pause mechanism does not expand the trust model beyond what already exists. The admin can already cancel all queued proposals to achieve the same practical outcome (no upgrades execute). `pause` achieves the same effect atomically in one transaction instead of N, which is strictly safer during an incident. If the admin key is compromised, an attacker gains the ability to pause (blocking legitimate upgrades) or unpause (allowing a malicious queued proposal to execute). Neither capability is new: a compromised admin key could already cancel all proposals or allow them to proceed by doing nothing.

---

## Existing guarantee touched

**Before:** "Anyone can call `execute()` on a Queued proposal once the timelock has elapsed."

**After:** "Anyone can call `execute()` on a Queued proposal once the timelock has elapsed, **unless the contract is paused by the admin**."

The liveness guarantee — that a Queued proposal with an expired timelock will eventually execute — now has an explicit admin-controlled exception. This is an intentional weakening in exchange for an emergency safety valve.

---

## Tests needed

| Test name | What it verifies |
|-----------|-----------------|
| `test_pause_blocks_execute` | After `pause()`, calling `execute()` on a Queued proposal with an expired timelock returns `Error::ContractPaused`. |
| `test_unpause_allows_execute` | After `pause()` then `unpause()`, calling `execute()` on a Queued proposal with an expired timelock succeeds. |
| `test_pause_requires_admin` | A non-admin address calling `pause()` returns `Error::Unauthorized`. |
| `test_unpause_requires_admin` | A non-admin address calling `unpause()` returns `Error::Unauthorized`. |
| `test_pause_already_paused` | Calling `pause()` a second time while already paused returns `Error::AlreadyPaused`. |
| `test_unpause_when_not_paused` | Calling `unpause()` on a contract that is not paused returns `Error::NotPaused`. |
| `test_pause_does_not_block_propose` | After `pause()`, `propose_upgrade()` still succeeds. |
| `test_pause_does_not_block_vote` | After `pause()`, `vote()` on an active proposal still succeeds. |
| `test_pause_does_not_block_finalize` | After `pause()`, `finalize()` on a proposal that has reached quorum still succeeds. |
| `test_pause_does_not_block_cancel` | After `pause()`, the admin calling `cancel()` on a proposal still succeeds. |
| `test_is_paused_returns_false_by_default` | `is_paused()` returns `false` on a freshly initialized contract before `pause()` has ever been called. |
| `test_is_paused_reflects_pause_unpause` | `is_paused()` returns `true` after `pause()` and `false` after the subsequent `unpause()`. |

---

## Out of scope

- **Pause by governance vote.** This design restricts pause/unpause to the admin only. A governance-controlled pause (requiring quorum) would be too slow to function as an emergency brake.
- **Automatic unpause after N ledgers.** A time-limited pause could be useful but adds complexity (a stored expiry ledger, an additional check in `execute`). It can be addressed as a follow-on feature.
- **Per-proposal pause or allowlist.** Blocking specific proposals while allowing others is a more granular mechanism that belongs in a separate design.
- **Pausing `propose_upgrade`, `vote`, or `finalize`.** The design deliberately leaves governance functions unblocked. A broader governance freeze is a separate concern.
- **Emitting the caller's address in pause/unpause events.** Event data is kept minimal (`()`) in this design. Caller attribution can be added later if needed for audit tooling.
- **Multi-sig or time-delay on pause.** Requiring multiple signers or a delay before a pause takes effect would reduce the utility of the panic button. That trade-off belongs in a separate design.
