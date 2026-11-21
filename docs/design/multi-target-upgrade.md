# Design: Multi-Target Upgrade in a Single Proposal (Issue #120)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #120  
**Branch:** `design/multi-target-upgrade`  
**Type:** Manager contract enhancement (breaking struct change)  
**Date:** 2026-11-20

---

## 1. Problem

A single proposal upgrades one contract. Coordinated protocol upgrades that
must replace multiple contracts atomically require creating N separate proposals
and executing them in separate transactions — there is no on-chain guarantee
they all pass or all fail together.

---

## 2. Existing guarantee this touches

> **One target per proposal.** `Proposal` stores a single `target: Address`
> and a single `new_wasm_hash: BytesN<32>`. `execute()` calls `invoke_upgrade`
> once with that one pair.

This design replaces that guarantee with:

> **One or more targets per proposal.** `Proposal` stores a parallel
> `Vec<Address>` and `Vec<BytesN<32>>` with at least one entry. `execute()`
> calls `invoke_upgrade` for each pair. All upgrades succeed or the transaction
> reverts entirely (all-or-nothing atomicity guaranteed by Soroban's transaction
> model).

---

## 3. Design

### 3.1 `Proposal` struct change

Replace the two single-target fields with parallel vectors:

```rust
pub struct Proposal {
    pub id: u64,
    // --- replaces: target: Address, new_wasm_hash: BytesN<32> ---
    /// Contracts to be upgraded. Must have the same length as new_wasm_hashes.
    pub targets: Vec<Address>,
    /// WASM hashes parallel to targets. targets[i] is upgraded to new_wasm_hashes[i].
    pub new_wasm_hashes: Vec<BytesN<32>>,
    // ... all other fields unchanged ...
}
```

A `max_targets` bound (see §3.5) caps the vector length to prevent unbounded
gas consumption. Minimum length is 1 (a single-target proposal is still valid).

### 3.2 `propose_upgrade` signature change

```rust
pub fn propose_upgrade(
    env: Env,
    proposer: Address,
    targets: Vec<Address>,
    new_wasm_hashes: Vec<BytesN<32>>,
    title: String,
    description: String,
    metadata_hash: BytesN<32>,
) -> Result<u64, Error>
```

The old `target: Address` and `new_wasm_hash: BytesN<32>` arguments are
replaced. This is a **breaking** signature change for all existing callers.

### 3.3 Input validation in `create_proposal`

Before storing the proposal:

1. `targets.len() == new_wasm_hashes.len()` — vectors must be parallel. Fail
   with new error `TargetHashLengthMismatch` on violation.
2. `targets.len() >= 1` — at least one target required. Fail with new error
   `EmptyTargets` on empty vectors.
3. `targets.len() <= config.max_targets` — cap to prevent gas abuse. Fail with
   new error `TooManyTargets` on violation.

No duplicate-target check is enforced: duplicates are allowed (an upgrade could
validly run the same contract through two WASM hashes in sequence, though that
is unusual). Callers are expected to deduplicate when needed.

### 3.4 Atomicity in `execute`

The checks-effects-interactions pattern is preserved and extended:

```rust
pub fn execute(env: Env, proposal_id: u64) -> Result<(), Error> {
    let mut proposal = load_proposal(&env, proposal_id)?;
    assert_timelock_passed(&env, &proposal)?;

    // Effects: set Executed BEFORE any external call.
    proposal.status = ProposalStatus::Executed;
    save_proposal(&env, &proposal);
    emit_executed(&env, proposal_id);

    // Interactions: iterate targets in order.
    // If ANY invoke_upgrade panics, the Soroban host reverts the ENTIRE
    // transaction, including the status update above. The proposal returns
    // to Queued, and execute() can be retried.
    for i in 0..proposal.targets.len() {
        invoke_upgrade(
            &env,
            &proposal.targets.get(i).unwrap(),
            &proposal.new_wasm_hashes.get(i).unwrap(),
        );
    }

    Ok(())
}
```

**Atomicity guarantee:** Soroban transactions are atomic. If `invoke_upgrade`
panics (e.g., the target lacks an `upgrade()` function, or its auth check
fails), the host reverts the entire transaction including the `save_proposal`
state update. The proposal remains `Queued` and can be retried. This is the
same behavior as the existing single-target `execute`.

**Partial failure is impossible at the transaction level.** There is no need
for a manual rollback loop — Soroban's host provides this for free. This is a
key difference from EVM contracts where manual rollback is required.

### 3.5 `max_targets` in `Config`

Add `max_targets: u32` to `Config`:

```rust
pub struct Config {
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
    pub max_targets: u32,   // NEW — 1..=N, validated >= 1
}
```

A new validation rule in `validate_config`: `max_targets >= 1`.
A new error `InvalidMaxTargets` (code 25) is returned when `max_targets == 0`.

The admin sets `max_targets` at initialization and can update it via
`update_config`. Existing deployments that do not explicitly set a value will
need to pass `1` for single-target-only behavior (backward-compatible at the
configuration level).

A sensible default for new deployments is `max_targets = 10`.

### 3.6 Existing tests that need updating

Every test that calls `propose_upgrade` with the old `(target, new_wasm_hash)`
arguments must be updated to pass `(vec![&env, target], vec![&env, new_wasm_hash])`.
The `setup()` helper's `initialize` call must pass `max_targets = 10`.

Specifically, all tests in `src/test.rs` that call `client.propose_upgrade(...)`.
Based on the current test file this affects approximately 40 call sites.

### 3.7 New error codes

| Code | Name | Trigger |
|------|------|---------|
| 25 | `InvalidMaxTargets` | `max_targets == 0` in `initialize` or `update_config`. |
| 26 | `EmptyTargets` | `targets` vector is empty in `propose_upgrade`. |
| 27 | `TargetHashLengthMismatch` | `targets.len() != new_wasm_hashes.len()`. |
| 28 | `TooManyTargets` | `targets.len() > config.max_targets`. |

---

## 4. Complexity vs. alternatives

### Why not separate proposals?

Separate proposals have no atomicity guarantee. Two proposals can pass or fail
independently. During the gap between their executions, the protocol is
partially upgraded, which may be unsafe.

### Why not a "proposal group"?

A linked group of single-target proposals would require a new data structure
and a new execution primitive. The vector approach reuses all existing
governance machinery (vote, finalize, timelock, cancel) with minimal code
change.

### All-or-nothing vs. best-effort

Best-effort (succeed on some targets, skip failures) would require tracking
which targets succeeded, introducing new state and new failure modes. It
would also break the invariant that a `Queued` proposal can always be retried.
All-or-nothing is simpler, safer, and consistent with the existing behavior.

---

## 5. Files affected (when implemented)

- `contracts/upgrade-manager/src/types.rs` — `Proposal` struct: replace
  `target`/`new_wasm_hash` with `targets`/`new_wasm_hashes`; add
  `max_targets` to `Config`; add new error codes.
- `contracts/upgrade-manager/src/governance.rs` — `create_proposal`: accept
  `Vec<Address>` and `Vec<BytesN<32>>`, validate lengths and `max_targets`;
  update `validate_config` to check `max_targets`.
- `contracts/upgrade-manager/src/lib.rs` — `propose_upgrade` signature;
  `execute` iteration loop; `initialize`/`update_config` accept `max_targets`.
- `contracts/upgrade-manager/src/test.rs` — update all ~40 `propose_upgrade`
  call sites; add new multi-target tests (see §6).
- `README.md` — update Config Reference, API table, error-code table.
- `docs/claims-audit.md` — update/add rows for multi-target behavior.
- `_CHANGELOG.md` — append a dated entry per CONTRIBUTING.md.

---

## 6. Tests needed

| Test name | What it verifies |
|-----------|-----------------|
| `test_single_target_still_works` | `propose_upgrade` with one-element vectors behaves identically to the old single-target API. |
| `test_multi_target_proposal_created` | Proposal stores two targets and two hashes; both readable from `get_proposal`. |
| `test_multi_target_execute_upgrades_all` | Execute a two-target proposal; both contracts have their WASM replaced. |
| `test_empty_targets_fails` | Empty `targets` vector returns `EmptyTargets`. |
| `test_targets_hashes_length_mismatch_fails` | `targets.len() != new_wasm_hashes.len()` returns `TargetHashLengthMismatch`. |
| `test_too_many_targets_fails` | `targets.len() > config.max_targets` returns `TooManyTargets`. |
| `test_max_targets_exactly_at_limit_succeeds` | `targets.len() == config.max_targets` succeeds. |
| `test_invalid_max_targets_zero_fails` | `initialize` with `max_targets = 0` returns `InvalidMaxTargets`. |
| `test_multi_target_atomicity_first_target_fails` | First target has no `upgrade()` function; transaction reverts, proposal remains `Queued`. |
| `test_multi_target_atomicity_second_target_fails` | Second target fails; transaction reverts, proposal remains `Queued` (first target's upgrade also rolled back). |
| `test_multi_target_governance_full_flow` | Full flow (initialize → propose → vote → finalize → execute) with two targets. |
| `test_multi_target_cancel` | Admin can cancel a multi-target proposal. |
| `test_multi_target_snapshot_isolation` | Config change after proposal creation does not affect `max_targets` check for in-flight proposal (proposal already stored its targets). |

---

## 7. Out of scope

- Per-target partial success / skip logic (all-or-nothing only).
- Duplicate-target detection (allowed — unusual but valid).
- No implementation code in this PR — this document is the design proposal
  only. Implementation proceeds only after maintainer approval.
