# Checks-Effects-Interactions Pattern in `execute()`

## What the pattern is

The checks-effects-interactions (CEI) pattern structures state-changing functions in three ordered phases:

1. **Checks** — verify all preconditions (proposal exists, status is Queued, timelock has passed).
2. **Effects** — update contract state to reflect the new reality.
3. **Interactions** — make any external calls.

The rule is strict: state must be written before any external call is made. If the external call comes first, a failed or malicious callee can re-enter the contract while the state still looks like it did before the call, opening a window for double-execution or inconsistent state.

## How `execute()` applies it

```rust
// contracts/upgrade-manager/src/lib.rs

pub fn execute(env: Env, proposal_id: u64) -> Result<(), Error> {
    let mut proposal = load_proposal(&env, proposal_id)?;

    // CHECKS — proposal must be Queued and timelock elapsed
    assert_timelock_passed(&env, &proposal)?;

    // EFFECTS — mark Executed BEFORE the external call
    proposal.status = ProposalStatus::Executed;
    governance::save_proposal(&env, &proposal);

    // emit event before external call (also an effect)
    emit_executed(&env, proposal_id);

    // INTERACTIONS — cross-contract upgrade call
    invoke_upgrade(&env, &proposal.target, &proposal.new_wasm_hash);

    Ok(())
}
```

The status is written to persistent storage and the event is emitted before `invoke_upgrade` crosses the contract boundary. At the point where the external call begins, any subsequent read of this proposal will see `ProposalStatus::Executed`.

## What would happen without it

If `invoke_upgrade` were called before `save_proposal`, two problems arise:

**Double-execution.** If the target contract's `upgrade()` function called back into `execute()` (re-entrancy), it would find the proposal still in `Queued` state, pass the timelock check again, and execute a second time. Soroban's execution model makes classic re-entrancy harder than in the EVM, but the CEI ordering removes the possibility entirely rather than relying on platform guarantees.

**Stuck Executed status on failure.** If `save_proposal` were called after `invoke_upgrade` and the upgrade call reverted, the transaction would revert entirely — including the status write — leaving the proposal in `Queued`. This is actually the correct behavior, and it is exactly what the current code achieves. A naive implementation that wrote status *after* a successful call would need to handle partial failure explicitly; CEI makes the rollback automatic.

## How Soroban's atomicity preserves the invariant

On Soroban, every transaction either commits fully or reverts fully. If `invoke_upgrade` panics or returns a host error, the entire transaction is rolled back, including the `save_proposal` write. The proposal returns to `Queued` as if `execute()` was never called.

This is why the pattern works here without additional error handling: the `save_proposal` before the call sets up the intended final state, and Soroban's atomicity either commits that state along with the successful upgrade or discards it along with the failed one.

## Verification test

`test_failed_upgrade_leaves_proposal_queued` in `contracts/upgrade-manager/src/test.rs` directly exercises this invariant. It registers a target contract with no `upgrade()` function (`NoUpgradeFn`), runs the full proposal lifecycle to `Queued`, advances past the timelock, then calls `execute()`. The call fails at the `invoke_upgrade` step because the target has no matching entry point. After the failure, the test asserts the proposal is still `Queued`:

```rust
// From test.rs — test_failed_upgrade_leaves_proposal_queued
let result = manager.try_execute(&id);
assert!(result.is_err(), "execute must fail when target has no upgrade()");

let proposal = manager.get_proposal(&id);
assert_eq!(
    proposal.status,
    ProposalStatus::Queued,
    "proposal must remain Queued after a failed upgrade"
);
```

The test passes because the transaction revert undoes the `save_proposal` write, confirming that the CEI ordering does not leave a proposal permanently stuck in `Executed` after a failed call.

## Summary

| Phase | Code | What it does |
|---|---|---|
| Check | `assert_timelock_passed(...)` | Verifies Queued status and elapsed timelock |
| Effect | `proposal.status = Executed; save_proposal(...)` | Writes final state before any external call |
| Effect | `emit_executed(...)` | Emits the event before any external call |
| Interaction | `invoke_upgrade(...)` | Calls `target.upgrade(new_wasm_hash)` |

If the interaction fails, Soroban reverts the whole transaction, including the effects, so the proposal returns to `Queued` automatically.
