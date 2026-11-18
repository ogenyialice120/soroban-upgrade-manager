# Design: Target-Side WASM Hash Allowlist (Issue #128)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #128  
**Branch:** `design/wasm-hash-allowlist`  
**Type:** Target-side enhancement (no manager change)  
**Date:** 2026-11-18

---

## 1. Problem

Each target contract's `upgrade()` currently trusts the upgrade manager unconditionally:

```rust
manager.require_auth();
env.deployer().update_current_contract_wasm(new_wasm_hash);
```

There is **no validation of `new_wasm_hash`** on the target side. If the manager account is compromised (or behaves maliciously), it can cause the target to upgrade to arbitrary, potentially malicious WASM. The manager itself does not validate WASM compatibility (a documented limitation).

This issue asks for a **defense-in-depth** layer: the target maintains its own allowlist of approved WASM hashes and rejects any hash not on the list before calling `update_current_contract_wasm`.

---

## 2. Existing guarantee touched

> The target trusts the manager unconditionally.

`upgrade()` authenticates the manager but performs no further check on the new WASM hash. This design tightens that guarantee: the manager must be authenticated **and** the target hash must be pre-approved.

---

## 3. Design

### 3.1 Storage

Add a persistent allowlist keyed by WASM hash:

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    ...
    /// Approved WASM hash that may be installed via `upgrade()`.
    AllowedWasm(BytesN<32>),
}
```

Each entry maps an approved `BytesN<32>` hash to `true` (presence = allowed). Storing per-hash entries gives O(1) lookup and cheap add/remove without rewriting an entire list.

### 3.2 Functions

| Function | Visibility | Behavior |
|---|---|---|
| `__constructor(env, manager)` | public | Stores manager at deploy time (unchanged). |
| `is_allowed_wasm(env, hash) -> bool` | public read-only | Returns whether `hash` is in the allowlist. |
| `add_allowed_wasm(env, hash)` | public | Admin-only; inserts `DataKey::AllowedWasm(hash) = true`. |
| `remove_allowed_wasm(env, hash)` | public | Admin-only; removes `DataKey::AllowedWasm(hash)`. |
| `upgrade(env, new_wasm_hash)` | public | `manager.require_auth()` → assert `is_allowed_wasm(new_wasm_hash)` → `update_current_contract_wasm(new_wasm_hash)`. |

### 3.3 Allowlist administration

**Recommended:** reuse the existing stored `manager` as the allowlist admin. This keeps the design minimal — no new admin address, no governance dependency — and matches the existing trust boundary (the manager already controls upgrades).

**Alternatives (deferred):** a separate `allowlist_admin` address, or governance-controlled allowlist changes proposed through the upgrade-manager. These add decentralization but increase complexity and are out of scope for the initial design.

### 3.4 Upgrade flow

```rust
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
    let manager: Address = ... // load stored manager
    manager.require_auth();              // existing guarantee
    assert!(is_allowed_wasm(&env, &new_wasm_hash), "wasm hash not allowlisted");
    env.deployer().update_current_contract_wasm(new_wasm_hash);
}
```

---

## 4. Tests needed

1. `upgrade` with an allowlisted hash succeeds (WASM replaced).
2. `upgrade` with a non-allowlisted hash is rejected (reverts before `update_current_contract_wasm`).
3. `add_allowed_wasm` then `upgrade` succeeds (dynamic allowlisting works).
4. `remove_allowed_wasm` then `upgrade` is rejected (dynamic revocation works).
5. Non-admin address cannot call `add_allowed_wasm`/`remove_allowed_wasm` (auth enforced).
6. `is_allowed_wasm` read-only membership is accurate before/after add/remove.
7. Existing manager-auth-rejection test still passes (manager auth still enforced).

---

## 5. Out of scope

- Manager-side WASM compatibility validation (documented manager limitation).
- Governance-controlled allowlist changes.
- A separate allowlist admin address (revisit as a follow-up).

No code should be written until a maintainer approves this design.
