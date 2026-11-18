# Design: Compact Proposal Storage Layout (Issue #127)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #127  
**Branch:** `design/proposal-storage-layout`  
**Type:** Storage-layout analysis + design (manager-side)  
**Date:** 2026-11-18

---

## 1. Problem

Each proposal is stored as a full `Proposal` struct in persistent storage under `DataKey::Proposal(id)` (`contracts/upgrade-manager/src/governance.rs:114`, `save_proposal`):

```rust
pub fn save_proposal(env: &Env, proposal: &Proposal) {
    let key = DataKey::Proposal(proposal.id);
    env.storage().persistent().set(&key, proposal);
    env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
    env.storage().instance().extend_ttl(INSTANCE_TTL_BUMP, INSTANCE_TTL_BUMP);
}
```

`Proposal` has 16 fields (`contracts/upgrade-manager/src/types.rs`), including:
- `description: String` — up to 256 bytes of free text.
- 4 snapshot `u32` fields (`snap_voting_period`, `snap_timelock_delay`, `snap_quorum`, `snap_threshold_pct`).
- 2 `Address` fields (`target`, `proposer`), 1 `BytesN<32>` (`new_wasm_hash`), 5 `u32`/`u64` scalar fields, 1 `ProposalStatus` enum.

Because Soroban serializes `contracttype` structs as a map carrying field names, both field-name symbols and field values occupy bytes. The entire struct is rewritten on every state-changing proposal operation, even when only voting/timelock metadata changes. Long descriptions inflate every read/write.

---

## 2. Existing guarantee touched

> `Proposal` is stored as one struct.

This design splits `description` out of the main `Proposal` entry, so a proposal is no longer a single atomic storage entry. Any code that reads/writes the full `Proposal` struct must be updated to also load/persist the separate description key.

---

## 3. Current storage footprint estimate (per proposal)

| Component | Approx. bytes |
|---|---|
| `id: u64` | 8 |
| `target: Address` | ~32 |
| `new_wasm_hash: BytesN<32>` | 32 |
| `created_at / voting_ends_at / executable_at: u32` ×3 | 12 |
| `yes_votes / no_votes: u32` ×2 | 8 |
| `status: ProposalStatus` enum | 1–4 |
| `proposer: Address` | ~32 |
| `description: String` (up to 256 bytes) | up to ~260 incl. length |
| 4 snapshot `u32` | 16 |
| Serialization overhead (field-name symbols, map framing) | variable, tens of bytes |

**Hot-path total (excluding description):** ~100–130 bytes.  
**With a max-length description:** up to ~390 bytes, all rewritten on every proposal mutation.

---

## 4. Design alternatives considered

| Alternative | Estimated savings | Complexity | Verdict |
|---|---|---|---|
| **Split `description` into separate key** `DataKey::ProposalDescription(u64) -> String` | Removes up to ~260 bytes (+ field name) from the hot-path entry on every metadata read/write | Low–Medium | **Recommended.** Description is rarely read; one extra read only when needed. Biggest win for the least complexity. |
| Shorter field names (e.g. `snap_voting_period` → `svp`) | Marginal (per-field symbol bytes) | Low | Savings uncertain (Soroban may dedupe type schema); hurts readability. Not recommended alone. |
| Pack 4 snapshot `u32` fields | A few bytes | High | Snapshots are independent config values; packing complicates reads/writes. Not recommended. |
| Smaller integer types | Negligible | Low | Ledger sequences and vote counts already `u32`; no meaningful shrink. |

---

## 5. Recommended design

### 5.1 Storage

Add a description-only key:

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    ...
    /// Proposal by ID (metadata only; description moved to its own key).
    Proposal(u64),
    /// Proposal description text by ID.
    ProposalDescription(u64),
    ...
}
```

Remove `description` from the `Proposal` struct. `save_proposal` writes:
- `DataKey::Proposal(id)` → metadata-only `Proposal`.
- `DataKey::ProposalDescription(id)` → `description: String`.
- Extend TTL on both keys.

Add a read-only query `get_proposal_description(env, id) -> String` and keep `get_proposal` returning metadata only (or have it optionally load the description).

### 5.2 Guarantee implication

A proposal is now two storage entries. Callers that need the full proposal (including description) must read both keys. This is the existing guarantee being touched: "Proposal is stored as one struct."

### 5.3 Does the savings justify the complexity?

For proposals with long descriptions, yes — the hot-path entry shrinks from up to ~390 bytes to ~120 bytes, a meaningful gas reduction on the frequent propose/vote/finalize path. For short descriptions the win is smaller, but the separate-key pattern also future-proofs adding other large optional fields. The added complexity is contained to one extra storage key and one extra read path.

---

## 6. Tests needed

1. Unit test: `description` stored/retrieved via `DataKey::ProposalDescription` separately from metadata.
2. Unit test: `get_proposal` returns metadata without description; `get_proposal_description` returns the text.
3. Unit test: `save_proposal` writes both keys and extends TTL on both.
4. Unit test: proposal lifecycle (propose → vote → finalize → execute) still works with the split layout.
5. TTL test: description key gets `PERSISTENT_TTL_BUMP`.

---

## 7. Existing tests that would need updating

Search for `.description` usage and full-`Proposal` struct assertions in:
- `contracts/upgrade-manager/src/test.rs`
- `contracts/upgrade-manager/src/governance.rs`
- `contracts/upgrade-manager/src/lib.rs`
- `contracts/upgrade-manager/src/types.rs`

Any test that constructs, compares, or reads `Proposal.description` must be updated to load the separate description key.

---

## 8. Out of scope

- Code changes: none until maintainer approval.
- Packing snapshot fields or renaming fields (rejected above as low value/high complexity).

No code should be written until a maintainer approves this design.
