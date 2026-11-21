# Design: Description Hash Storage Instead of Full Text (Issue #114)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #114  
**Branch:** `design/description-hash-storage`  
**Type:** Manager contract change (breaking storage layout change)  
**Date:** 2026-11-20

---

## 1. Problem

`Proposal.description` stores up to 256 bytes of UTF-8 text directly in
persistent storage. The 256-byte cap was chosen to bound cost, but long
descriptions still inflate the storage footprint of every proposal entry.
Storing only a SHA-256 hash (`BytesN<32>`) of the description text would
reduce every proposal's on-chain description storage from up to 256 bytes to
exactly 32 bytes, while moving the full text off-chain.

---

## 2. Existing guarantee this touches

> **Description is stored on-chain and readable via `get_proposal`.**
> `Proposal.description` is a `soroban_sdk::String` and can be read back
> directly by calling `get_proposal(id).description`.

Replacing `description: String` with `description_hash: BytesN<32>` removes
this guarantee. After the change:

- `get_proposal` returns `description_hash` (a 32-byte opaque value).
- Clients must supply the off-chain text and compute `sha256(text)` to verify
  it matches the stored hash.

---

## 3. Design

### 3.1 Recommendation: do not make this change

After analyzing the trade-offs, the conclusion is that this change is **not
recommended** for this contract. The reasoning follows.

**Cost saving is small.** A `soroban_sdk::String` with 256 bytes of content
costs 256 bytes plus a small header. Persistent storage on Stellar is charged
per byte, but the saving per proposal is at most ~224 bytes (from 256 to 32).
At Stellar's minimum base fee and typical rent costs, this is a minor saving
per proposal.

**The contract already has `metadata_hash: BytesN<32>`.** The `Proposal`
struct already carries a `metadata_hash` field for exactly this purpose: a
SHA-256 hash of arbitrary off-chain content (title, changelog URI,
compatibility report hash, etc.). Proposers who want to reference long
documents off-chain can include that link in `metadata_hash`. The 256-byte
`description` is intended as a short human-readable summary that clients can
display without fetching any off-chain data.

**Descriptions are already bounded.** The 256-byte cap already limits on-chain
storage consumption. The concern "storage cost for proposals with long
descriptions" is bounded in the current design.

**Loses a key usability property.** The description is the primary human-
readable field that wallets, block explorers, and governance UIs display to
voters. Requiring off-chain data retrieval and hash verification to display a
human-readable label degrades UX significantly. The contract would become less
usable by anyone without a trusted off-chain data source.

**Backward compatibility is costly.** Existing deployed proposals on-chain
store a `String` in the `description` field. Changing to `BytesN<32>` on a
live deployment would leave old proposals unreadable (the contract code could
not deserialize the old storage layout). A migration would require reading
every existing proposal, hashing its description, and rewriting the entry — a
one-time operation that is expensive and cannot be done atomically.

### 3.2 If implemented anyway — design

If a maintainer decides the saving is worth the trade-off, the minimal
implementation is:

**`Proposal` struct:**

```rust
// Before:
pub description: soroban_sdk::String,

// After:
pub description_hash: BytesN<32>,
```

**`propose_upgrade` / `create_proposal`:** Accept `description_hash:
BytesN<32>` instead of `description: String`. Remove the length and emptiness
validation for `description` (there is no content to validate).

**Existing description validation removed:**

- `Error::EmptyDescription` (code 18) is no longer triggered.
- `Error::DescriptionTooLong` (code 9) is no longer triggered.
- The 256-byte limit is removed (there is no on-chain text, only a hash).

Both error codes remain defined in the enum for backward compatibility with
clients that may check for them, but neither is ever returned after the change.

**Backward compatibility for in-flight proposals:** Old proposals stored with a
`String` are unreadable after the upgrade because the Soroban host deserializes
the storage entry using the new struct layout. There is no migration path that
does not require iterating all existing proposals. This means the change is
only safe for new deployments or for deployments with zero existing proposals.

**Off-chain convention:** Proposers supply the description text off-chain
(e.g., in a governance forum post, IPFS, or the `metadata_hash` URI). Clients
verify by computing `sha256(text)` and comparing to `description_hash`. The
contract makes no guarantees about the off-chain text's availability or content.

### 3.3 Recommendation summary

| Criterion | Current (String) | Hash alternative |
|-----------|-----------------|-----------------|
| On-chain storage per proposal | Up to 256 bytes | 32 bytes |
| Human-readable via `get_proposal` | Yes | No (need off-chain text) |
| Backward compatible with existing proposals | N/A | No — existing proposals unreadable |
| Existing `metadata_hash` already covers off-chain | Yes | Yes (redundant with this change) |
| Removes meaningful user-facing validation | No | Yes (removes EmptyDescription / DescriptionTooLong) |

**Conclusion:** The saving is marginal, the usability cost is real, the
backward-compatibility problem is severe, and `metadata_hash` already serves
the off-chain-data use case. This design is documented here for completeness
and to explain the trade-offs, but the **recommended decision is to keep the
current `description: String` field unchanged**.

---

## 4. Tests needed (if implemented)

| Test name | What it verifies |
|-----------|-----------------|
| `test_propose_with_description_hash` | `propose_upgrade` accepts `description_hash: BytesN<32>` and stores it in the proposal. |
| `test_get_proposal_returns_description_hash` | `get_proposal` returns the stored `description_hash`, not a `String`. |
| `test_description_hash_all_zeros_allowed` | A `BytesN<32>` of all zeros is accepted (no emptiness check). |
| `test_old_description_validation_errors_not_returned` | `EmptyDescription` and `DescriptionTooLong` are never returned by `propose_upgrade`. |

---

## 5. Files affected (if implemented)

- `contracts/upgrade-manager/src/types.rs` — `Proposal.description` →
  `Proposal.description_hash: BytesN<32>`.
- `contracts/upgrade-manager/src/governance.rs` — `create_proposal`: remove
  description length/empty validation; store `description_hash` directly.
- `contracts/upgrade-manager/src/lib.rs` — `propose_upgrade` signature:
  replace `description: String` with `description_hash: BytesN<32>`.
- `contracts/upgrade-manager/src/test.rs` — update all call sites that pass a
  `description: String`; remove tests for `EmptyDescription` and
  `DescriptionTooLong`; add new hash tests.
- `README.md` — update API table (new parameter type), error-code table
  (codes 9 and 18 never returned).
- `docs/claims-audit.md` — update/remove rows that reference description
  validation.
- `_CHANGELOG.md` — append a dated entry per CONTRIBUTING.md.

---

## 6. Out of scope

- Migration of existing on-chain proposals (not feasible atomically).
- Off-chain indexer changes (out of scope for the contract).
- Changes to `title: String` (title is 128 bytes; the saving would be even
  smaller and the UX cost even higher).
- No implementation code in this PR — this document is the design proposal
  only. Implementation proceeds only after maintainer approval.
