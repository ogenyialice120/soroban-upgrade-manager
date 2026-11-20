# Migration Guide: Pre-Phase 1 → Phase 1

This guide documents the **Phase 1 breaking changes** to the Soroban Upgrade
Manager contract and what you must update when upgrading an integration from
the pre-Phase 1 contract.

It mirrors the **"BREAKING changes summary for Phase 1"** section of
`_CHANGELOG.md`. Every change below matches that summary.

## Summary of breaking changes

1. `initialize` takes a new required argument: `voting_period: u32`.
2. `get_config` and `get_admin` return `Result<_, Error>` instead of bare values.
3. `proposal.executable_at` is `0` until `finalize()` is called.
4. `finalize()` now requires the voting period to have ended (returns
   `VotingStillActive` if called too early); previously it required
   `executable_at` to have passed.
5. `cancel()` now returns `ProposalFinalized` for `Defeated` proposals too.

---

## 1. `initialize` now takes a required `voting_period` argument

### What changed

`initialize` previously accepted `(admin, timelock_delay, quorum,
approval_threshold_pct)`. Phase 1 added a new required first config argument,
`voting_period: u32`, so the signature is now:

```
initialize(admin, voting_period, timelock_delay, quorum, approval_threshold_pct)
```

`voting_period` is the number of ledgers during which votes are accepted on a
proposal (must be >= 1). It is stored in `Config` and snapshotted into each
proposal at creation time.

### Why

Pre-Phase 1 there was no voting window: once a proposal existed it could be
finalized immediately. Phase 1 introduces a proper voting period that runs from
proposal creation to `voting_ends_at`, separate from the timelock delay that
runs from `finalize()` to `executable_at`.

### What you must update

Every call to `initialize` must now pass a `voting_period` value as the second
argument (after `admin`).

#### Before (pre-Phase 1)

Rust SDK client:

```rust
client.initialize(&admin, &100u32, &3u32, &51u32);
//                                        ^ timelock_delay   ^ quorum   ^ threshold
// (no voting_period argument)
```

CLI:

```bash
stellar contract invoke --id "$MANAGER_ID" --source alice \
  -- initialize \
  --admin alice \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

#### After (Phase 1)

Rust SDK client:

```rust
client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
//                                        ^ voting_period  ^ timelock_delay  ^ quorum   ^ threshold
```

CLI:

```bash
stellar contract invoke --id "$MANAGER_ID" --source alice \
  -- initialize \
  --admin alice \
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

> Tip: `voting_period` and `timelock_delay` are both in ledgers. At ~5 s/ledger,
> `17_280` ≈ 24 hours. The voting period is when votes are accepted; the timelock
> delay is the mandatory wait between `finalize()` and `execute()`.

## 3. `proposal.executable_at` is `0` until `finalize()` is called

### What changed

The `Proposal.executable_at` field is now `0` until the proposal is finalized.
On a successful `finalize()`, it is set to `finalize_ledger + timelock_delay`.

Pre-Phase 1, `executable_at` was set at proposal creation to
`created_at + timelock_delay`, so it was non-zero from the start and did not
mean "when execution is permitted".

### Why

Pre-Phase 1 the timelock was measured from proposal creation, so a
finalized-Queued proposal could be executed immediately (the delay had already
elapsed). Phase 1 moves the timelock to start at `finalize()` time:
`executable_at = finalize_ledger + timelock_delay`. Until then `executable_at`
is `0`, and `execute()` requires `ledger.sequence() >= executable_at`.

### What you must update

- Do not read `executable_at` before `finalize()` expecting a future execution
  deadline — it will be `0`. Use `proposal.voting_ends_at` for the voting
  deadline and `time_until_executable()` for the remaining timelock.
- After `finalize()`, read `executable_at` (or call `time_until_executable()`)
  to know when `execute()` becomes permitted.

#### Before (pre-Phase 1)

```rust
let proposal = client.get_proposal(&id)?;
// executable_at was already = created_at + timelock_delay (non-zero)
let executable_at = proposal.executable_at;
```

#### After (Phase 1)

```rust
let proposal = client.get_proposal(&id)?;

// Before finalize: executable_at == 0
assert_eq!(proposal.executable_at, 0);

// After a successful finalize: executable_at = finalize_ledger + timelock_delay
let finalized = client.get_proposal(&id)?;
let executable_at = finalized.executable_at; // now non-zero
```

Or use the read-only query:

```rust
let remaining = client.time_until_executable(&id)?; // 0 => execution permitted
```

## 4. `finalize()` timing: after the voting period, not after the timelock

### What changed

`finalize()` now requires the voting period to have ended:
`ledger.sequence() > proposal.voting_ends_at`. If called too early it returns
`VotingStillActive`. On pass it moves the proposal to `Queued` and sets
`executable_at = finalize_ledger + timelock_delay`.

Pre-Phase 1, `finalize()` required `executable_at` to have already passed
(reporting `TimelockNotExpired` otherwise), which effectively meant the delay
ran from creation, not from approval.

### Why

Pre-Phase 1 the naming was backwards: `finalize()` waited for the timelock to
expire instead of waiting for the vote to close, and `TimelockNotExpired` was
raised by `finalize()` rather than by `execute()`. Phase 1 corrects the order:

```
propose_upgrade → vote (during voting_period) → finalize (after voting ends)
  → Queued → wait timelock_delay → execute (after executable_at)
```

The timelock is now a genuine mandatory delay between approval and execution.

### What you must update

- Do not call `finalize()` while voting is still open. Wait until
  `ledger.sequence() > voting_ends_at` (the voting period has ended).
- Handle `VotingStillActive` if you call `finalize()` too early.
- `TimelockNotExpired` is now returned by `execute()` (when called before
  `executable_at`), not by `finalize()`. Update any error handling that
  associated `TimelockNotExpired` with `finalize()`.

#### Before (pre-Phase 1)

```rust
// finalize() waited for executable_at (set at creation) to pass.
client.finalize(&id)?; // TimelockNotExpired if too early
```

#### After (Phase 1)

```rust
// Wait until the voting period ends, then finalize.
// finalize() returns VotingStillActive if called before voting_ends_at.
client.finalize(&id)?; // OK once ledger.sequence() > voting_ends_at

// execute() enforces the timelock: TimelockNotExpired before executable_at.
client.execute(&id)?; // TimelockNotExpired if ledger.sequence() < executable_at
```

CLI example:

```bash
# finalize after voting period ends
stellar contract invoke --id "$MANAGER_ID" --source alice \
  -- finalize --proposal_id 0
```

## 5. `cancel()` now rejects `Defeated` proposals with `ProposalFinalized`

### What changed

`cancel()` previously returned `ProposalFinalized` only for `Executed` and
`Cancelled` proposals. Phase 1 extends this to `Defeated` proposals too: any
proposal whose status is `Executed`, `Cancelled`, or `Defeated` is considered
finalized and cannot be cancelled.

Only `Active` and `Queued` proposals can be cancelled.

### Why

A `Defeated` proposal is already terminal (the vote failed), so cancelling it
is meaningless. Returning `ProposalFinalized` makes the cancel semantics
consistent across all terminal states and prevents callers from assuming a
`Defeated` proposal can still be revoked.

### What you must update

- Only attempt `cancel()` on `Active` or `Queued` proposals.
- Handle `ProposalFinalized` when cancelling a `Defeated` (or `Executed` /
  `Cancelled`) proposal.

#### Before (pre-Phase 1)

```rust
// Defeated proposals were not matched as finalized; cancel logic differed.
client.cancel(&admin, &id)?;
```

#### After (Phase 1)

```rust
match client.cancel(&admin, &id) {
    Ok(()) => println!("cancelled"),
    Err(Ok(Error::ProposalFinalized)) => {
        println!("proposal is already finalized (Executed/Cancelled/Defeated)");
    }
    Err(other) => Err(other)?,
}
```

---

## Quick reference table

| Change | Pre-Phase 1 | Phase 1 | Action required |
|--------|-------------|---------|-----------------|
| `initialize` | `(admin, timelock_delay, quorum, threshold)` | `(admin, voting_period, timelock_delay, quorum, threshold)` | Add `voting_period` argument |
| `get_config` | `Config` (bare) | `Result<Config, Error>` | Handle `Result` / `NotInitialized` |
| `get_admin` | `Address` (bare) | `Result<Address, Error>` | Handle `Result` / `NotInitialized` |
| `executable_at` | `created_at + timelock_delay` (non-zero at creation) | `0` until `finalize()`, then `finalize_ledger + timelock_delay` | Don't read before finalize; use `time_until_executable()` |
| `finalize()` | Requires `executable_at` passed (`TimelockNotExpired`) | Requires voting period ended (`VotingStillActive`) | Wait for voting period to end |
| `cancel()` | `ProposalFinalized` for `Executed`/`Cancelled` | `ProposalFinalized` for `Executed`/`Cancelled`/`Defeated` | Only cancel `Active`/`Queued` |

---

## Source of truth

These changes match the **"BREAKING changes summary for Phase 1"** section of
`_CHANGELOG.md`. If anything here conflicts with the changelog or the current
source in `contracts/upgrade-manager/src/`, the changelog and source take
precedence.