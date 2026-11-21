# Design: Optional proposal expiry mechanism

**Issue:** #104  
**Status:** Proposed — awaiting maintainer approval  
**Branch:** `feature/proposal-expiry-mechanism`  
**Type:** Configuration + behavior change (no new status variant)

---

## 1. Problem statement

Proposals currently never expire. An `Active` proposal can be finalized at any
time after the voting period ends, even years later. The README "Security Model
& Limitations" lists "No expiry" as a limitation:

> **No expiry:** Proposals do not expire. An Active proposal can be finalized
> at any time after the voting period ends, even years later.

An optional expiry mechanism would allow proposals to automatically become
`Defeated` after a configurable window, preventing abandoned proposals from
lingering indefinitely.

---

## 2. Proposed design

### 2.1 Config parameter: `proposal_expiry`

Add an optional `proposal_expiry: Option<u32>` field to `Config` (in ledgers,
set at `initialize` and changeable via `update_config`):

- `None` — no expiry (current behavior, backward compatible). Default for
  existing deployments.
- `Some(n)` — proposal expires `n` ledgers after the expiry anchor (see §2.2).

```rust
pub struct Config {
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
    /// Optional proposal expiry in ledgers. None = no expiry (backward
    /// compatible). Some(n) = proposals expire n ledgers after the expiry
    /// anchor.
    pub proposal_expiry: Option<u32>,
}
```

**Validation:** `proposal_expiry` has no minimum/maximum constraint beyond
being `Option<u32>`. If `Some(0)`, expiry fires immediately (effectively all
proposals are expired at creation — a degenerate but valid configuration).

`validate_config` is unchanged; `proposal_expiry` is optional and needs no
range check.

### 2.2 Expiry anchor: from creation (`created_at`)

**Decision: expiry applies from creation (`created_at + proposal_expiry`), not
from `voting_ends_at`.**

Rationale:

- The security concern is an `Active` proposal lingering **indefinitely**.
  Expiry from creation gives a predictable, absolute deadline regardless of
  when (or whether) anyone calls `finalize()`.
- Expiry from `voting_ends_at` would give voters a fixed window after voting
  closes, but a proposal could still sit `Active` for `voting_period` ledgers
  before the expiry clock even starts. That doesn't solve the "lingers
  indefinitely" problem as directly.
- From creation is simpler to implement and reason about: `expires_at =
  created_at + proposal_expiry`. The check in `finalize` compares
The `Proposal` struct already stores `created_at`, so no new storage field is
needed to compute expiry. We add one persisted field `expires_at: Option<u32>`
set at proposal creation (or compute it on the fly from `created_at +
proposal_expiry` — but since `proposal_expiry` can change via `update_config`,
computing on the fly could change the meaning of in-flight proposals. To
preserve snapshot isolation, we **store** `expires_at` at creation time).

Add to `Proposal`:

```rust
/// Ledger at which the proposal expires (created_at + proposal_expiry).
/// None if no expiry was configured at creation time.
pub expires_at: Option<u32>,
```

### 2.3 How `finalize` handles an expired proposal

In `finalize_voting`, after the existing checks (status is Active, voting
period has ended), add an expiry check:

```rust
// If the proposal has expired, defeat it without counting votes.
if let Some(expires_at) = proposal.expires_at {
    if current_ledger > expires_at {
        proposal.status = ProposalStatus::Defeated;
        save_proposal(env, &proposal);
        env.events().publish(
            (symbol_short!("proposal"), symbol_short!("finalized")),
            (proposal_id, proposal.status.clone()),
        );
        return Ok(ProposalStatus::Defeated);
    }
}
```

**Behavior:**
- If `expires_at` is `None` (no expiry configured), proceed as today.
- If `current_ledger <= expires_at`, proceed as today (count votes, pass/fail).
- If `current_ledger > expires_at`, the proposal is expired → move directly to
  `Defeated` without counting votes. No `executable_at` is set.

### 2.4 New `Expired` status vs. `Defeated`

**Decision: reuse `Defeated` — do NOT add a new `Expired` status.**

Rationale:

- The outcome is identical from a lifecycle perspective: the proposal did not
  pass and cannot be executed. `Defeated` already means "voting closed without
  passing."
- Adding a new `ProposalStatus::Expired` variant would require changes to:
  - `ProposalStatus` enum (and all `match` sites that must handle it
    exhaustively — `cancel_proposal`, `finalize_voting`, `execute`, tests)
  - Event types and any downstream consumers
  - The "ProposalFinalized" error check (which treats `Defeated` as finalized)
  - Serialization compatibility (`#[contracttype]` enums are stored on-chain)
- `Defeated` is semantically accurate: an expired proposal is defeated by
  time, not by votes. The distinction (why it was defeated) can be inferred
  from `expires_at` and `current_ledger` if needed off-chain.

### 2.5 Existing guarantee this touches

This changes the guarantee documented in the README:

> **No expiry:** Proposals do not expire. An Active proposal can be finalized
> at any time after the voting period ends, even years later.

With this change, an `Active` proposal **cannot** be finalized after its expiry
window — `finalize()` will defeat it instead. The guarantee "anyone can
finalize at any time after voting ends" becomes "anyone can finalize at any
time after voting ends, **until the expiry window closes**."

This is a deliberate, opt-in behavior change (controlled by `proposal_expiry`
being `Some`). Deployments that set `None` retain the current behavior exactly.


---

## 3. Tests needed

| Test name | What it verifies |
|-----------|-----------------|
| `test_finalize_expired_proposal_is_defeated` | Proposal past its `expires_at`, `finalize()` returns `Defeated` without counting votes. |
| `test_finalize_within_expiry_still_works` | Proposal within its expiry window, `finalize()` works normally (passes or fails based on votes). |
| `test_expiry_none_no_effect` | Config with `proposal_expiry=None`, proposal never expires — `finalize()` works as today even long after creation. |
| `test_expires_at_snapshotted_at_creation` | After `update_config` changes `proposal_expiry`, an existing proposal's `expires_at` is unchanged (snapshot isolation). |
| `test_expires_at_zero_expires_immediately` | Config with `proposal_expiry=Some(0)`, proposal expires immediately — `finalize()` defeats it. |

---

## 4. Files affected (when implemented)

- `contracts/upgrade-manager/src/types.rs` — add `proposal_expiry: Option<u32>`
  to `Config`, add `expires_at: Option<u32>` to `Proposal`.
- `contracts/upgrade-manager/src/governance.rs` — expiry check in
  `finalize_voting`; set `expires_at` in `create_proposal`.
- `contracts/upgrade-manager/src/lib.rs` — `initialize` and `update_config`
  accept/pass `proposal_expiry`; `validate_config` unchanged.
- `README.md` — update Config Reference table and Security Model &
  Limitations (remove/reword "No expiry" limitation).
- `contracts/upgrade-manager/src/test.rs` — add the five tests listed above.
- `docs/claims-audit.md` — update/add claim rows for expiry behavior.
- `_CHANGELOG.md` — append a dated entry per CONTRIBUTING.md.

---

## 5. Out of scope

- No new `ProposalStatus::Expired` variant (reuse `Defeated`).
- No changes to `validate_config` (expiry is `Option<u32>`, no range check).
- No changes to `vote()` — votes are still accepted during the voting period
  regardless of expiry (expiry only matters at `finalize` time).
- No changes to `cancel()` — admin can still cancel Active proposals before
  expiry.
- No implementation code in this PR — this document is the design proposal
  only. Implementation proceeds only after maintainer approval.

  `current_ledger > expires_at`.
