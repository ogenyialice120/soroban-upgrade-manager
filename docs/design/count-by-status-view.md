# Design: Read-Only count_by_status View (Issue #126)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #126
**Branch:** `design/count-by-status`
**Type:** Manager contract read-only view
**Date:** 2026-11-22

---

## 1. Problem

There is no aggregate view that counts proposals by status. Dashboards and
off-chain tooling that want to show how many proposals are `Active`, `Queued`,
`Executed`, `Cancelled`, or `Defeated` must call `get_proposals` page by page
and tally statuses client-side, duplicating the counting logic across every
consumer and paying repeated paginated reads.

A `count_by_status()` view returns the per-status counts in a single call —
one authoritative aggregate.

## 2. Existing guarantee touched

**None.** This is an additive read-only view. It reads the same `Proposal`
entries already exposed by `get_proposal` / `get_proposals` and tallies their
`status` field. It adds no storage, no authorization, and no state mutation
beyond the existing persistent-TTL bump that every `load_proposal` already
performs. It does not weaken or change any existing on-chain guarantee.

## 3. Design

### 3.1 Function signature

```rust
/// Return the number of proposals in each `ProposalStatus`.
///
/// Read-only; no authorization required.
///
/// # Returns
/// A `Map<ProposalStatus, u64>` with one entry per status variant, including
/// zero-count statuses. On an uninitialised contract (or with no proposals)
/// every entry is `0`.
pub fn count_by_status(env: Env) -> Map<ProposalStatus, u64>
```

### 3.2 Return type: `Map<ProposalStatus, u64>`

The view returns a `Map<ProposalStatus, u64>` keyed by the five `ProposalStatus`
variants, each initialised to `0` and then incremented as proposals are read.

Why a `Map` rather than a 5-field struct (`StatusCounts { active, queued,
executed, cancelled, defeated }`):

- It matches the issue's stated signature and is self-describing: the keys
  *are* the statuses, so a consumer never has to remember field order.
- It stays correct if `ProposalStatus` ever gains a variant — the tally loop
  naturally includes the new status, whereas a struct would need a new field
  and a contract change.
- The fixed set of five variants keeps construction and serialization cheap.

The struct alternative is viable and slightly cheaper to decode on the host,
but loses extensibility and self-description. The `Map` is recommended.

### 3.3 Computation (on-demand)

The view is computed on demand from existing storage:

```text
count   = instance.get(DataKey::ProposalCount)  // unwrap_or(0)
counts  = Map { Active: 0, Queued: 0, Executed: 0, Cancelled: 0, Defeated: 0 }
for id in 0 .. count:
    p = load_proposal(id)        // persistent read + existing TTL bump
    counts[p.status] += 1
return counts
```

Proposal IDs are dense and sequential (`0 .. ProposalCount`), created by
`next_proposal_id` and never deleted, so a simple `0 .. count` loop covers
exactly the live proposals — the same iteration shape `get_proposals` already
uses.

Gas cost is **O(n)** in the total proposal count: one persistent storage read
per proposal (plus the existing TTL bump). For the small proposal volumes
expected of a lean upgrade layer this is fine; it is the same per-proposal
read cost `get_proposals` already incurs.

### 3.4 Cache vs. on-demand

| Aspect | On-demand (recommended for v1) | Cached counters |
|---|---|---|
| Query cost | O(n) reads per call | O(1): read 5 entries |
| Write cost on transitions | None | One extra write on every `create`, `finalize`, `execute`, `cancel` |
| Correctness | Correct by construction (tallies live state) | Risk of drift if any transition path forgets to update a counter |
| Storage | None | 5 persistent entries (`StatusCount(ProposalStatus)`) |
| Extensibility | Automatic for new status variants | Needs a new counter per new variant |

**Recommendation: on-demand for v1.** Reasons:

1. Proposal counts for an upgrade manager are expected to stay small — this is
   a lean upgrade layer, not a high-throughput DAO.
2. It adds no write amplification to the state-machine hot path
   (`vote`, `finalize`, `execute`, `cancel`) and no drift risk.
3. It matches the existing `get_proposals` pattern, which already iterates
   proposals by ID.

If query frequency or proposal count grows, cached counters are a
**backward-compatible follow-up**: the view's signature and return type are
unchanged, only the implementation switches from tallying to reading the
counters (and the counters must be backfilled once when first enabled).

### 3.5 Edge cases

- **Uninitialised contract / zero proposals:** `ProposalCount` is absent;
  treat as `0` (mirroring `get_proposals`, which uses `unwrap_or(0)`). Return a
  map with all five statuses `0`. No error is returned.
- **All proposals in one status:** the other four entries are `0` (always
  present, never omitted).
- Arithmetic uses `checked_add` / saturating add so a pathological count can
  never overflow `u64`.

## 4. Tests needed

1. Uninitialised contract (or zero proposals): returns a map with all five
   statuses `0`.
2. A single `Active` proposal: `Active == 1`, the other four `0`.
3. Mixed statuses after a full lifecycle: create several proposals, finalize
   some to `Queued` and some to `Defeated`, execute one to `Executed`, cancel
   one to `Cancelled`; assert every count matches exactly.
4. Counts track each transition:
   - create → `Active +1`;
   - finalize pass → `Active -1`, `Queued +1`;
   - finalize fail → `Active -1`, `Defeated +1`;
   - execute → `Queued -1`, `Executed +1`;
   - cancel (Active or Queued) → that status `-1`, `Cancelled +1`.
5. `count_by_status` is read-only: calling it does not change proposal state or
   emit events (only the existing TTL bumps).
6. Sum of all status counts equals `ProposalCount`.
7. Consistency with `get_proposals`: tallying statuses across paginated
   `get_proposals` calls equals `count_by_status`.

## 5. Out of scope

- New storage (on-demand reads existing proposals only).
- Authorization (the view is open to anyone).
- Cached counters (documented as a follow-up, not v1).
- Changing `get_proposal`, `get_proposals`, `finalize`, `execute`, or
  `cancel`.
- Code: no contract code is written until this design is approved by a
  maintainer.
