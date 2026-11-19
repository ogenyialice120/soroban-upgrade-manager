# Design: Manager-Side WASM Hash Allowlist for Proposals (Issue #111)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #111  
**Branch:** `design/wasm-hash-allowlist-manager`  
**Type:** Manager contract enhancement  
**Date:** 2026-11-19  
**Note:** Distinct from the target-side allowlist design in `docs/design/wasm-hash-allowlist.md` (issue #128). This design governs which WASM hashes may be *proposed* in the upgrade manager.

---

## 1. Problem

`create_proposal` (exposed as `propose_upgrade`) accepts any `BytesN<32>` as `new_wasm_hash` with no validation beyond a non-empty description. A proposer can submit a proposal pointing at an unreviewed, malicious, or nonexistent WASM hash. The hash is only applied later at `execute()`, so a bad hash can sit in a proposal for the full voting + timelock window before being discovered.

An allowlist lets the admin pre-approve which WASM hashes may be proposed, preventing proposals for unreviewed code.

## 2. Existing guarantee touched

> **Any hash can be proposed.** `create_proposal` accepts any `BytesN<32>` as `new_wasm_hash`.

This design tightens that guarantee: a hash may only be proposed if it is present on the admin-managed allowlist (when the allowlist is enabled).

## 3. Design

### 3.1 Scope: restrict `propose_upgrade`, not `execute`

The allowlist is checked at proposal creation (`create_proposal` / `propose_upgrade`). It is **not** re-checked at `execute()`, because the hash was already snapshotted into the proposal at creation and re-checking would allow an admin to invalidate an in-flight proposal retroactively. The allowlist governs what may enter the system, not what may leave it.

### 3.2 Storage

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    ...
    /// Whether the WASM hash allowlist is enforced (true = enabled).
    AllowlistEnabled,
    /// An approved WASM hash that may be referenced in `propose_upgrade`.
    AllowedWasm(BytesN<32>),
}
```

- `AllowlistEnabled` — a single bool stored in instance or persistent storage. Default `false` (disabled) for backward compatibility; existing contracts are unaffected unless an admin enables it.
- `AllowedWasm(hash)` — presence (`= true`) means the hash is allowlisted. Storing per-hash entries gives O(1) lookup and cheap add/remove without rewriting an entire list. (A `Vec<BytesN<32>>` was considered but rejected: membership checks would be O(N) and removing requires rewriting the whole vector.)

A `soroban_sdk::Map` was also considered; the per-key `DataKey` entry matches the existing contract style and avoids importing `Map`.

### 3.3 Functions

| Function | Visibility | Behavior |
|---|---|---|
| `set_allowlist_enabled(env, caller, enabled)` | public | Admin-only; toggles `DataKey::AllowlistEnabled`. |
| `add_allowed_wasm(env, caller, hash)` | public | Admin-only; inserts `DataKey::AllowedWasm(hash) = true`. |
| `remove_allowed_wasm(env, caller, hash)` | public | Admin-only; removes `DataKey::AllowedWasm(hash)`. |
| `is_allowed_wasm(env, hash) -> bool` | public read-only | Returns whether `hash` is allowlisted (and the allowlist is enabled). |
| `propose_upgrade(...)` | public | Existing, plus: if allowlist enabled, assert `new_wasm_hash` is allowlisted before saving the proposal. |

### 3.4 Admin-only + optional

- **Admin only:** `add_allowed_wasm`, `remove_allowed_wasm`, and `set_allowlist_enabled` require the current admin (`require_admin`).
- **Optional / always-on:** The allowlist is **optional**, gated by `AllowlistEnabled`, defaulting to `false` (disabled). This preserves backward compatibility — existing deployments keep accepting any hash until an admin explicitly enables the allowlist. (An always-on allowlist would break existing proposals and was rejected for v1.)

When disabled, `propose_upgrade` behaves exactly as today (any hash accepted). When enabled, any `new_wasm_hash` not in the allowlist is rejected with a new `Error::WasmHashNotAllowlisted` before any state is written.

### 3.5 Interaction with voting

None. Voting operates on proposal IDs and yes/no counts; it never inspects the WASM hash. The allowlist only gates proposal creation. A proposal created while the allowlist was enabled and the hash was allowlisted remains valid even if the admin later removes the hash or disables the allowlist (no retroactive invalidation).

### 3.6 New error

```rust
/// The new_wasm_hash is not on the admin allowlist.
WasmHashNotAllowlisted = 25,
```

## 4. Tests needed

1. Allowlist disabled (default): `propose_upgrade` with any hash succeeds (backward compatibility).
2. Allowlist enabled + allowlisted hash: `propose_upgrade` succeeds.
3. Allowlist enabled + non-allowlisted hash: `propose_upgrade` rejected with `WasmHashNotAllowlisted`, no proposal created.
4. `add_allowed_wasm` by admin then `propose_upgrade` succeeds (dynamic allowlisting).
5. `remove_allowed_wasm` by admin then `propose_upgrade` rejected (dynamic revocation).
6. Non-admin cannot call `add_allowed_wasm`/`remove_allowed_wasm`/`set_allowlist_enabled` (`Unauthorized`).
7. `is_allowed_wasm` read-only membership accurate before/after add/remove and enable/disable.
8. Enabling/disabling the allowlist does not affect already-created proposals (no retroactive invalidation).
9. `execute()` is not gated by the allowlist (hash already snapshotted in proposal).

## 5. Out of scope

- Per-proposal allowlists (allowlist is global).
- Re-checking the allowlist at `execute()`.
- Always-on allowlist (optional by default; revisit as a follow-up).
- A separate allowlist admin address (reuse existing admin).
- Code: no contract code is written until this design is approved by a maintainer.
