# Writing a Safe Target Contract

This guide explains how to write a Soroban contract that can be safely upgraded
through the upgrade manager. It covers authentication, storage layout
compatibility, constructor-only initialization, and common pitfalls.

The reference implementation is
[`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs).
Every code claim in this guide is backed by that file.

---

## Overview

The upgrade manager performs upgrades by calling `target.upgrade(new_wasm_hash)`
via cross-contract invocation. The target contract must expose an `upgrade`
function that:

1. Authenticates that the caller is the stored upgrade-manager address.
2. Calls `env.deployer().update_current_contract_wasm(new_wasm_hash)` to swap
   the WASM.

If the target omits step 1, anyone who can invoke the manager's `execute`
function can trigger an upgrade, bypassing governance entirely.

---

## 1. Store the manager address at deploy time via `__constructor`

The upgrade manager's address must be stored before anyone can call `upgrade`.
The safe way to do this is in the Soroban constructor (`__constructor`), which
the host calls atomically in the same transaction that deploys the contract. It
cannot be called again after deployment, so there is no window for a
front-running attack.

```rust
pub fn __constructor(env: Env, manager: Address) {
    // The manager is a contract address and cannot sign a transaction,
    // so we intentionally do NOT call require_auth() here.
    env.storage()
        .instance()
        .set(&soroban_sdk::symbol_short!("manager"), &manager);
}
```

This is exactly what `example-target` does. Because `manager` is a contract
address (not a user key), it cannot produce a Soroban auth signature. Calling
`manager.require_auth()` in the constructor would cause every deployment to
fail. The constructor itself is protected from replay by the Soroban host — it
runs exactly once and cannot be invoked from outside.

**Key point:** Pass the manager's contract ID as the `--manager` argument when
deploying, not after the fact through a separate initializer function. A
separate initializer can be called by an observer before the deployer, stealing
control of the upgrade path.

---

## 2. Call `manager.require_auth()` in `upgrade()`

The `upgrade` function must enforce that only the stored manager can call it:

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

`manager.require_auth()` tells the Soroban host to verify that the manager
contract authorized this call. When the upgrade manager calls `invoke_upgrade`,
it passes its own authorization. If `require_auth()` is missing, any caller
can trigger the WASM swap. This is the most critical security property of a
target contract.

---

## 3. Storage layout compatibility across upgrades

Soroban's `update_current_contract_wasm` swaps the WASM bytecode but leaves
all stored data intact. If the new WASM reads a storage key with a different
type than the old WASM wrote, the deserialization will panic and brick the
contract.

Rules for safe storage layout changes:

| Change | Safe? |
|---|---|
| Adding a new storage key | ✅ Safe — old code never wrote it; new code reads with a default |
| Removing a storage key (stop reading it) | ✅ Safe — old data persists but is ignored |
| Keeping a key with the same type | ✅ Safe |
| Renaming a key's symbol | ⚠️ Treated as a new key; old data remains orphaned |
| Changing a key's value type | ❌ Unsafe — old data will fail to deserialize under the new type |

If you must change a key's type, perform the data migration in the **old**
contract's `upgrade()` function before the WASM swap. The old code runs during
`upgrade()`, so it can read the old format, transform the data, and write it
under the new key or new format before `update_current_contract_wasm` replaces
the WASM.

---

## 4. Constructor-only initialization

Only use `__constructor` to store values that must be set exactly once and
cannot change. For values that need to evolve, use separate admin-gated
functions.

If you use a separate `initialize()` function instead of `__constructor`,
protect it against being called more than once and against front-running.
`__constructor` is the simpler, safer choice for the manager address because
the Soroban host guarantees it runs exactly once at deploy time.

---

## 5. Checks-effects-interactions pattern

When writing `upgrade()`, update any internal state before calling
`update_current_contract_wasm`. In `example-target`, the `upgrade` function
does not need to update internal state, so there is nothing to do before the
WASM swap. If your contract tracks upgrade history or emits an event, do so
before the WASM swap:

```rust
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
    let manager: Address = env
        .storage()
        .instance()
        .get(&soroban_sdk::symbol_short!("manager"))
        .expect("manager not set");
    manager.require_auth();

    // ✅ Update state and emit events BEFORE the WASM swap.
    // env.events().publish(...);

    env.deployer().update_current_contract_wasm(new_wasm_hash);
    // ❌ Do NOT put logic after update_current_contract_wasm.
    //    The old WASM code stops executing after this call.
}
```

Logic placed after `update_current_contract_wasm` does not execute; the host
stops running the old WASM at that point.

---

## 6. Read-only helpers

Expose a read-only function that returns the stored manager address so callers
can verify the configuration without reading raw storage:

```rust
pub fn get_manager(env: Env) -> Address {
    env.storage()
        .instance()
        .get(&soroban_sdk::symbol_short!("manager"))
        .expect("manager not set")
}
```

This is the `get_manager` function in `example-target`.

---

## Common pitfalls

### Forgetting `manager.require_auth()`

If `upgrade()` calls `update_current_contract_wasm` without first calling
`manager.require_auth()`, the upgrade-manager's governance is bypassed. Anyone
who can call the manager's `execute` function can trigger an upgrade on the
unprotected target, regardless of the proposal vote.

### Changing storage layout without migration

Adding a field to a stored struct, renaming a storage key symbol, or changing
a value type between v1 and v2 will cause deserialization panics when the
upgraded WASM reads pre-upgrade data. Test your storage layout changes by
writing a migration test that creates a v1 contract state, upgrades the WASM,
and reads the data under the new code.

### Using a separate initializer instead of `__constructor`

A separate `initialize()` function that is not called in the deployment
transaction creates a front-running window. An observer can watch the mempool,
see your deployment transaction, and call `initialize()` with a different
manager address before your follow-up initialization transaction confirms. Use
`__constructor` to close this window.

### No upgrade target compatible check

The contract does not verify that the new WASM is storage-layout-compatible
with the existing target. Run a compatibility check (e.g.,
[soroban-upgrade-safeguard](https://github.com/eigercoin/soroban-upgrade-safeguard))
before creating a governance proposal. If the new WASM is incompatible and the
upgrade executes, the target may be bricked.

---

## Minimal safe target — complete example

The full reference implementation is in
[`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs).
Key points:

- `__constructor` stores the manager address without `require_auth()` (manager is a contract).
- `upgrade()` loads the manager, calls `manager.require_auth()`, then calls `update_current_contract_wasm`.
- `get_manager()` returns the stored manager address for verification.
- `version()` returns `1` normally and `2` when compiled with `--features v2`, demonstrating how feature flags can version a target contract.

---

## See also

- [docs/walkthrough.md](walkthrough.md) — full deployment walkthrough using `example-target`
- [docs/threat-model.md](threat-model.md) — adversary analysis including the malicious target threat
- [README.md — Security Model & Limitations](../README.md#-security-model--limitations)
- [contracts/upgrade-manager/src/governance.rs](../contracts/upgrade-manager/src/governance.rs) — `invoke_upgrade` implementation
