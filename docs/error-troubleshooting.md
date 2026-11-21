# Error Code Troubleshooting

This page documents every error variant in the `Error` enum
(`contracts/upgrade-manager/src/types.rs`), explains common causes, provides
steps to debug or fix each one, and cross-references the test in `test.rs` that
triggers it.

Error codes are `u32` values returned as contract errors. See also the
[Error Codes table in README.md](../README.md#-error-codes).

---

## Error reference

### Code 1 — `AlreadyInitialized`

**Cause:** `initialize` was called on a contract that already has a stored
`Config` entry.

**Fix:** `initialize` can only be called once per contract deployment. If you
need to change governance parameters after deployment, use `update_config`
instead.

**Triggering test:** `test_initialize_twice_fails`

---

### Code 2 — `Unauthorized`

**Cause:** An admin-only function (`cancel`, `update_config`, `transfer_admin`)
was called by an address that is not the stored admin.

**Fix:** Check the current admin with `get_admin`. Call the function using the
admin address as the source account. If the admin key has been lost, there is
no recovery path — the admin role is permanent until `transfer_admin` is called.

**Triggering test:** `test_auth_cancel_non_admin_fails`

---

### Code 3 — `ProposalNotFound`

**Cause:** A proposal ID passed to `get_proposal`, `vote`, `finalize`,
`execute`, or `cancel` does not exist in storage.

**Fix:** Use a proposal ID returned by a prior `propose_upgrade` call. Proposal
IDs are auto-incremented starting from 0.

**Triggering test:** `test_vote_on_unknown_proposal_fails`

---

### Code 4 — `VotingClosed`

**Cause:** `vote` or `finalize` was called on a proposal whose status is not
`Active`. This includes proposals that are already `Queued`, `Executed`,
`Cancelled`, or `Defeated`.

**Fix:** Only vote on `Active` proposals and only call `finalize` on `Active`
proposals. Check the proposal status with `get_proposal` first.

**Triggering test:** `test_finalize_already_queued_fails`

---

### Code 5 — `AlreadyVoted`

**Cause:** `vote` was called by an address that has already voted on the same
proposal.

**Fix:** Each address may cast exactly one vote per proposal. There is no way to
change a vote once cast.

**Triggering test:** `test_double_vote_fails`

---

### Code 6 — `NotQueued`

**Cause:** `execute` was called on a proposal whose status is not `Queued`.
This includes `Active`, `Defeated`, `Cancelled`, and `Executed` proposals.

**Fix:** A proposal must pass `finalize` and move to `Queued` status before it
can be executed. Check the proposal status with `get_proposal`. If the proposal
is `Active`, wait for the voting period to end and call `finalize` first.

**Triggering test:** `test_execute_fails_on_active_proposal`

---

### Code 7 — `TimelockNotExpired`

**Cause:** `execute` was called before the current ledger sequence has reached
`proposal.executable_at`.

**Fix:** Call `time_until_executable` to check how many ledgers remain. Wait
until it returns `0`, then call `execute`. At ~5 s/ledger, a `timelock_delay`
of `17280` is approximately 24 hours.

**Triggering test:** `test_execute_fails_before_timelock`

---

### Code 8 — `ProposalDefeated` *(never returned)*

**Note:** This error code is defined in the `Error` enum but is **never
returned** by any function. When a proposal fails finalization (quorum not met
or threshold not reached), `finalize()` returns `Ok(ProposalStatus::Defeated)`
as a success value rather than this error. This matches the README error table
and is verified by `test_finalize_defeated_without_quorum`.

---

### Code 9 — `DescriptionTooLong`

**Cause:** `propose_upgrade` was called with a description string longer than
256 bytes.

**Fix:** Shorten the description to 256 bytes or fewer. Use an off-chain storage
system (e.g., IPFS) for long explanations and include only a URI or hash in the
description.

**Triggering test:** `test_description_too_long`

---

### Code 10 — `InvalidThreshold`

**Cause:** `initialize` or `update_config` was called with
`approval_threshold_pct` set to `0` or greater than `100`.

**Fix:** Use a value in the range `1`–`100`. A value of `51` represents a simple
majority.

**Triggering test:** `test_initialize_zero_threshold_fails`, `test_initialize_threshold_101_fails`

---

### Code 11 — `InvalidTimelockDelay`

**Cause:** `initialize` or `update_config` was called with `timelock_delay`
set to `0`.

**Fix:** Use a value of at least `1`. A `timelock_delay` of `0` would allow
immediate execution after finalization, defeating the purpose of the timelock.

**Triggering test:** `test_initialize_zero_delay_fails`

---

### Code 12 — `InvalidQuorum`

**Cause:** `initialize` or `update_config` was called with `quorum` set to `0`.

**Fix:** Use a value of at least `1`. A quorum of `0` would allow a proposal to
pass with no votes at all.

**Triggering test:** `test_initialize_zero_quorum_fails`

---

### Code 13 — `ProposalFinalized`

**Cause:** `cancel` was called on a proposal that is already in a terminal
state: `Executed`, `Cancelled`, or `Defeated`.

**Fix:** Only call `cancel` on `Active` or `Queued` proposals. Terminal
proposals cannot be changed.

**Triggering test:** `test_cancel_defeated_proposal_fails`, `test_cancel_executed_proposal_fails`, `test_cancel_already_cancelled_fails`

---

### Code 14 — `NotInitialized`

**Cause:** Any contract function was called before `initialize`. The contract
has no stored `Config` or `Admin` entry.

**Fix:** Call `initialize` with a valid admin address and governance parameters
before calling any other function.

**Triggering test:** `test_propose_not_initialized_fails`, `test_get_config_not_initialized_fails`, `test_get_admin_not_initialized_fails`

---

### Code 15 — `Overflow`

**Cause:** An arithmetic overflow occurred in ledger sequence, vote count, or
proposal ID computation. This should not occur in normal operation.

**Debug:** If this error appears, the ledger sequence number or vote count has
wrapped past the `u32` or `u64` maximum. This is a theoretical edge case that
would require extremely large ledger sequence numbers or vote counts.

**Triggering test:** N/A — not reachable in normal operation.

---

### Code 16 — `VotingPeriodEnded`

**Cause:** `vote` was called after the voting period has closed
(`ledger.sequence() > proposal.voting_ends_at`).

**Fix:** Vote before the voting period ends. Check `proposal.voting_ends_at`
with `get_proposal` and compare to the current ledger sequence. At ~5 s/ledger,
a `voting_period` of `17280` is approximately 24 hours from proposal creation.

**Triggering test:** `test_vote_after_voting_period_ends_fails`

---

### Code 17 — `VotingStillActive`

**Cause:** `finalize` was called before the voting period has ended
(`ledger.sequence() <= proposal.voting_ends_at`).

**Fix:** Wait until the ledger sequence is strictly greater than
`proposal.voting_ends_at`, then call `finalize`. Use `get_proposal` to check
`voting_ends_at`.

**Triggering test:** `test_finalize_before_voting_ends_fails`, `test_finalize_at_voting_ends_at_fails`

---

### Code 18 — `EmptyDescription`

**Cause:** `propose_upgrade` was called with an empty description string.

**Fix:** Provide a non-empty description of at least 1 byte and at most 256
bytes.

**Triggering test:** `test_description_empty_fails`

---

### Code 19 — `InvalidVotingPeriod`

**Cause:** `initialize` or `update_config` was called with `voting_period` set
to `0`.

**Fix:** Use a value of at least `1`. A `voting_period` of `0` would mean no
voting window exists.

**Triggering test:** `test_initialize_zero_voting_period_fails`

---

### Code 20 — `EmptyTitle`

**Cause:** `propose_upgrade` was called with an empty title string.

**Fix:** Provide a non-empty title of at least 1 byte and at most 128 bytes.

**Triggering test:** `test_propose_empty_title`

---

### Code 21 — `TitleTooLong`

**Cause:** `propose_upgrade` was called with a title string longer than 128
bytes.

**Fix:** Shorten the title to 128 bytes or fewer.

**Triggering test:** `test_propose_title_too_long`

---

### Code 22 — `HasDelegated`

**Cause:** `vote` was called by an address that has delegated its vote to
another address. A delegated voter cannot vote directly — only the delegate can
vote on their behalf.

**Fix:** Either revoke the delegation with `revoke_delegation` before voting, or
have the delegate cast the vote.

**Triggering test:** `test_delegated_voter_cannot_vote_directly`

---

### Code 23 — `CannotDelegateToSelf`

**Cause:** `delegate` was called with the same address for both the voter and
the delegate.

**Fix:** Delegate to a different address.

**Triggering test:** `test_cannot_delegate_to_self`

---

### Code 24 — `NotDelegating`

**Cause:** `revoke_delegation` was called by an address that has not delegated
its vote.

**Fix:** Only call `revoke_delegation` if you have previously called `delegate`.
Check whether a delegation exists before revoking.

**Triggering test:** `test_revoke_not_delegating`

---

## Quick reference table

| Code | Name | Trigger | Fix |
|------|------|---------|-----|
| 1 | `AlreadyInitialized` | `initialize` called twice | Use `update_config` to change params after deployment |
| 2 | `Unauthorized` | Admin function called by non-admin | Use the admin address |
| 3 | `ProposalNotFound` | Unknown proposal ID | Use an ID returned by `propose_upgrade` |
| 4 | `VotingClosed` | `vote`/`finalize` on non-Active proposal | Check proposal status first |
| 5 | `AlreadyVoted` | Same address votes twice | Each address votes once per proposal |
| 6 | `NotQueued` | `execute` on non-Queued proposal | Call `finalize` first |
| 7 | `TimelockNotExpired` | `execute` before `executable_at` | Wait until `time_until_executable` returns 0 |
| 8 | `ProposalDefeated` | *(never returned)* | N/A |
| 9 | `DescriptionTooLong` | Description > 256 bytes | Shorten description |
| 10 | `InvalidThreshold` | `approval_threshold_pct` = 0 or > 100 | Use 1–100 |
| 11 | `InvalidTimelockDelay` | `timelock_delay` = 0 | Use ≥ 1 |
| 12 | `InvalidQuorum` | `quorum` = 0 | Use ≥ 1 |
| 13 | `ProposalFinalized` | `cancel` on terminal proposal | Only cancel Active or Queued |
| 14 | `NotInitialized` | Function called before `initialize` | Call `initialize` first |
| 15 | `Overflow` | Arithmetic overflow | Should not occur in normal operation |
| 16 | `VotingPeriodEnded` | `vote` after `voting_ends_at` | Vote before the voting period ends |
| 17 | `VotingStillActive` | `finalize` before `voting_ends_at` elapses | Wait for voting period to end |
| 18 | `EmptyDescription` | Empty description in `propose_upgrade` | Provide a non-empty description |
| 19 | `InvalidVotingPeriod` | `voting_period` = 0 | Use ≥ 1 |
| 20 | `EmptyTitle` | Empty title in `propose_upgrade` | Provide a non-empty title |
| 21 | `TitleTooLong` | Title > 128 bytes | Shorten title |
| 22 | `HasDelegated` | Delegated voter tries to vote directly | Revoke delegation first, or have delegate vote |
| 23 | `CannotDelegateToSelf` | `delegate` called with self as delegate | Delegate to a different address |
| 24 | `NotDelegating` | `revoke_delegation` with no active delegation | Only revoke if you have delegated |
