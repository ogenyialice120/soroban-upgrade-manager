# Design: Cheaper TTL Strategy for Vote Entries (Issue #115)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #115  
**Branch:** `design/vote-ttl-strategy`  
**Type:** Manager contract change (storage and TTL model)  
**Date:** 2026-11-20

---

## 1. Problem

Each vote is stored as a separate `DataKey::Vote(proposal_id, voter)` persistent
entry with `PERSISTENT_TTL_BUMP = 2_592_000` ledgers (~150 days). For a proposal
with many voters:

- N voters → N persistent storage entries, each bumped to 150 days.
- Rent cost scales linearly with voter count.
- These entries must live at least as long as the proposal itself, but after the
  proposal is finalized, they serve only the double-vote prevention function,
  which is no longer needed.

A cheaper strategy would reduce either the per-entry cost, the number of
entries, or the entry lifetime.

---

## 2. Existing guarantee this touches

> **Per-voter vote records are stored permanently (until TTL eviction) as
> individual `DataKey::Vote(proposal_id, voter)` persistent entries.**
> `has_voted(proposal_id, voter)` checks existence of this key to prevent
> double voting.

Any cheaper strategy must still prevent double voting during the proposal's
active voting window.

---

## 3. Analysis of current storage cost

Each `Vote(proposal_id, voter)` entry stores a `bool` (1 byte), but the key
itself encodes a `u64` proposal ID and a Stellar `Address` (32-byte Ed25519
public key or 32-byte contract ID). The key serialization in Soroban XDR adds
overhead. A rough estimate per entry:

- Key: ~70–80 bytes (XDR encoding of the enum variant + u64 + Address)
- Value: ~8 bytes (XDR bool)
- Total: ~90 bytes per vote entry

At `PERSISTENT_TTL_BUMP = 2_592_000` ledgers the rent obligation is
proportional to `size * ttl`. For a proposal with 1,000 voters, that is
~90,000 bytes * 150 days of persistent rent — a meaningful cost for high-
participation proposals.

---

## 4. Alternatives considered

### 4.1 Shorter TTL tied to proposal lifetime

**Approach:** Set `Vote` entry TTL to `voting_period + timelock_delay +
execution_window` instead of a flat 150 days. The vote entry only needs to
exist while the proposal is `Active` (to block double voting) and `Queued`
(no use — but the contract must not evict it while the proposal is non-final
to avoid confusion). Once the proposal is `Executed`, `Cancelled`, or
`Defeated`, votes are no longer needed.

**Implementation:**

```rust
// In record_vote: use the snapshotted voting period + timelock as the TTL.
let ttl = proposal.snap_voting_period
    .checked_add(proposal.snap_timelock_delay)
    .unwrap_or(PERSISTENT_TTL_BUMP)
    .max(MIN_VOTE_TTL); // floor to prevent immediate eviction
env.storage().persistent().set(&key, &true);
env.storage().persistent().extend_ttl(&key, ttl, ttl);
```

**Trade-off:** Cheaper for short-lived proposals. No behavior change — double-
vote prevention is only relevant during the voting period, which always
completes before the TTL expires. After finalization, vote entries expire
naturally and free rent.

**Risk:** If `voting_period` is very short (e.g., 1 ledger in an emergency
proposal) and `timelock_delay` is small, the TTL could be too short for the
entry to survive until finalization. A floor `MIN_VOTE_TTL` (e.g., 10,000
ledgers ≈ 14 hours) prevents premature eviction.

**Recommendation: implement this.** It is a pure optimization with no
behavioral change. Existing double-vote prevention is unaffected.

### 4.2 Proposal-scoped vote bitmap in instance storage

**Approach:** Instead of one persistent entry per voter, store a serialized
set of voted addresses in the `Proposal` struct itself (or in a compact
instance-storage entry).

**Problems:**

- Soroban has a 64 KB instance storage limit. For large voter counts, this
  is insufficient.
- `soroban_sdk::Vec<Address>` stored in instance storage grows proportionally
  with voter count. Linear search to check `has_voted` would be O(N).
- Proposal entries are persistent, not instance. Moving vote tracking into the
  `Proposal` persistent entry would inflate the proposal's serialized size
  without bound.
- Address sets cannot use Soroban's `Map` type for O(1) lookup in instance
  storage (instance storage is a key-value map but values must be
  `contracttype`-serializable values, not sub-maps).

**Conclusion:** Not viable for unbounded voter counts.

### 4.3 Vote count only (no per-voter records)

**Approach:** Remove `DataKey::Vote(proposal_id, voter)` entries entirely.
Track only `yes_votes` and `no_votes` on the `Proposal` struct. Remove the
double-vote check.

**Problem:** Double voting becomes possible. Any voter can call `vote()` N
times, inflating counts. This would completely break the governance security
model.

**Conclusion:** Not viable without sybil resistance at the Soroban level (which
does not exist). Rejected.

### 4.4 Proposal-TTL-aligned persistent entries (recommended)

This is a refinement of Alternative 4.1.

After a proposal reaches a terminal state (`Executed`, `Cancelled`, or
`Defeated`), its vote entries are no longer needed. Currently there is no
mechanism to clean them up.

**Approach:** Set vote entry TTL to `voting_period + timelock_delay + buffer`
at record time (as in 4.1). Additionally, provide a new `cleanup_votes`
function that the admin or anyone can call after a proposal is terminal to
explicitly remove vote entries, earning a storage refund.

```rust
/// Remove all Vote entries for a finalized proposal.
/// Can be called by anyone after the proposal reaches a terminal state.
/// Returns the number of entries removed.
///
/// # Errors
/// - `ProposalNotFound` — unknown proposal ID.
/// - `ProposalNotFinalized` — proposal is still Active or Queued.
pub fn cleanup_votes(env: Env, proposal_id: u64, voters: Vec<Address>) -> Result<u32, Error>
```

Callers supply the list of voters to clean up (the contract cannot iterate
all keys — Soroban has no prefix-scan). Off-chain indexers can reconstruct the
voter list from `vote_cast` events.

**Trade-offs:**
- Provides an explicit cleanup path and storage refund.
- Requires off-chain knowledge of voter addresses.
- Double voting is still prevented during the active window.
- Adds a new function and a new error code.

---

## 5. Recommendation

**Implement Alternative 4.1 (shorter TTL) as the primary change.**

Set the vote entry TTL to `max(voting_period + timelock_delay + 50_000, MIN_VOTE_TTL)` where:
- `50_000` ledgers (~69 days) is a generous buffer after timelock expiry.
- `MIN_VOTE_TTL = 17_280` (1 day) is a floor.

This reduces rent for typical governance deployments (e.g., 7-day voting +
1-day timelock ≈ 2.9M-ledger TTL → ≈ 170,000-ledger TTL, a 15× reduction)
with no behavior change.

**Do not implement Alternative 4.4 (`cleanup_votes`) in the same PR.** It is
an independent optimization that adds API surface. It can be proposed
separately after 4.1 lands.

---

## 6. Existing guarantee preserved

Double-vote prevention is preserved: `has_voted` checks the same
`DataKey::Vote(proposal_id, voter)` key. As long as the TTL floor ensures
the entry survives past `voting_ends_at`, the check is reliable. The TTL
floor `MIN_VOTE_TTL = 17_280` and the `voting_period + timelock_delay`
calculation guarantee the entry outlives the proposal's active window.

---

## 7. Files affected (when implemented)

- `contracts/upgrade-manager/src/types.rs` — add `MIN_VOTE_TTL: u32` constant;
  optionally update `PERSISTENT_TTL_BUMP` comment to note it no longer applies
  to vote entries.
- `contracts/upgrade-manager/src/governance.rs` — `record_vote`: compute
  per-proposal TTL from `snap_voting_period + snap_timelock_delay + 50_000`,
  apply floor `MIN_VOTE_TTL`; pass the computed TTL to `extend_ttl` instead
  of `PERSISTENT_TTL_BUMP`. Requires `Proposal` to be passed into
  `record_vote` (signature change from `(env, proposal_id, voter)` to
  `(env, proposal_id, voter, &Proposal)`).
- `contracts/upgrade-manager/src/test.rs` — update `test_ttl_extended_on_write_and_read`
  to reflect the new computed TTL; add new tests (see §8).
- `_CHANGELOG.md` — append a dated entry per CONTRIBUTING.md.

---

## 8. Tests needed

| Test name | What it verifies |
|-----------|-----------------|
| `test_vote_ttl_shorter_than_persistent_ttl_bump` | Vote entry TTL is strictly less than `PERSISTENT_TTL_BUMP` for a short-voting-period proposal. |
| `test_vote_ttl_equals_voting_period_plus_timelock_plus_buffer` | Vote entry TTL equals `snap_voting_period + snap_timelock_delay + 50_000` (or the floor) for a known proposal. |
| `test_vote_ttl_floor_applied_for_very_short_period` | A proposal with `voting_period = 1` and `timelock_delay = 1` still has `vote_ttl >= MIN_VOTE_TTL`. |
| `test_double_vote_still_prevented` | Double-vote prevention still works with the new TTL strategy (vote entry exists during voting window). |
| `test_vote_ttl_bounded_by_max_proposal_ttl` | For a maximum-length proposal, vote TTL does not exceed `PERSISTENT_TTL_BUMP` (or documents when it might). |

---

## 9. Out of scope

- Bitmap or set-based vote storage (not viable for unbounded voter counts).
- Removing per-voter records entirely (breaks double-vote prevention).
- `cleanup_votes` API (independent follow-up).
- No implementation code in this PR — this document is the design proposal
  only. Implementation proceeds only after maintainer approval.
