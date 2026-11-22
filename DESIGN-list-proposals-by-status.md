# Design: Read-only list_proposals_by_status view

**Issue:** #106  
**Branch:** `design/list-proposals-by-status-106`  
**Status:** Design proposal — awaiting maintainer approval before code is written.

---

## Problem

The contract exposes two read functions for proposals:

- `get_proposal(env, proposal_id)` — fetches one proposal by ID.
- `get_proposals(env, offset, limit)` — fetches a page of proposals by ID range, returning all statuses mixed together.

Neither function lets a caller retrieve proposals filtered by status. A dashboard showing all Active proposals, or a monitoring tool checking for Queued proposals ready to execute, must call `get_proposals` for every page, then discard proposals that do not match the desired status. This burns RPC calls and forces filtering logic into every client.

The gap is a missing read-only view: given a `ProposalStatus`, return the matching proposals in ascending ID order with offset-based pagination over the filtered results.

---

## Proposed Design

### Function signature

```rust
pub fn list_proposals_by_status(
    env: Env,
    status: ProposalStatus,
    offset: u64,
    limit: u32,
) -> Result<Vec<Proposal>, Error>
```

`status` — the status to filter on. One of `Active`, `Queued`, `Executed`, `Cancelled`, `Defeated`.

`offset` — the number of matching proposals to skip before collecting results. This is **not** a proposal ID; it is a logical position within the filtered result set. To page through results, increment `offset` by `limit` on each call.

`limit` — the maximum number of proposals to return, capped at `MAX_PROPOSALS_PAGE_LIMIT` (50). Passing `0` returns an empty vec immediately.

Returns full `Proposal` structs in ascending proposal-ID order.

### Return type

The function returns `Vec<Proposal>` — full structs, not just IDs. Callers building dashboards or audit tools need the complete proposal data (title, votes, timelock expiry, etc.) in a single call. Returning only IDs would force every caller to make up to 50 additional `get_proposal` RPC calls per page.

### Offset semantics

`offset` counts matching proposals, not proposal IDs. This keeps pagination behavior consistent regardless of how the matching proposals are distributed across the full ID space.

Example: five Active proposals exist with IDs 0, 2, 5, 7, 9. To retrieve them in two pages:

- Page 1: `list_proposals_by_status(Active, 0, 3)` → proposals 0, 2, 5
- Page 2: `list_proposals_by_status(Active, 3, 3)` → proposals 7, 9

Callers do not need to know the underlying IDs to paginate; they only need to track how many matching proposals they have already seen.

### Gas cost and iteration

Soroban provides no built-in index or prefix scan. The implementation scans proposal IDs from `0` to `ProposalCount - 1`, loads each proposal from persistent storage, and checks its status. The complexity is O(total proposals), not O(matching proposals). With `MAX_PROPOSALS_PAGE_LIMIT = 50`, the function collects at most 50 results but may read every proposal in the contract to find them.

This is acceptable for the following reasons:

1. The function is read-only. It makes no state changes, so there is no risk of partial writes or corrupted state if the scan is expensive.
2. Persistent storage reads in Soroban do not write ledger entries, so the fee model is read-only throughout.
3. The contract is not intended for deployments with millions of proposals. The typical use case involves tens to low hundreds of proposals, where a full scan is fast and cheap.
4. Callers that do not need status filtering can use `get_proposals`, which pages by ID range and avoids a full scan.
5. If the proposal count grows to the point where full scans become prohibitively expensive, a secondary index can be added in a future design (see [Out of scope](#out-of-scope)).

### Secondary index — not added

A secondary index such as `DataKey::StatusIndex(ProposalStatus) -> Vec<u64>` would make filtered reads O(results) instead of O(total proposals). It is **not** included in this design.

Adding a secondary index requires updating it on every proposal state transition: `create_proposal` (insert into `Active` index), `finalize_voting` (move from `Active` to `Queued` or `Defeated`), `cancel_proposal` (move to `Cancelled`), and `execute` (move to `Executed`). Each transition would require reading the index vec, mutating it, and writing it back. This adds storage cost to every write path and increases the surface area for bugs — particularly around keeping the index consistent with actual proposal state across all code paths.

For the expected scale of this contract, the O(n) scan in a read-only function is a better trade-off than index maintenance complexity on every write. The design notes this as a future optimization when the proposal count justifies it.

### New storage keys

None. This is a pure read over existing `DataKey::Proposal(u64)` entries. No new `DataKey` variants are introduced.

### New errors

None. The function reuses existing errors:

- `Error::NotInitialized` — returned when `DataKey::Config` is absent, consistent with other entry-point functions that require initialization.
- `Error::ProposalNotFound` — propagated from `load_proposal` if a proposal ID in the range `[0, count)` is missing from storage. This should not occur in normal operation but is preserved for correctness.

### Implementation

```rust
pub fn list_proposals_by_status(
    env: Env,
    status: ProposalStatus,
    offset: u64,
    limit: u32,
) -> Result<Vec<Proposal>, Error> {
    // Require initialization, consistent with all other entry-point functions.
    if !env.storage().instance().has(&DataKey::Config) {
        return Err(Error::NotInitialized);
    }

    if limit == 0 {
        return Ok(vec![&env]);
    }
    let limit = limit.min(MAX_PROPOSALS_PAGE_LIMIT);

    let count = env
        .storage()
        .instance()
        .get::<DataKey, u64>(&DataKey::ProposalCount)
        .unwrap_or(0);

    let mut result = Vec::new(&env);
    let mut skipped: u64 = 0;
    let mut id: u64 = 0;

    while id < count {
        let proposal = load_proposal(&env, id)?;
        if proposal.status == status {
            if skipped < offset {
                skipped += 1;
            } else {
                result.push_back(proposal);
                if result.len() as u32 >= limit {
                    break;
                }
            }
        }
        id += 1;
    }

    Ok(result)
}
```

The loop exits as soon as `limit` matching proposals have been collected, so it does not scan proposals beyond the last result once the page is full. When `offset` is large or matching proposals are sparse, the loop may scan the entire ID space before returning fewer than `limit` results — this is expected and matches the O(n) cost described above.

---

## Existing guarantee touched

None. This is a purely additive change. No existing function is modified, no storage layout changes, and no state transitions are introduced. All existing behavior is preserved exactly.

---

## Tests needed

| Test name | What it verifies |
|---|---|
| `test_list_by_status_returns_matching_proposals` | Create 3 proposals, finalize one to `Queued`; call with `Active` → returns the 2 remaining Active proposals and excludes the Queued one. |
| `test_list_by_status_empty_when_none_match` | Call with `Executed` on a contract that has only Active proposals → returns an empty vec, not an error. |
| `test_list_by_status_offset_pagination` | Create 5 Active proposals; call with `offset=2, limit=2` → returns the proposals at logical positions 2 and 3 in the filtered set (IDs are 2 and 3 in a simple sequential setup). |
| `test_list_by_status_limit_capped_at_max` | Call with `limit=1000` → result contains at most `MAX_PROPOSALS_PAGE_LIMIT` (50) proposals; no error is returned. |
| `test_list_by_status_limit_zero_returns_empty` | Call with `limit=0` → returns an empty vec immediately without scanning storage. |
| `test_list_by_status_not_initialized` | Call before `initialize` → returns `Err(Error::NotInitialized)`. |
| `test_list_by_status_no_proposals` | Call on a freshly initialized contract with `ProposalCount = 0` → returns an empty vec for any status. |
| `test_list_by_status_all_statuses` | Create proposals and drive each one to a distinct terminal status (`Active`, `Queued`, `Executed`, `Cancelled`, `Defeated`); call once per status value and verify that each call returns exactly the proposals in that status and no others. |

---

## Out of scope

- **Secondary index on status.** A `DataKey::StatusIndex(ProposalStatus) -> Vec<u64>` index would make filtered reads O(results). This requires index maintenance on every proposal state transition and is deferred until proposal counts grow large enough to make full scans expensive.
- **Filtering by multiple statuses in one call.** Accepting a `Vec<ProposalStatus>` filter would require a more complex interface. Callers that need multiple statuses can issue one call per status or use `get_proposals` and filter client-side.
- **Sorting orders other than ascending ID.** Results are always returned in ascending proposal-ID order, which matches insertion order. Reverse or arbitrary sort order is not supported.
- **Returning proposal IDs only.** Returning `Vec<u64>` instead of `Vec<Proposal>` would reduce per-call data, but it shifts the burden of N follow-up calls to every caller. Full structs are returned unconditionally.
- **Count-only query.** A `count_proposals_by_status` function (returning a `u64`) would also require a full scan and is a separate concern from retrieval. It is not included here.
- **Changes to `get_proposals`.** The existing unfiltered paginator is left untouched.
