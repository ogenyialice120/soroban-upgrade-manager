# Design: Proposal Vote Receipts (#122)

**Status:** Design — awaiting maintainer approval before any code is written.

---

## Summary

`record_vote` in `governance.rs` currently stores `true` as the value for
`DataKey::Vote(proposal_id, voter)`. This means the direction a voter chose
(YES or NO) is discarded once the vote is counted in `proposal.yes_votes` /
`proposal.no_votes`. Storing the `approve` boolean instead of `true` turns
each `Vote` entry into a **receipt** — a durable, verifiable record of what
the voter actually chose.

---

## Existing guarantee this change touches

> **Vote direction is not stored per-voter.** Only the aggregate `yes_votes`
> and `no_votes` counters on the `Proposal` struct record the split. Once
> `cast_vote` returns, there is no on-chain way to prove, query, or audit
> which way a specific address voted.

This guarantee will be **removed** by the change: vote direction becomes
per-voter persistent state.

---

## Proposed storage change

### Current

```rust
// governance.rs — record_vote
pub fn record_vote(env: &Env, proposal_id: u64, voter: &Address) {
    let key = DataKey::Vote(proposal_id, voter.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
}
```

Value stored: `bool` = `true`.

### Proposed

```rust
pub fn record_vote(env: &Env, proposal_id: u64, voter: &Address, approve: bool) {
    let key = DataKey::Vote(proposal_id, voter.clone());
    env.storage().persistent().set(&key, &approve);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
}
```

Value stored: `bool` = `approve` (`true` = YES, `false` = NO).

The `DataKey::Vote` variant is unchanged — only the stored value changes from
the constant `true` to the voter's actual choice. This is the smallest possible
change to the storage layout.

---

## Backwards compatibility

Votes cast before this upgrade were written as `true`. Because `true` happens
to equal a YES vote, any existing vote entry read back as a `bool` will appear
as YES. That is **incorrect for NO voters**.

There is no safe way to distinguish old `true` entries (existence-only
sentinels) from new `true` entries (YES receipts) without an additional
sentinel flag, which would add storage cost and complexity.

**Recommendation:** Accept the ambiguity and document it. The change is
additive for new proposals. Historical proposals that were finalized before
the upgrade already have their aggregate counts recorded in `yes_votes` /
`no_votes` on the `Proposal` struct and are unlikely to need per-voter
auditing after the fact. Any deployment that requires a clean migration can
drain all active proposals before upgrading the contract.

This is the right trade-off because:
1. The storage value is a `bool` in both old and new code — no type
   mismatch, no deserialization failure.
2. Old YES votes remain correct. Old NO votes appear as YES in `get_vote`
   — an incorrect result, but only for the small window of votes cast before
   the upgrade. Aggregate counts on proposals are not affected.
3. Adding a version sentinel (e.g., wrapping the value in an enum) adds
   per-vote storage cost of at least 4 bytes for every future vote and
   complicates `has_voted` and `get_vote` for no benefit beyond migration
   correctness.

---

## New read-only view: `get_vote`

```rust
/// Return the vote direction cast by `voter` on `proposal_id`.
///
/// Returns `true` for YES, `false` for NO.
///
/// # Errors
/// - `ProposalNotFound` — no vote record exists for this (proposal_id, voter) pair,
///   which means the voter has not voted on this proposal.
pub fn get_vote(env: Env, proposal_id: u64, voter: Address) -> Result<bool, Error> {
    let key = DataKey::Vote(proposal_id, voter);
    env.storage()
        .persistent()
        .get::<DataKey, bool>(&key)
        .ok_or(Error::ProposalNotFound)
}
```

The error re-uses `ProposalNotFound` to signal "no vote for this pair". An
alternative is a new `VoteNotFound` error (would require a new error variant
at index 25); this is a minor design choice left for the maintainer to decide.
Using `ProposalNotFound` avoids adding an error code but is slightly misleading
— the proposal may exist but the voter simply didn't vote.

**Recommendation:** Add `VoteNotFound = 25` to `types::Error`. It makes the
API self-documenting and avoids overloading `ProposalNotFound`.

---

## Storage cost impact

| | Before | After |
|---|---|---|
| Bytes per Vote entry | 1 byte (`true`) | 1 byte (`approve`) |
| Net change | 0 bytes | 0 bytes |

The value type is `bool` in both cases. Soroban encodes a `bool` as a 1-byte
XDR `ScVal::Bool`. Storage cost per vote is unchanged.

---

## Call-site change in `cast_vote`

`cast_vote` currently calls:
```rust
record_vote(env, proposal_id, &voter);
```

It would become:
```rust
record_vote(env, proposal_id, &voter, approve);
```

The `approve` argument is already in scope at the call site.

---

## Files that change

| File | Change |
|---|---|
| `contracts/upgrade-manager/src/governance.rs` | `record_vote` gains `approve: bool` parameter; `cast_vote` passes `approve`; new `get_vote` helper added |
| `contracts/upgrade-manager/src/lib.rs` | New `get_vote(env, proposal_id, voter) -> Result<bool, Error>` public entry point |
| `contracts/upgrade-manager/src/types.rs` | Optional: new `VoteNotFound = 25` error variant |

No changes to `DataKey`, `Proposal`, `Config`, `ProposalStatus`, or any other
type.

---

## Tests needed

1. **`test_get_vote_yes`** — cast a YES vote; call `get_vote(proposal_id, voter)`;
   assert the result is `Ok(true)`.

2. **`test_get_vote_no`** — cast a NO vote; call `get_vote(proposal_id, voter)`;
   assert the result is `Ok(false)`.

3. **`test_get_vote_not_voted_returns_vote_not_found`** — call `get_vote` for an
   address that has not voted; assert the result is `Err(Ok(Error::VoteNotFound))`
   (or `ProposalNotFound` if no new error is added).

4. **`test_get_vote_multiple_voters_independent`** — cast YES for voter A and NO
   for voter B on the same proposal; assert `get_vote` returns `true` for A and
   `false` for B.

5. **`test_get_vote_multiple_proposals_independent`** — voter votes YES on
   proposal 0 and NO on proposal 1; assert each receipt is correct and the
   proposals are independent.

6. **`test_get_vote_does_not_affect_aggregate_counts`** — cast votes and confirm
   that `yes_votes` and `no_votes` on the `Proposal` struct are unchanged by the
   new storage value (i.e., the aggregate counts are still correct).

7. **`test_get_vote_delegated_voter`** — voter A delegates to voter B; voter B
   casts a YES vote (carrying weight 2); assert `get_vote(proposal_id, voter_B)`
   returns `Ok(true)` and voter A has no receipt (returns `VoteNotFound`).

8. **`test_get_vote_after_proposal_cancelled`** — cast a YES vote; cancel the
   proposal; assert the receipt is still readable (storage is not erased on
   cancel).

---

## Out of scope

- Do not implement until a maintainer approves this design.
- Indexing or listing all votes for a proposal (requires prefix iteration, which
  Soroban does not support; would need a separate counter/set structure).
- Vote weight in the receipt (the `weight` is emitted in the `vote_cast` event
  but not stored in the Vote entry; storing it would increase per-vote cost).
