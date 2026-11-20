# Example target contract: design and security model

This document explains the design decisions behind the example-target contract,
a reference implementation used to demonstrate and test the Soroban Upgrade
Manager. It is intended for contributors building their own target contracts.

The contract source is `contracts/example-target/src/lib.rs`. Every claim here
matches that code.

## Overview

The example-target is a minimal, upgradable contract. It stores the address of
the upgrade-manager that is allowed to call `upgrade()`, exposes a read-only
`version()`, and lets the authenticated manager replace the contract's WASM.

```
contracts/example-target/src/lib.rs
├── __constructor(manager)  — deploy-time, stores manager, no auth
├── version()               — 1 or 2 depending on the `v2` feature flag
├── get_manager()           — read-only, returns stored manager
└── upgrade(new_wasm_hash)  — manager.require_auth(), then update_current_contract_wasm
```

---

## Why the constructor does not call `require_auth`

```rust
pub fn __constructor(env: Env, manager: Address) {
    // The manager is stored at deploy time. The manager is a contract address
    // and cannot sign, so we intentionally do NOT call require_auth() here.
    env.storage()
        .instance()
        .set(&soroban_sdk::symbol_short!("manager"), &manager);
}
```

The constructor intentionally does **not** call `manager.require_auth()`.

**Reason:** The manager is a **contract address**, and a contract address
cannot sign a transaction. `require_auth()` requires the caller to produce a
valid signature, which a contract cannot do. Calling `require_auth()` on the
manager in the constructor would therefore always fail.

---

## Why `upgrade()` calls `require_auth`

```rust
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
    let manager: Address = env
        .storage()
        .instance()
        .get(&soroban_sdk::symbol_short!("manager"))
        .expect("manager not set");
    // Only the stored manager may trigger an upgrade.
    manager.require_auth();
    env.deployer().update_current_contract_wasm(new_wasm_hash);
}
```

`upgrade()` **does** call `manager.require_auth()`.

**Reason:** `upgrade()` performs a state-changing cross-contract operation —
replacing the contract's own WASM. Only the stored manager address (set at
deploy time) is authorized to do this. `manager.require_auth()` enforces that
the invocation is authenticated by the manager contract, so no other address
can replace the target's WASM.

If `require_auth()` fails (the caller is not the manager), the function panics
and the transaction reverts.


---

## How the `v2` feature flag works

```rust
pub fn version(_env: Env) -> u32 {
    #[cfg(feature = "v2")]
    return 2;
    #[cfg(not(feature = "v2"))]
    return 1;
}
```

`version()` returns:

* `1` when built **without** the `v2` feature flag.
* `2` when built **with** `--features v2`.

This is a compile-time feature flag controlled by the `[features]` section of
`contracts/example-target/Cargo.toml`. It is used by the upgrade-manager test
suite to distinguish the v1 and v2 fixture WASMs:

* `target_v1.wasm` — built without `v2` → `version()` returns `1`.
* `target_v2.wasm` — built with `--features v2` → `version()` returns `2`.

Because the feature is resolved at compile time, a single compiled WASM reports
one fixed version; there is no runtime toggle.

---

## Front-running prevention: the constructor is atomic with deployment

The manager address is stored in the same transaction that deploys the
contract. `__constructor` runs **atomically during deployment**, so there is no
window between deployment and initialization in which an observer could call a
separate initializer and claim the manager role. The constructor cannot be
re-called after deployment.

This contrasts with the upgrade-manager's `initialize()`, which is a separate,
callable function and therefore explicitly calls `admin.require_auth()` to
prevent a competing initialization (see the Phase 1 migration guide).

---

## How the contract serves as a reference for production targets

A production target contract upgradable by the Soroban Upgrade Manager should
follow the same pattern:

1. **Store the manager at deploy time** via a constructor, so the manager is
   set atomically with deployment (no front-running window).
2. **Expose `upgrade(new_wasm_hash)`** that loads the stored manager address,
   calls `manager.require_auth()` to authorize the caller, and calls
   `env.deployer().update_current_contract_wasm(new_wasm_hash)` to replace the
   contract's WASM.
3. **Expose read-only queries** (e.g. `version()`, `get_manager()`) for tooling
   and verification.

The example-target is deliberately minimal so the auth and upgrade mechanics
are easy to read and copy. Production targets may add additional authorization,
version gating, or metadata, but should keep the
`manager.require_auth()` + `update_current_contract_wasm()` upgrade pattern.

---

## Quick reference

| Function | Auth | Behavior |
|----------|------|----------|
| `__constructor(manager)` | None (deploy-time, atomic) | Stores manager; manager is a contract address that cannot sign |
| `version()` | None | Returns `1` or `2` based on the `v2` feature flag |
| `get_manager()` | None | Returns the stored manager address (read-only) |
| `upgrade(new_wasm_hash)` | `manager.require_auth()` | Replaces WASM; only the stored manager may call |

---

## Source of truth

`contracts/example-target/src/lib.rs` — module doc comment, `__constructor`,
`version`, `get_manager`, and `upgrade`. If anything here conflicts with the
source, the source takes precedence.