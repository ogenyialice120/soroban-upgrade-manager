# Design: Admin-less Fully Governed Mode (Issue #117)

## Summary

This document describes how the Soroban Upgrade Manager would operate
without an admin role, where all actions (cancellation, config changes,
admin transfer) require governance votes. This makes the contract fully
decentralized.

**Status:** Proposed — awaiting maintainer approval before any code is written.

---

## Core Design Decision: Config Flag vs Separate Contract

**Recommended: Config flag on the same contract.**

Rationale:
1. **Single deployment** — one contract can operate in either mode.
2. **Migration path** — a contract initialized with an admin can transition
   to admin-less via a governance vote.
3. **Backward compatible** — existing deployments with admin are unaffected.
4. **Simpler codebase** — one contract to audit, test, and maintain.

A separate contract variant was considered but rejected: it duplicates
code, complicates upgrades, and forces users to migrate to a new address.

## How Config Changes Work Without Admin

### New proposal type: `ConfigChange`

A `ConfigChange` governance proposal contains the new configuration values:

```
voting_period, timelock_delay, quorum, approval_threshold_pct
```

Lifecycle:
1. Any address creates a `ConfigChange` proposal (like `create_proposal`).
2. The proposal goes through the normal vote/finalize cycle.
3. If passed, the config is updated atomically at finalize time.
4. If defeated, no change occurs.

Validation: same config validation as `initialize`/`update_config`
(voting_period >= 1, timelock_delay >= 1, quorum >= 1, threshold 1–100).

Snapshot semantics: `ConfigChange` proposals do **not** snapshot config,
because they *are* the config change. In-flight upgrade proposals keep
their snapshotted rules (unchanged from current behavior).

## How Cancellation Works Without Admin

### New proposal type: `CancelProposal`

A `CancelProposal` governance proposal contains:

```
target_proposal_id: u64
```

Lifecycle:
1. Any address creates a `CancelProposal` targeting an Active or Queued proposal.
2. The proposal goes through the normal vote/finalize cycle.
3. If passed, the target proposal is set to `Cancelled`.
4. If defeated, the target proposal remains unchanged.

Constraints:
- Cannot cancel an already-finalized proposal (Executed, Cancelled, Defeated).
- Cannot cancel the `CancelProposal` itself (prevents self-referential loops).
- The `target_proposal_id` must exist and be Active or Queued.

### Who cancels a bad proposal?

**The governance vote cancels bad proposals.** If a proposal is malicious,
erroneous, or no longer desired, the community creates a `CancelProposal`
and votes on it. If quorum and threshold are met, the target is cancelled.

This shifts the "emergency cancel" power from a single admin to the
governance body, consistent with full decentralization.

## Initialization Modes

Two initialization paths:

### `initialize(admin, voting_period, timelock_delay, quorum, approval_threshold_pct)`
- **Admin mode** (current behavior).
- Admin can cancel proposals, update config, transfer admin.
- `admin_less_mode = false`.

### `initialize_admin_less(voting_period, timelock_delay, quorum, approval_threshold_pct)`
- **Admin-less mode**.
- No admin address is stored.
- `admin_less_mode = true`.
- All actions require governance votes.

Alternatively, a single `initialize` with an `admin_less_mode: bool` flag
and an optional `admin: Address` (required only when false).

## Data Model Changes

### New field on `Proposal`

```
proposal_type: ProposalType
```

Where:

```rust
pub enum ProposalType {
    Upgrade,        // WASM upgrade (current behavior)
    ConfigChange,   // Change governance configuration
    CancelProposal, // Cancel another proposal
}
```

### ConfigChange payload (stored on the Proposal)

```rust
pub struct ConfigChangePayload {
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
}
```

### CancelProposal payload (stored on the Proposal)

```rust
pub struct CancelProposalPayload {
    pub target_proposal_id: u64,
}
```

These can be stored as optional fields on `Proposal`, or as separate
storage entries keyed by proposal ID.

## Admin Function Behavior in Admin-less Mode

| Function | Admin Mode | Admin-less Mode |
|---|---|---|
| `cancel_proposal` | `require_admin` | Governance `CancelProposal` vote required |
| `update_config` | `require_admin` | Governance `ConfigChange` vote required |
| `transfer_admin` | `require_admin` | Not applicable (no admin to transfer) |

In admin-less mode:
- `cancel(env, caller, proposal_id)` → returns `Unauthorized` (no admin).
- `update_config(env, caller, ...)` → returns `Unauthorized` (no admin).
- `transfer_admin(env, caller, new_admin)` → returns `Unauthorized` (no admin).

The admin-less contract still has an `initialize` guard (can't initialize twice).

## Guarantees Touched

This design changes the following existing guarantees:

1. **"Admin can cancel proposals"** — Replaced with:
   - "Governance vote (CancelProposal) can cancel Active or Queued proposals."
   - No single address can unilaterally cancel.

2. **"Admin can update config"** — Replaced with:
   - "Governance vote (ConfigChange) can update governance configuration."
   - No single address can unilaterally change voting rules.

3. **"Admin role can be transferred"** — Removed in admin-less mode.
   - No admin exists to transfer.

All other guarantees (one-address-one-vote, quorum, timelock, proposal
snapshot) remain unchanged.

## Tests Needed

### Initialization
1. `test_initialize_admin_less` — contract initializes with no admin, `admin_less_mode = true`.
2. `test_admin_less_rejects_admin_calls` — `cancel`, `update_config`, `transfer_admin` return `Unauthorized`.

### ConfigChange proposal
3. `test_create_config_change_proposal` — creates ConfigChange proposal with valid values.
4. `test_config_change_proposal_passes` — vote, finalize, verify config updated.
5. `test_config_change_proposal_defeated` — vote NO, finalize, verify config unchanged.
6. `test_config_change_invalid_values` — invalid config values rejected.

### CancelProposal
7. `test_create_cancel_proposal` — creates CancelProposal targeting an Active proposal.
8. `test_cancel_proposal_passes` — vote, finalize, target proposal set to Cancelled.
9. `test_cannot_cancel_finalized_proposal` — CancelProposal targeting Executed/Cancelled/Defeated rejected.
10. `test_cannot_cancel_self` — CancelProposal targeting itself rejected.

### Integration
11. `test_admin_less_full_flow` — propose config change → vote → finalize → verify; then propose cancel → vote → finalize → verify.

### Edge cases
12. `test_admin_less_proposal_snapshot` — upgrade proposals keep snapshotted config even after ConfigChange.
13. `test_admin_less_double_initialize` — calling initialize twice returns AlreadyInitialized.

## Open Questions for Maintainer

1. **Should ConfigChange proposals snapshot the config at creation?** Currently they *are* the config change, so no snapshot is needed. But if config changes mid-vote, should the vote use old or new config? Proposal: vote uses config at creation (snapshot), consistent with Upgrade proposals.

2. **Should there be a timelock on ConfigChange and CancelProposal execution?** Proposal: yes, same snap_timelock_delay applies, giving users time to react.

3. **Can a CancelProposal cancel a ConfigChange proposal?** Proposal: yes, all proposal types are equal.

4. **What happens if the only delegate (in a delegation setup) is the target of a CancelProposal?** Edge case to consider if delegation (#116) is also implemented.

## References

- Issue #117: "Design admin-less fully governed mode"
- Issue #116: "Design voting by delegation with sybil caveats" (orthogonal; delegation can be combined with admin-less mode)
- README.md "Security Model & Limitations" — notes admin is powerful; this design removes that power
- `contracts/upgrade-manager/src/governance.rs` — `require_admin` used by `cancel_proposal`, `update_config`, `transfer_admin`


### New DataKey

```
ConfigChange(u64)  // ConfigChange proposal by ID — can reuse Proposal(u64)
```

We can reuse `DataKey::Proposal(u64)` and add a `proposal_type` field to
`Proposal` to distinguish `Upgrade` vs `ConfigChange` vs `CancelProposal`.

