# Design: Batch Vote Function (Issue #113)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #113  
**Branch:** `design/batch-vote`  
**Type:** Manager contract enhancement  
**Date:** 2026-11-19

---

## 1. Problem

Voting on multiple proposals requires one transaction per proposal (`vote(voter, proposal_id, approve)`). Each transaction:
- Calls `voter.require_auth()` once,
- Reads/writes the proposal entry and the `Vote(proposal_id, voter)` entry,
- Pays full transaction fee overhead.

A voter who wants to vote on several proposals must broadcast and pay for N separate transactions. A single `batch_vote(voter, proposal_ids, approves)` call would let a voter cast votes on multiple proposals in one transaction, saving gas.

## 2. Existing guarantee touched

> **One vote per call.** The public `vote()` entrypoint processes exactly one proposal per invocation, authenticates the caller once, and records exactly one `Vote` entry.

Batch voting relaxes this guarantee: one call records N votes (one per proposal) after a single `require_auth`.

## 3. Design

### 3.1 Function signature

```rust
/// Cast YES/NO votes on multiple proposals in a single transaction.
///
/// The caller (`voter`) must sign once. All votes are cast as `voter`.
///
/// # Parameters
/// - `voter`        — Address casting every vote (must sign; `require_auth` is called once).
/// - `proposal_ids` — IDs of the target proposals.
/// - `approves`     — `true` for YES, `false` for NO, parallel to `proposal_ids`.
///
/// # Returns
/// A `Vec<VoteOutcome>` parallel to the input vectors, one entry per proposal:
///   - `VoteOutcome::Cast(id)`   — vote was recorded.
///   - `VoteOutcome::Skipped(id, reason)` — vote was not recorded (see §3.4).
///
/// # Errors
/// - `NotInitialized` — contract not initialised (systemic, fails whole batch).
/// - `InvalidInput`   — `proposal_ids.len() != approves.len()` (validated up front).
pub fn batch_vote(
    env: Env,
    voter: Address,
    proposal_ids: Vec<u64>,
    approves: Vec<bool>,
) -> Result<Vec<VoteOutcome>, Error>
```

Return type:

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteOutcome {
    /// Vote was recorded for `proposal_id`.
    Cast(u64),
    /// Vote was skipped for `proposal_id` for the given reason.
    Skipped(u64, SkipReason),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkipReason {
    /// Proposal is not Active (already finalized).
    VotingClosed,
    /// Voting period has ended.
    VotingPeriodEnded,
    /// Voter has delegated and cannot vote directly.
    HasDelegated,
    /// Voter already voted on this proposal.
    AlreadyVoted,
    /// Proposal ID is unknown.
    ProposalNotFound,
}
```

### 3.2 Validation

1. **Equal-length vectors** — `proposal_ids.len() == approves.len()` is validated first; mismatch returns `Error::InvalidInput` and fails the whole batch (no partial state).
2. **Same voter for all** — a single `voter` parameter means every vote in the batch is cast by the same address; there is no per-vote voter list to validate.
3. **Non-empty** — an empty `proposal_ids` (with matching empty `approves`) is a valid no-op returning an empty vector.

### 3.3 Authorization

`voter.require_auth()` is called **once**, before iterating proposals. Per the Soroban host, `require_auth` is charged once per address per transaction, so batching amortizes the auth cost across all votes. No per-vote `require_auth`.

### 3.4 Partial failure handling — skip failures (recommended)

**Decision:** skip failures rather than all-or-nothing. Rationale: the purpose of batch voting is gas efficiency. If one proposal in the batch is already voted on or already finalized, the remaining votes should still be recorded instead of reverting the entire transaction and wasting all gas.

Per-proposal recoverable errors (`AlreadyVoted`, `VotingClosed`, `VotingPeriodEnded`, `HasDelegated`, `ProposalNotFound`) are recorded as `VoteOutcome::Skipped(id, reason)` and iteration continues.

Systemic errors (`NotInitialized`) and the up-front length-mismatch validation fail the whole batch immediately.

**Alternative (all-or-nothing):** any per-vote failure reverts the entire batch. Simpler to audit but wastes gas when a single vote is ineligible. Documented as a future option; not recommended for v1 because it contradicts the efficiency goal.

### 3.5 Events

Each `Cast` outcome emits the existing `vote_cast` event (proposal_id, voter, approve) so off-chain indexers can reconstruct per-vote history from a single transaction. Skipped votes emit no event (no state change).

## 4. Tests needed

1. `proposal_ids.len() != approves.len()` returns `Error::InvalidInput` (whole batch fails, no state change).
2. Empty vectors return empty `Vec` (valid no-op).
3. All valid proposals: every vote recorded (`Cast`), `yes_votes`/`no_votes` updated, `Vote` entries created, single `require_auth`.
4. Mixed batch: one `AlreadyVoted` + two fresh votes → two `Cast`, one `Skipped(AlreadyVoted)`, fresh votes recorded, already-voted proposal unchanged.
5. Mixed batch: one finalized/non-Active proposal → that entry `Skipped(VotingClosed)`, others cast.
6. Voter who has delegated → all entries `Skipped(HasDelegated)` (delegation is global).
7. Unknown proposal ID in batch → that entry `Skipped(ProposalNotFound)`, others cast.
8. Non-voter caller (no auth) → transaction reverts (auth enforced once).
9. Events: exactly one `vote_cast` event per `Cast` outcome, none for `Skipped`.
10. Gas: batch of N votes costs less than N individual `vote` transactions (auth amortized).

## 5. Out of scope

- Per-vote different voters (a single `voter` signs for the whole batch).
- Vote weight/delegation counting changes (uses existing `cast_vote` weight logic).
- All-or-nothing semantics (deferred alternative).
- Code: no contract code is written until this design is approved by a maintainer.
