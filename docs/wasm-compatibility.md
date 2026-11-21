# WASM Compatibility Guide

Verifying that a new WASM binary is storage-layout compatible with an existing
target contract **before** proposing an upgrade through the upgrade manager.

> ⚠️ **Important:** The `soroban-upgrade-manager` contract does **not** perform any
> WASM compatibility check on-chain or off-chain. It accepts any `new_wasm_hash`
> supplied to `propose_upgrade()`. This guide is purely advisory — it helps
> proposal authors and voters perform their own review. See the
> ["No WASM compatibility check" limitation](../README.md#security-model--limitations)
> in the README.

---

## 1. What storage layout compatibility means in Soroban

A Soroban contract's state lives in **instance storage** (one entry per contract)
and **persistent storage** (key/value entries). When you call
`env.deployer().update_current_contract_wasm(new_hash)`, the contract's code is
replaced but its **storage is preserved**. The new WASM must be able to read the
old data correctly, or the contract will behave incorrectly or become bricked.

Two WASM versions are storage-layout compatible when:

| Aspect | Safe (compatible) | Unsafe (incompatible) |
|--------|-------------------|-----------------------|
| **Key types** | Same `Symbol`, `BytesN`, `Address`, etc. used as storage keys | Changing a key's type (e.g. `Symbol` → `BytesN`) |
| **Value types** | Same deserialization type for each key | Changing a value's type (e.g. `u32` → `u64`, `Address` → `BytesN`) |
| **Adding keys** | Adding new storage keys that the old code never reads | — |
| **Removing keys** | — | Removing keys the new code still reads |
| **Renaming keys** | — | Renaming a storage key (old data becomes unreachable under the old name) |
| **Default values** | New keys have sensible defaults when absent | New keys are required without a default (read panics on old data) |
| **Persistent vs instance** | Same storage tier (instance vs persistent) for each key | Moving a key from instance to persistent storage or vice versa |

### Why this matters for upgrades

When `execute()` calls `target.upgrade(new_wasm_hash)`:

1. The target's instance storage and persistent storage entries are **kept**.
2. The new WASM code is loaded.
3. The new code's `upgrade()` (or constructor-like init) runs and may read
   existing storage.

If the new code interprets an existing `u32` value as a `u64`, or reads a key
that no longer exists, the contract may trap, return garbage, or lose data.


## 2. Manual review checklist

Before proposing an upgrade, review the old and new target contract source and
answer every question below. If any answer is "yes" to an unsafe item, the
upgrade is **not** storage-layout compatible and should not be proposed without
a data migration strategy.

### A. Storage key inventory

- [ ] List every storage key used by the current (deployed) contract.
- [ ] List every storage key used by the new contract.
- [ ] Are all keys present in both versions using the **same type**?
- [ ] Are any keys being removed? If yes, is the new code guaranteed never to read them?
- [ ] Are any keys being renamed? (Unsafe — old data is orphaned.)
- [ ] Are any keys moving between instance and persistent storage? (Unsafe.)

### B. Storage value types

- [ ] For each shared key, is the value type identical (e.g. both `u32`, both `Address`, both `BytesN<32>`)?
- [ ] Are any values changing from `Option<T>` to `T` or vice versa?
- [ ] Are any enum variants being added, removed, or reordered? (Soroban serializes enums by variant index; reordering changes deserialization.)

### C. Default values for new keys

- [ ] Does every new storage key have a default value when read from a contract that never wrote it?
- [ ] Does the new code call `.unwrap()` or `.expect()` on a `get()` that may return `None` for old data?

### D. Upgrade function behavior

- [ ] Does `upgrade()` authenticate the manager (`manager.require_auth()`)?
- [ ] Does `upgrade()` call `env.deployer().update_current_contract_wasm(hash)` only after auth succeeds?
- [ ] Does `upgrade()` read any storage before replacing WASM? If so, are those reads compatible with the old layout?

### E. Off-chain verification

- [ ] Have you compared the two WASM binaries with a storage-layout analysis tool?

## 3. soroban-upgrade-safeguard

[soroban-upgrade-safeguard](https://github.com/eigercoin/soroban-upgrade-safeguard)
is an external tool (not part of this repository) that analyzes two Soroban
WASM binaries and reports storage-layout incompatibilities.

### How to use it

```bash
# Install (external repo — follow its own installation instructions)
cargo install soroban-upgrade-safeguard

# Compare old vs new target WASM
soroban-upgrade-safeguard compare \
  --old target_v1.wasm \
  --new target_v2.wasm \
  --format json
```

### What it checks

The tool inspects the WASM's storage-related instructions (`storage.get`, `storage.set`, `storage.has`, etc.) and flags:

- Type mismatches for the same storage key across versions
- Keys present in one version but not the other
- Changes in serialization patterns that suggest layout changes

### Limitations

- It is a **static analysis** tool — it cannot catch every runtime incompatibility (e.g. logic changes that assume a specific data shape).
- It does not interact with the `soroban-upgrade-manager` contract.
- It is maintained independently; pin a version and review its output manually.


## 4. Examples of safe and unsafe storage changes

### ✅ Safe: Adding a new optional key

Old contract (`v1`) stores:

```rust
// Key: symbol_short!("owner") → Address
```

New contract (`v2`) adds:

```rust
// Key: symbol_short!("owner") → Address  (unchanged)
// Key: symbol_short!("fee_bps") → u32   (new, defaults to 0 when absent)
```

Reading `fee_bps` on old data returns `None`; the new code treats `None` as `0`.
**Safe.**

### ✅ Safe: Adding a new contract function that uses existing keys

New contract adds a `pause()` function that reads `symbol_short!("paused") → bool`
and writes `true`. The key didn't exist before; reads return `None`/`false`.
**Safe** as long as `None` is treated as "not paused".

### ❌ Unsafe: Changing a value type

Old:
```rust
// Key: symbol_short!("version") → u32   // value = 1
```

New:
```rust
// Key: symbol_short!("version") → u64   // reads old 4-byte u32 as 8-byte u64 → garbage/trap
```

**Unsafe** — the serialized byte length differs.

### ❌ Unsafe: Removing a key still read by new code

Old:
```rust
// Key: symbol_short!("minter") → Address
```

New contract removes the `minter` key but a migration function still calls
`storage.get::<Address>(symbol_short!("minter"))`.

**Unsafe** — the read returns `None`, which may cause a panic or incorrect behavior.

### ❌ Unsafe: Renaming a key

Old:
```rust
// Key: symbol_short!("admin") → Address
```

New:
```rust
// Key: symbol_short!("governor") → Address   // old "admin" data is orphaned
```

**Unsafe** — data written under `"admin"` is inaccessible under `"governor"`.

### ❌ Unsafe: Reordering enum variants

Old:
```rust
enum Status { Active = 0, Paused = 1 }
```

New:
```rust
enum Status { Paused = 0, Active = 1 }   // serialized index changes
```

**Unsafe** — an old `Active` (index 0) deserializes as `Paused` in the new code.


## 5. Recommended workflow (summary)

1. **Build both WASM versions** of the target contract.
2. **Run `soroban-upgrade-safeguard compare`** (or equivalent review) on old vs new.
3. **Apply the manual checklist** above to confirm storage-layout compatibility.
4. **Upload the new WASM** to the network and record the hash.
5. **Create a proposal** with the hash. Include the compatibility-check report
   link in the proposal `description` or `metadata_hash` so voters can verify.
6. **Vote, finalize, and execute** following the upgrade manager lifecycle.

> This is guidance, not enforcement. The contract does not verify the
> description or require a compatibility check. Voters should independently
> verify the proposal before voting.

---

## See also

- [README.md — Recommended Workflow](../README.md#recommended-workflow)
- [README.md — Security Model & Limitations](../README.md#security-model--limitations)
- [docs/walkthrough.md — Storage layout note](../docs/walkthrough.md)
- [soroban-upgrade-safeguard](https://github.com/eigercoin/soroban-upgrade-safeguard) (external tool)

- [ ] Is the compatibility check report linked in the proposal description/metadata?
