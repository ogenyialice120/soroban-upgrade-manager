# Design: `get_config_snapshot` view for proposals

**Issue:** #108  
**Status:** Proposed — awaiting maintainer approval  
**Branch:** `feature/get-config-snapshot-view`  
**Type:** Convenience view (read-only, no contract state changes)

---

## 1. Problem statement

Each `Proposal` snapshots the governance config at creation time
(`snap_voting_period`, `snap_timelock_delay`, `snap_quorum`,
`snap_threshold_pct`). The existing `get_proposal(proposal_id)` view returns the
entire `Proposal` struct, including all snapshot fields. Clients that only need
the snapshotted governance rules must extract four fields from the full struct.

There is no dedicated view that returns **just** the config snapshot. A focused
`get_config_snapshot(proposal_id)` view would be more convenient for clients.

---

## 2. Proposed design

### 2.1 Return type

A new `#[contracttype]` struct `ConfigSnapshot` containing the four snapshotted
governance parameters:

```rust
/// The governance configuration snapshotted into a proposal at creation time.
/// Later `update_config` calls do not affect in-flight proposals because each
/// proposal carries its own snapshot.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigSnapshot {
    /// Snapshot of Config.voting_period at creation time.
    pub voting_period: u32,
    /// Snapshot of Config.timelock_delay at creation time.
    pub timelock_delay: u32,
    /// Snapshot of Config.quorum at creation time.
    pub quorum: u32,
    /// Snapshot of Config.approval_threshold_pct at creation time.
    pub approval_threshold_pct: u32,
}
```

**Naming rationale:** The fields omit the `snap_` prefix used on `Proposal`
because the struct name (`ConfigSnapshot`) already conveys that these are
snapshotted values. This keeps the API clean while remaining self-documenting.

### 2.2 Function signature

Add a new public read-only function to `lib.rs`:

```rust
/// Return the governance configuration snapshotted into a proposal at
/// creation time. Later `update_config` calls do not affect this snapshot.
///
/// # Errors
/// - `ProposalNotFound` — unknown proposal ID.
pub fn get_config_snapshot(env: Env, proposal_id: u64) -> Result<ConfigSnapshot, Error> {
    let proposal = load_proposal(&env, proposal_id)?;
    Ok(ConfigSnapshot {
        voting_period: proposal.snap_voting_period,
        timelock_delay: proposal.snap_timelock_delay,
        quorum: proposal.snap_quorum,
        approval_threshold_pct: proposal.snap_threshold_pct,
    })
}
- **Errors:** `ProposalNotFound` if the proposal ID is unknown (same as
  `get_proposal`). No other errors possible — this is a pure read of already-
  persisted fields.
- **Auth:** None required (read-only, like `get_proposal`, `get_config`,
  `get_admin`).
- **TTL:** Calls `load_proposal`, which extends the persistent TTL for the
  proposal entry (consistent with `get_proposal`).
- **Implementation location:** The body can live in `lib.rs` (delegating to
  `load_proposal` from `governance.rs`) or a thin helper in `governance.rs`.
  Given its simplicity, inlining in `lib.rs` is sufficient.

### 2.3 API table addition (README)

| Function | Description | Auth |
|----------|-------------|------|
| `get_config_snapshot` | Fetch the snapshotted governance config for a proposal. | Read-only |

---

## 3. Is this worth adding vs. extracting from `get_proposal`?

### Arguments in favor

1. **Focused response:** Clients that only need governance parameters (e.g. a UI
   showing "this proposal was created under quorum=3, threshold=51%") receive a
   smaller struct instead of the full `Proposal` (which includes `target`,
   `new_wasm_hash`, `title`, `description`, `metadata_hash`, vote counts,
   status, etc.). Smaller return values mean lower calldata/serialization cost.

2. **API stability:** If `Proposal` gains new fields in the future (e.g.
   `cancelled_at`, `executed_at`, `proposal_type`), `ConfigSnapshot` remains
   unchanged. Clients depending on `get_config_snapshot` are insulated from
   unrelated struct growth.

3. **Self-documenting:** A dedicated view makes it obvious that these four
   values are a unit (the snapshotted config) rather than four unrelated fields
   on a larger struct.

4. **Consistency:** The contract already has `get_config` (current config) and
   `get_proposal` (full proposal). `get_config_snapshot` fills the gap between
   them — "the config as it was when this proposal was created."

### Arguments against

1. **Redundancy:** Clients can already extract the four snapshot fields from
   `get_proposal()`. The view adds no new information — it's purely a
   projection.

2. **Surface area:** Every public function increases the contract's audit and
   maintenance burden. For a "lean, ultra-secure upgrade layer" (per
   CONTRIBUTING.md), adding convenience views should be weighed carefully.

3. **Client-side extraction is trivial:** Destructuring four fields from a
   `Proposal` struct is a one-liner in most languages.

### Recommendation

**Add it.** The convenience value is modest but real, and the implementation
cost is near-zero (one function, ~10 lines, no new storage, no new errors).
It matches the existing pattern of having both a full-struct getter
(`get_proposal`) and focused getters (`get_config`, `get_admin`,
`time_until_executable`). The API stability benefit (insulating clients from
future `Proposal` field additions) is the strongest argument.


---

## 4. Tests needed

| Test name | What it verifies |
|-----------|-----------------|
| `test_get_config_snapshot_returns_snapshot_fields` | `get_config_snapshot` returns the four values that were snapshotted at proposal creation (matching `Config` at that time). |
| `test_get_config_snapshot_proposal_not_found` | Unknown proposal ID returns `Error::ProposalNotFound`. |
| `test_config_snapshot_isolation_after_update_config` | After `update_config` changes the global config, `get_config_snapshot` on an existing proposal still returns the **original** snapshotted values (proves snapshots are immutable). |

The third test overlaps conceptually with the existing `test_config_snapshot_isolation`
(which verifies `get_proposal` snapshot fields after `update_config`). The new
test asserts the same property through `get_config_snapshot`.

No changes to `ProposalStatus`, `Error`, `Config`, or `DataKey` are required.
No new events are emitted.

---

## 5. Files affected (when implemented)

- `contracts/upgrade-manager/src/types.rs` — add `ConfigSnapshot` struct.
- `contracts/upgrade-manager/src/lib.rs` — add `get_config_snapshot` public function.
- `README.md` — add row to Contract API table.
- `contracts/upgrade-manager/src/test.rs` — add the three tests listed above.
- `docs/claims-audit.md` — optional: add a claim row if the maintainer wants
  the new view covered by the claims audit (not required for a convenience
  view).
- `_CHANGELOG.md` — append a dated entry per CONTRIBUTING.md.

---

## 6. Out of scope

- No contract state changes (no new storage entries, no config changes).
- No changes to `Proposal`, `ProposalStatus`, `Error`, or `DataKey`.
- No implementation code in this PR — this document is the design proposal
  only. Implementation proceeds only after maintainer approval.

```
