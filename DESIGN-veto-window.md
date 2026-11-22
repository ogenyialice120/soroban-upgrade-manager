# Design: Optional Veto Window for Proposals

**Issue:** #112  
**Branch:** `design/veto-window-112`  
**Status:** Design proposal — awaiting maintainer approval before code is written.

---

## Problem

Once a proposal passes voting and enters the `Queued` state, the admin can cancel it at any time via `cancel_proposal` — there is no deadline. This is a weaker governance guarantee than it appears: a proposal that has passed a quorum vote with majority approval can be unilaterally blocked by the admin indefinitely, even after the timelock has elapsed and the proposal is executable.

The timelock is designed to give the community time to react before execution. But if the admin can cancel a Queued proposal after the timelock has expired, the timelock's protective function is undermined: the community sees execution is now possible, yet the admin still holds an indefinite veto.

The touched guarantee: **the admin can cancel any Active or Queued proposal at any time.**

---

## Proposed Design

### Principle

The design adds a `veto_enabled: bool` field to `Config`. When `false` (the default), `cancel_proposal` behaves exactly as it does today — no existing deployment is affected. When `true`, the admin's ability to cancel a `Queued` proposal is bounded by a deadline: once `executable_at` is reached, the proposal cannot be cancelled and must proceed to execution or sit indefinitely awaiting the executor. This is a voluntary tightening of admin power that deployers can opt into to strengthen governance trust.

The veto window is the entire timelock delay. The admin may veto at any ledger `< executable_at`; at ledger `>= executable_at` the window has closed. No new timestamp field is required because `executable_at` already encodes the end of the timelock.

### New config field

```rust
pub struct Config {
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
    pub proposal_min_interval: u32,
    /// When true, the admin may only cancel a Queued proposal while
    /// current_ledger < executable_at.  Once executable_at is reached the
    /// cancel path returns VetoWindowClosed.
    /// When false (default), existing cancel behaviour is unchanged.
    pub veto_enabled: bool,
}
```

`veto_enabled = false` is the zero-value default, so existing deployments that upgrade the contract without re-writing their `Config` entry preserve current behaviour.

### New error

```rust
/// Admin attempted to cancel a Queued proposal after the veto window closed
/// (current_ledger >= executable_at and veto_enabled = true).
VetoWindowClosed = 26,
```

Error code 26 is the next available code after the existing 1–25 range.

### Change to `cancel_proposal`

The only code change is a single guard inserted after the terminal-status check and before the status update:

```rust
pub fn cancel_proposal(env: &Env, caller: &Address, proposal_id: u64) -> Result<(), Error> {
    require_admin(env, caller)?;
    let mut proposal = load_proposal(env, proposal_id)?;
    match proposal.status {
        ProposalStatus::Executed | ProposalStatus::Cancelled | ProposalStatus::Defeated => {
            return Err(Error::ProposalFinalized)
        }
        _ => {}
    }

    // NEW: if veto_enabled, Queued proposals can only be cancelled before executable_at.
    if proposal.status == ProposalStatus::Queued {
        let config = load_config(env)?;
        if config.veto_enabled && env.ledger().sequence() >= proposal.executable_at {
            return Err(Error::VetoWindowClosed);
        }
    }

    proposal.status = ProposalStatus::Cancelled;
    save_proposal(env, &proposal);
    env.events().publish(
        (symbol_short!("proposal"), symbol_short!("cancelled")),
        (proposal_id, caller.clone()),
    );
    Ok(())
}
```

The guard fires only when all three conditions hold simultaneously: the proposal is `Queued`, `veto_enabled` is `true`, and `current_ledger >= executable_at`. In all other cases the function is unchanged.

Active proposals are not subject to the veto window check: an Active proposal has not yet passed voting, so cancelling it does not overturn a governance decision.

### No new `Vetoed` status

`Cancelled` is reused. The lifecycle outcome is identical whether the proposal was cancelled during voting, during the timelock, or as an emergency action — the proposal cannot proceed to execution. If an off-chain observer needs to distinguish a veto from an ordinary cancellation, it can compare the `cancel_ledger` (from the event) against `executable_at` stored on the proposal. Adding a `Vetoed` variant would be a breaking change to the `#[contracttype]` enum serialisation and would require updates at every `match` site and all event consumers.

### No new `DataKey`

`veto_enabled` is part of `Config`, which is already stored under `DataKey::Config` in instance storage. No new persistent key is needed.

### Snapshot behaviour

`veto_enabled` is **not** snapshotted into individual proposals. The veto check reads the live config at cancel time, so a deployer who enables or disables veto via `update_config` sees the change apply immediately to all future cancel calls, including those on proposals that were already Queued. This is intentional: `veto_enabled` governs an admin permission, not a voting outcome, and consistent admin rules across all in-flight proposals are easier to reason about than per-proposal snapshots of a permission flag.

### Interaction with `update_config`

`update_config` already replaces the entire `Config` struct. No structural change is required; the caller passes a `Config` with the desired `veto_enabled` value. The new setting takes effect for all subsequent `cancel_proposal` calls.

### Boundary condition

At exactly `executable_at`, `env.ledger().sequence() >= proposal.executable_at` is `true`, so the veto window is closed. This is consistent with `execute`: `execute` requires `current_ledger >= executable_at`, so the ledger at which execution becomes available is simultaneously the first ledger at which veto is unavailable. There is no gap and no overlap.

---

## Existing guarantee touched

**Current guarantee:** The admin can cancel any `Active` or `Queued` proposal at any time.

**New guarantee (when `veto_enabled = true`):** The admin can cancel any `Active` proposal at any time, and any `Queued` proposal only while `current_ledger < executable_at`. Once `executable_at` is reached, a `Queued` proposal is irrevocable by any party short of contract migration.

**When `veto_enabled = false` (default):** The existing guarantee is unchanged.

---

## Tests needed

| Test | What it verifies |
|---|---|
| `test_veto_disabled_admin_can_cancel_queued_at_any_time` | When `veto_enabled = false`, admin can cancel a `Queued` proposal after `executable_at` without error |
| `test_veto_enabled_admin_can_cancel_queued_during_window` | When `veto_enabled = true`, admin can cancel while `current_ledger < executable_at`; proposal status becomes `Cancelled` |
| `test_veto_enabled_admin_cannot_cancel_queued_after_window` | When `veto_enabled = true`, cancel at `current_ledger >= executable_at` returns `VetoWindowClosed` |
| `test_veto_enabled_admin_cannot_cancel_queued_at_exact_boundary` | Cancel at exactly `current_ledger == executable_at` returns `VetoWindowClosed` |
| `test_veto_enabled_admin_can_still_cancel_active` | When `veto_enabled = true`, `Active` proposals can be cancelled at any ledger; veto window does not apply |
| `test_veto_enabled_execute_succeeds_after_window_closes` | When `veto_enabled = true`, `execute()` works normally at and after `executable_at` |
| `test_veto_config_toggleable_via_update_config` | `update_config` can enable and then disable veto; each change applies immediately to subsequent cancel calls |
| `test_veto_no_effect_on_executed_cancelled_defeated` | Attempting to cancel a finalized proposal still returns `ProposalFinalized`, not `VetoWindowClosed` |

---

## Out of scope

- Expanding veto power to non-admin addresses. Doing so would let a veto coalition collude to block any passing proposal, undermining the governance vote. Admin-only veto is consistent with existing `cancel_proposal` permissions.
- A `veto_deadline_pct` field that restricts the veto window to a fraction of the timelock (e.g. first 50%). This adds complexity for limited benefit; the full-timelock window is simpler and already directly encoded in `executable_at`.
- A new `Vetoed` status variant. The reuse of `Cancelled` avoids a breaking schema change while preserving the same lifecycle semantics.
- Snapshotting `veto_enabled` into proposals. The flag governs an admin permission, not a voting outcome, so live-config reads are appropriate.
- Any change to `execute` or `finalize_voting`. Those functions are unaffected.
