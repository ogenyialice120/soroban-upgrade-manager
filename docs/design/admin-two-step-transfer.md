# Design: Admin Two-Step Transfer (Issue #109)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #109  
**Branch:** `design/admin-two-step-transfer`  
**Type:** Manager contract enhancement  
**Date:** 2026-11-19

---

## 1. Problem

`transfer_admin(caller, new_admin)` is a single-step operation: the current admin immediately transfers the admin role to `new_admin`. If the admin mistypes the address or transfers to an unreachable/lost key, the contract can be permanently locked (no admin to cancel proposals, update config, or transfer again).

A two-step transfer — `propose_admin(new_admin)` then `accept_admin()` by the new admin — prevents accidental transfers to wrong or unreachable addresses, mirroring OpenZeppelin's `Ownable2Step`.

## 2. Existing guarantee touched

> **Admin transfer is instant.** `transfer_admin` requires the current admin to sign and immediately persists the new admin.

This design replaces instant transfer with a pending state: the new admin role is not active until the designated address calls `accept_admin()`.

## 3. Design

### 3.1 Flow

1. Current admin calls `propose_admin(new_admin)` → stores `PendingAdmin = new_admin`, emits `admin_proposed`.
2. New admin calls `accept_admin()` → requires caller == `PendingAdmin`, persists `Admin = new_admin`, clears `PendingAdmin`, emits `admin_transferred` (reusing existing helper).

### 3.2 Storage

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    ...
    /// The address awaiting acceptance of the admin role (two-step transfer).
    PendingAdmin(Address),
}
```

`PendingAdmin` is stored in persistent storage (like `Admin`). Clearing it removes the entry.

### 3.3 Functions

| Function | Visibility | Behavior |
|---|---|---|
| `propose_admin(env, caller, new_admin)` | public | `require_admin(caller)`. Reject if `new_admin == caller` (no-op transfer). Sets `DataKey::PendingAdmin(new_admin)`, overwriting any previous pending admin (re-propose overrides). Emits `admin_proposed(caller, new_admin)`. |
| `accept_admin(env, caller)` | public | `caller.require_auth()`. Error if no `PendingAdmin` set. Error if `caller != PendingAdmin`. Sets `Admin = caller`, removes `PendingAdmin`, emits `admin_transferred(old_admin, caller)`. |
| `get_pending_admin(env) -> Option<Address>` | public read-only | Returns the pending admin if a transfer is in flight, else `None`. |
| `transfer_admin(...)` | public | **Removed** (replaced by the two-step flow). Kept out of scope; see §5. |

### 3.4 Can the old admin cancel?

**Yes.** The old admin can cancel a pending transfer. Options:
- `cancel_propose_admin(env, caller)` — `require_admin(caller)`, removes `PendingAdmin`, emits `admin_proposal_cancelled`. Recommended: include so a mistaken proposal can be undone without the new admin's cooperation.
- Alternatively, a new `propose_admin(...)` simply overwrites the pending admin (re-propose), which implicitly cancels the previous pending transfer.

This design includes both: re-propose overrides, and an explicit cancel for clarity.

### 3.5 What if the new admin never accepts?

The pending transfer remains in flight indefinitely (no auto-expiry in v1). It stays until:
- the new admin calls `accept_admin()`, or
- the current admin calls `cancel_propose_admin()` / re-proposes to a different address.

There is no expiration timestamp in v1 to keep the design minimal; an expiry (`pending_expires_at`) can be added as a follow-up if the team wants stale pending transfers to auto-lapse.

### 3.6 Authorization

- `propose_admin`: current admin must sign (`require_admin`).
- `accept_admin`: the pending admin must sign (`caller.require_auth()` + check `caller == PendingAdmin`).
- `cancel_propose_admin`: current admin must sign (`require_admin`).
- `get_pending_admin`: read-only, no auth.

## 4. Tests needed

1. `propose_admin` by current admin sets `PendingAdmin` and `get_pending_admin` returns it; `get_admin` unchanged.
2. `propose_admin` by non-current admin returns `Unauthorized`.
3. `propose_admin(new_admin == current_admin)` rejected (no-op transfer).
4. Re-proposing to a different address overwrites `PendingAdmin` (previous pending transfer cancelled implicitly).
5. `accept_admin` by the pending admin finalizes: `get_admin` returns new admin, `get_pending_admin` returns `None`, `admin_transferred` event emitted.
6. `accept_admin` by anyone other than the pending admin returns `NotPendingAdmin`/`Unauthorized`.
7. `accept_admin` with no pending transfer returns `NoPendingAdmin`.
8. Old admin can `cancel_propose_admin` to clear `PendingAdmin`; subsequent `accept_admin` by the (never-accepted) new admin returns `NoPendingAdmin`.
9. After a completed two-step transfer, the new admin can call `update_config`/`cancel` (admin actions work), and the old admin can no longer.
10. `get_pending_admin` returns `None` when no transfer is in flight (initial state).

## 5. Out of scope

- Keeping the old single-step `transfer_admin` (replaced by two-step; can be re-added as a convenience wrapper later if desired).
- Auto-expiry of pending transfers (follow-up).
- Timelock on admin transfer itself (the transfer is immediate once accepted; separate from proposal timelock).
- Code: no contract code is written until this design is approved by a maintainer.
