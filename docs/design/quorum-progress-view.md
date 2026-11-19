# Design: Read-Only quorum_progress View (Issue #107)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #107  
**Branch:** `design/quorum-progress-view`  
**Type:** Manager contract read-only view  
**Date:** 2026-11-19

---

## 1. Problem

There is no view that shows how close a proposal is to meeting quorum and the approval threshold. Off-chain tools and UIs must fetch the full `Proposal` struct via `get_proposal` and recompute progress themselves, duplicating the pass/fail math across every consumer.

A dedicated `quorum_progress(proposal_id)` view returns current vote counts, quorum, threshold, and whether the proposal is on track to pass — a single, authoritative source of progress metrics.

## 2. Existing guarantee touched

None directly — this is an additive read-only view. It exposes the same data already available through `get_proposal` (yes_votes, no_votes, snap_quorum, snap_threshold_pct) plus computed progress, so it does not weaken or change any existing on-chain guarantee.

## 3. Design

### 3.1 Function signature

```rust
/// Return the quorum/threshold progress of a proposal.
///
/// Read-only; no authorization required.
///
/// # Parameters
/// - `proposal_id` — ID of the target proposal.
///
/// # Returns
/// A `QuorumProgress` snapshot of the proposal's current voting progress.
///
/// # Errors
/// - `ProposalNotFound` — unknown proposal ID.
pub fn quorum_progress(env: Env, proposal_id: u64) -> Result<QuorumProgress, Error>
```

Return type:

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuorumProgress {
    /// Proposal ID.
    pub proposal_id: u64,
    /// Cumulative YES votes (including delegation weight).
    pub yes_votes: u32,
    /// Cumulative NO votes (including delegation weight).
    pub no_votes: u32,
    /// Total votes cast so far (yes_votes + no_votes).
    pub total_votes: u32,
    /// Minimum total votes required for the proposal to be eligible to pass (snap_quorum).
    pub quorum: u32,
    /// Percentage of YES/(YES+NO) votes required to pass (snap_threshold_pct, 1–100).
    pub approval_threshold_pct: u32,
    /// Additional YES votes still needed to reach quorum (0 if quorum already met).
    pub yes_needed_for_quorum: u32,
    /// Additional YES votes still needed to meet the approval threshold
    /// (0 if threshold already met, given current no_votes).
    pub yes_needed_for_threshold: u32,
    /// Whether the proposal is currently on track to pass:
    /// total_votes >= quorum && yes_votes * 100 >= total_votes * approval_threshold_pct.
    pub passing: bool,
}
```

### 3.2 Computation

The view loads the proposal (no state mutation beyond the existing TTL bump on read) and computes:

```
total_votes = yes_votes + no_votes
passing = total_votes >= quorum && yes_votes * 100 >= total_votes * approval_threshold_pct
yes_needed_for_quorum   = quorum.saturating_sub(total_votes)
yes_needed_for_threshold = smallest x >= 0 such that
    (yes_votes + x) * 100 >= (total_votes + x) * approval_threshold_pct
```

`yes_needed_for_threshold` is the number of additional YES votes (with NO votes held constant) required to flip the proposal to passing. It is `0` when the threshold is already satisfied. (If `approval_threshold_pct == 100`, any NO vote makes the threshold unreachable; in that case return `u32::MAX` or saturate to indicate impossible — documented below.)

### 3.3 Is it just a convenience wrapper?

Yes — largely. `quorum_progress` is a read-only projection over `get_proposal` that packages the already-computed `finalize()` pass/fail math into a single struct. Its real value is:
- A single authoritative implementation of the progress math (consumers don't re-implement it inconsistently).
- A smaller, purpose-built return type instead of exposing the full `Proposal` struct (which includes target, timestamps, proposer, metadata, snapshotted rules, etc.).
- Pre-computed `yes_needed_for_*` fields that tell a UI exactly how many more YES votes are needed.

It does not add new on-chain state, new auth requirements, or new side effects.

### 3.4 Edge cases

- Proposal with zero votes: `total_votes = 0`, `passing = false` (quorum >= 1 so quorum is not met).
- `yes_needed_for_threshold` when `approval_threshold_pct == 100` and `no_votes > 0`: threshold is mathematically unreachable; return `u32::MAX` to signal "impossible" (consumers can treat as "cannot pass").
- All arithmetic uses `checked_add`/`saturating_sub` to avoid overflow.

## 4. Tests needed

1. `quorum_progress` on an unknown proposal returns `ProposalNotFound`.
2. Active proposal with zero votes: `total_votes = 0`, `passing = false`, `yes_needed_for_quorum == quorum`.
3. Active proposal meeting quorum and threshold: `passing = true`, `yes_needed_for_* == 0`.
4. Active proposal meeting quorum but below threshold: `passing = false`, `yes_needed_for_threshold > 0`.
5. Active proposal below quorum: `passing = false`, `yes_needed_for_quorum > 0`.
6. Values match `get_proposal`'s yes_votes/no_votes/snap_quorum/snap_threshold_pct exactly (consistency).
7. `yes_needed_for_threshold` correctly computes additional YES votes needed for a mid-progress proposal.
8. `quorum_progress` is read-only: calling it does not change proposal state or emit events (only TTL bump).
9. Threshold == 100 with a NO vote already cast: `yes_needed_for_threshold` indicates impossible (`u32::MAX`).

## 5. Out of scope

- New storage (reads existing proposal only).
- Authorization (view is open to anyone).
- Changing `get_proposal` or the existing `finalize()` math.
- Code: no contract code is written until this design is approved by a maintainer.
