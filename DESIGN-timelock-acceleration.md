# Design: Optional Timelock Acceleration for Emergencies (#119)

**Status:** Design — awaiting maintainer approval before any code is written.

---

## Summary

The timelock delay is fixed at finalization time: `executable_at = finalize_ledger + snap_timelock_delay`. In a confirmed emergency (e.g., a critical security vulnerability being actively exploited), waiting for the full delay may be undesirable. This document proposes an optional mechanism that allows shortening `executable_at` on a per-proposal basis, subject to strict authorization and a non-zero minimum review window.

---

## Existing guarantee this change touches

> **The timelock is fixed per proposal.** `snap_timelock_delay` is copied from the config at proposal creation time and written to `executable_at` when `finalize_voting` is called. After finalization, `executable_at` is immutable. No path in the current code allows shortening it.

This guarantee will be **partially relaxed** by the change: `executable_at` becomes mutable after finalization, but only under explicit, auditable conditions.

---

## Who can accelerate

Acceleration requires both of the following:

1. **Admin authorization.** The current contract admin must sign (call `accelerate_timelock` with the admin address, which runs `require_admin`). This bounds the attack surface to the same address that can already cancel proposals.

2. **A second authorized signer from a fixed emergency multi-sig address.** The contract stores an optional `emergency_multisig: Option<Address>` in `Config`. When set, `accelerate_timelock` requires both the admin AND the multi-sig address to authorize (both appear in `auths` for the transaction). When not set (the default), acceleration is disabled entirely — this preserves full backwards compatibility for deployers who do not need the feature.

This two-of-two structure was chosen over alternatives:

| Option | Pros | Cons |
|---|---|---|
| Admin alone | Simple | Admin key compromise → instant bypass of timelock |
| Supermajority re-vote | Fully decentralized | Requires another voting round; slow in a true emergency |
| Multi-sig only (no admin) | Separates roles | Requires a new key management story |
| Admin + emergency multi-sig (chosen) | Bounds blast radius; auditable; optional feature | Requires deployers to manage a second key |

The supermajority re-vote option (a governance vote on the acceleration itself) would take at least `voting_period + 1` ledgers to close — which is exactly the kind of delay an emergency is trying to avoid. It is therefore ruled out for this use case.

---

## Minimum accelerated delay

Acceleration must not reduce `executable_at` below:

```
current_ledger + minimum_emergency_delay
```

where `minimum_emergency_delay` is a new config field (type `u32`, stored in `Config`). It defaults to `0` when the feature is disabled (no `emergency_multisig` set). When the feature is enabled, the deployer must set `minimum_emergency_delay >= 1`.

**Rationale:** The timelock's purpose is to give token holders a window to observe a passed proposal before it executes. Even in an emergency, a non-zero minimum window preserves the core guarantee. Setting it to zero would allow instant execution and completely undermines the timelock model.

**Recommended minimum:** At least 1_000 ledgers (≈ 83 minutes at 5 s/ledger). This is a configuration choice left to the deployer, but the contract enforces `minimum_emergency_delay >= 1` when `emergency_multisig` is `Some(...)`.

---

## Storage and config changes

### `Config` gains two new fields

```rust
pub struct Config {
    // existing fields unchanged
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
    // new fields
    /// Address authorized to co-sign timelock acceleration with the admin.
    /// None means acceleration is disabled.
    pub emergency_multisig: Option<Address>,
    /// Minimum ledgers that must remain between acceleration and execution.
    /// Must be >= 1 when emergency_multisig is Some; ignored (and 0) when None.
    pub minimum_emergency_delay: u32,
}
```

`Option<Address>` serializes cleanly as an XDR optional in Soroban.

### No new `DataKey` variants required

Acceleration modifies `proposal.executable_at` in-place and re-saves the
`DataKey::Proposal(id)` entry — no new storage key is needed.

---

## New entry point: `accelerate_timelock`

```rust
/// Shorten the timelock for a Queued proposal to `new_executable_at`.
///
/// Requires both the admin and the emergency multi-sig address to authorize.
/// `new_executable_at` must be >= current_ledger + minimum_emergency_delay.
/// The proposal must be in the Queued state.
///
/// # Errors
/// - `NotInitialized`         — contract is not yet initialised.
/// - `Unauthorized`           — caller is not the admin.
/// - `AccelerationDisabled`   — emergency_multisig is not configured.
/// - `ProposalNotFound`       — unknown proposal ID.
/// - `NotQueued`              — proposal is not Queued.
/// - `AccelerationTooAggressive` — new_executable_at is below the minimum window.
/// - `AccelerationNotEarlier` — new_executable_at >= current executable_at (not useful).
pub fn accelerate_timelock(
    env: Env,
    caller: Address,
    proposal_id: u64,
    new_executable_at: u32,
) -> Result<(), Error>
```

The function:
1. Calls `require_admin(&env, &caller)?` — admin must sign.
2. Loads `config.emergency_multisig`. If `None`, returns `AccelerationDisabled`.
3. Calls `multisig_address.require_auth()` — multi-sig must also sign.
4. Loads the proposal. Returns `NotQueued` if not `Queued`.
5. Computes `min_allowed = env.ledger().sequence() + config.minimum_emergency_delay`.
6. Returns `AccelerationTooAggressive` if `new_executable_at < min_allowed`.
7. Returns `AccelerationNotEarlier` if `new_executable_at >= proposal.executable_at`.
8. Sets `proposal.executable_at = new_executable_at`, saves the proposal.
9. Emits a `timelock_accelerated` event (see below).

---

## Event

```rust
env.events().publish(
    (symbol_short!("timelock"), symbol_short!("accel")),
    (proposal_id, old_executable_at, new_executable_at, caller),
);
```

The event includes the old and new `executable_at` values so off-chain monitors can detect and audit any acceleration.

---

## New error variants

| Code | Name | Meaning |
|---|---|---|
| 25 | `AccelerationDisabled` | `emergency_multisig` is not configured |
| 26 | `AccelerationTooAggressive` | `new_executable_at` is below the minimum emergency window |
| 27 | `AccelerationNotEarlier` | `new_executable_at` is not earlier than the current `executable_at` |

(Adjust codes if `VoteNotFound = 25` is added by issue #122 first.)

---

## Backwards compatibility

- `Config` gains two optional fields. The `initialize` and `update_config`
  entry points will gain two new parameters: `emergency_multisig: Option<Address>`
  and `minimum_emergency_delay: u32`. Passing `None` / `0` disables the feature
  and preserves existing behavior exactly.
- `snap_timelock_delay` on existing proposals is not affected; only `executable_at`
  can be shortened if acceleration is called.
- **BREAKING when implemented:** `initialize` and `update_config` gain new required
  parameters. Existing callers must be updated.

---

## Files that change

| File | Change |
|---|---|
| `contracts/upgrade-manager/src/types.rs` | `Config` gains `emergency_multisig` and `minimum_emergency_delay`; new error variants |
| `contracts/upgrade-manager/src/governance.rs` | New `accelerate_timelock` function; `validate_config` updated |
| `contracts/upgrade-manager/src/lib.rs` | New `accelerate_timelock` public entry point; `initialize` and `update_config` gain new parameters |

---

## Tests needed

1. **`test_accelerate_timelock_success`** — set `emergency_multisig`; create and finalize a proposal; call `accelerate_timelock` with both admin and multi-sig auths; assert `executable_at` is reduced to `new_executable_at`.

2. **`test_accelerate_timelock_disabled_fails`** — no `emergency_multisig` configured; call `accelerate_timelock`; assert `AccelerationDisabled`.

3. **`test_accelerate_timelock_not_queued_fails`** — call on an Active proposal; assert `NotQueued`.

4. **`test_accelerate_timelock_too_aggressive_fails`** — set `new_executable_at` below `current_ledger + minimum_emergency_delay`; assert `AccelerationTooAggressive`.

5. **`test_accelerate_timelock_not_earlier_fails`** — set `new_executable_at >= proposal.executable_at`; assert `AccelerationNotEarlier`.

6. **`test_accelerate_timelock_requires_admin`** — call with a non-admin address; assert `Unauthorized`.

7. **`test_accelerate_timelock_requires_multisig`** — call with admin but without multi-sig auth; assert the call fails at the host auth level.

8. **`test_accelerate_timelock_emits_event`** — after a successful acceleration, assert the `(timelock, accel)` event is present with the correct old/new values.

9. **`test_accelerate_timelock_then_execute`** — accelerate to a near-future ledger, advance past it, call `execute`; assert the proposal executes successfully.

10. **`test_accelerate_timelock_minimum_delay_zero_disables`** — when `minimum_emergency_delay = 0` and `emergency_multisig = None`, confirm the feature is disabled (default state, preserves current behavior).

---

## Out of scope

- Do not implement until a maintainer approves this design.
- Transitive or multiple acceleration calls on the same proposal (the simplest approach is to allow any number of acceleration calls as long as each one strictly reduces `executable_at` and respects the minimum window; this is covered by check 7 in the function spec above).
- Storing `snap_timelock_delay` versus the accelerated delay in `executable_at` — the proposal already has `snap_timelock_delay` on it for reference; the acceleration is recorded in the event.
