# Design: Per-Proposal Configurable Voting Parameters (Issue #129)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #129  
**Branch:** `design/per-proposal-voting-params`  
**Type:** Manager contract enhancement  
**Date:** 2026-11-20

---

## 1. Problem

Every proposal inherits its governance rules from the global `Config` at
creation time. That snapshot is immutable per proposal once created, which is
correct. However, all proposals share the same parameter values — there is no
way to create an "emergency" proposal with a shorter voting period and timelock
versus a "standard" proposal with longer windows.

The issue requests that proposers be able to specify custom `voting_period`,
`timelock_delay`, `quorum`, and `approval_threshold_pct` per proposal, within
admin-enforced bounds.

---

## 2. Existing guarantee this touches

> **All proposals use the same config snapshot.** `create_proposal` reads the
> single global `Config` and copies those values into the proposal's `snap_*`
> fields. The snapshot is immutable after creation.

This design relaxes the first part of that guarantee: `snap_*` fields can now
reflect per-proposal overrides rather than the global config, but they remain
immutable after creation. The guarantee "snapshot is immutable after creation"
is preserved.

---

## 3. Design

### 3.1 Bounds struct added to `Config`

The admin sets floor/ceiling for each parameter. Proposers cannot exceed these
bounds. When a proposer provides no override, the global default from `Config`
is used.

```rust
/// Admin-set bounds for per-proposal parameter overrides.
/// Each parameter has an inclusive [min, max] range.
/// When min == max (or when no override is provided), only the global
/// default is used.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ParamBounds {
    pub min_voting_period: u32,
    pub max_voting_period: u32,
    pub min_timelock_delay: u32,
    pub max_timelock_delay: u32,
    pub min_quorum: u32,
    pub max_quorum: u32,
    pub min_approval_threshold_pct: u32,
    pub max_approval_threshold_pct: u32,
}
```

`Config` gains an optional `param_bounds: Option<ParamBounds>` field:

```rust
pub struct Config {
    pub voting_period: u32,
    pub timelock_delay: u32,
    pub quorum: u32,
    pub approval_threshold_pct: u32,
    /// Admin-set bounds for per-proposal overrides.
    /// None = no per-proposal overrides allowed (default, current behavior).
    pub param_bounds: Option<ParamBounds>,
}
```

`None` preserves full backward compatibility: proposers cannot provide
overrides, and behavior is identical to today.

### 3.2 Per-proposal override struct

A new optional parameter struct passed to `propose_upgrade`:

```rust
/// Optional per-proposal governance parameter overrides.
/// Each field is `None` to use the global config default, or `Some(value)`
/// to override.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProposalParams {
    pub voting_period: Option<u32>,
    pub timelock_delay: Option<u32>,
    pub quorum: Option<u32>,
    pub approval_threshold_pct: Option<u32>,
}
```

### 3.3 `propose_upgrade` signature change

```rust
pub fn propose_upgrade(
    env: Env,
    proposer: Address,
    target: Address,
    new_wasm_hash: BytesN<32>,
    title: String,
    description: String,
    metadata_hash: BytesN<32>,
    params: Option<ProposalParams>,   // NEW — None = use global config
) -> Result<u64, Error>
```

Passing `None` for `params` is identical to the current behavior. This is a
**breaking** signature change (existing callers must add `None` as the last
argument), but it is the least-disruptive approach because it keeps all
existing functionality intact.

### 3.4 Validation in `create_proposal`

When `params` is `Some(overrides)`:

1. If `config.param_bounds` is `None`, return a new error
   `PerProposalOverridesDisabled` — the admin has not enabled overrides.
2. For each `overrides.X` that is `Some(v)`:
   - Check `v >= bounds.min_X` and `v <= bounds.max_X`. On failure, return a
     new error `ParamOutOfBounds`.
   - Apply existing validity rules: `voting_period >= 1`,
     `timelock_delay >= 1`, `quorum >= 1`,
     `approval_threshold_pct` in `[1, 100]`.
3. For each `overrides.X` that is `None`, fall back to `config.X`.

The `snap_*` fields in `Proposal` are set to the resolved (post-override)
values, so the execution path in `finalize_voting` and `assert_timelock_passed`
requires no changes.

### 3.5 Bounds validation in `update_config` / `initialize`

When `config.param_bounds` is `Some(bounds)`:

- Each `bounds.min_X` must satisfy the same minimum as the existing
  `validate_config` check for that field (e.g., `min_voting_period >= 1`).
- Each `bounds.max_X >= bounds.min_X`.
- Each `bounds.max_X <= 100` for threshold, with no upper limit imposed for
  ledger-count parameters (the admin sets sensible ceilings).
- `bounds.min_approval_threshold_pct >= 1` and
  `bounds.max_approval_threshold_pct <= 100`.

Validation failures return the existing error codes where possible
(`InvalidVotingPeriod`, `InvalidTimelockDelay`, `InvalidQuorum`,
`InvalidThreshold`), plus a new `InvalidParamBounds` error for
min > max violations.

### 3.6 New error codes

| Code | Name | Trigger |
|------|------|---------|
| 25 | `PerProposalOverridesDisabled` | Proposer passed `Some(params)` but `config.param_bounds` is `None`. |
| 26 | `ParamOutOfBounds` | A per-proposal override value falls outside the admin-set `[min, max]` range. |
| 27 | `InvalidParamBounds` | A `ParamBounds` field has `min > max`. |

### 3.7 Abuse prevention

The critical concern is a proposer setting `quorum=1, threshold=1` to auto-pass
any proposal. This is prevented by:

1. The admin must opt in by setting `param_bounds` to `Some(...)`.
2. Every override is clamped to `[bounds.min_X, bounds.max_X]`. The admin
   controls both floors and ceilings.
3. If the admin sets `min_quorum = 10` and `max_quorum = 10`, quorum is
   effectively locked — per-proposal override adds no flexibility there.
4. The `param_bounds` themselves are validated: `min_X <= max_X`, and all
   minimums must satisfy the same sanity checks as the global config.
5. When `param_bounds` is `None`, override attempts fail immediately —
   proposers cannot bypass bounds even if they construct a `ProposalParams`.

The admin can tighten or widen bounds via `update_config` at any time. Changes
to `param_bounds` only affect future proposals; in-flight proposals use their
snapshotted `snap_*` values.

---

## 4. Files affected (when implemented)

- `contracts/upgrade-manager/src/types.rs` — add `ParamBounds`, `ProposalParams`,
  add `param_bounds: Option<ParamBounds>` to `Config`, add new error codes.
- `contracts/upgrade-manager/src/governance.rs` — bounds validation in
  `create_proposal`, bounds validation in `validate_config`.
- `contracts/upgrade-manager/src/lib.rs` — `propose_upgrade` gains `params`
  argument; `initialize` and `update_config` accept `param_bounds` as part of
  Config construction.
- `contracts/upgrade-manager/src/test.rs` — new tests (see §5).
- `README.md` — update Config Reference table (add `param_bounds`), API table
  (`propose_upgrade` signature), error-code table (codes 25–27).
- `docs/claims-audit.md` — add rows for new behavior.
- `_CHANGELOG.md` — append a dated entry per CONTRIBUTING.md.

---

## 5. Tests needed

| Test name | What it verifies |
|-----------|-----------------|
| `test_per_proposal_params_disabled_by_default` | Passing `Some(ProposalParams {...})` when `config.param_bounds` is `None` returns `PerProposalOverridesDisabled`. |
| `test_per_proposal_params_none_uses_global_config` | Passing `None` for params uses the global config values in the proposal's `snap_*` fields. |
| `test_per_proposal_voting_period_override` | Valid override for `voting_period`; proposal's `snap_voting_period` reflects the override, not the global config. |
| `test_per_proposal_timelock_delay_override` | Valid override for `timelock_delay`; `snap_timelock_delay` reflects the override. |
| `test_per_proposal_quorum_override` | Valid override for `quorum`; `snap_quorum` reflects the override. |
| `test_per_proposal_threshold_override` | Valid override for `approval_threshold_pct`; `snap_threshold_pct` reflects the override. |
| `test_per_proposal_all_overrides` | All four fields overridden simultaneously; all `snap_*` fields correct. |
| `test_per_proposal_partial_overrides` | Two fields overridden, two `None`; overridden fields match overrides, `None` fields match global config. |
| `test_per_proposal_quorum_below_min_fails` | Override below `bounds.min_quorum` returns `ParamOutOfBounds`. |
| `test_per_proposal_quorum_above_max_fails` | Override above `bounds.max_quorum` returns `ParamOutOfBounds`. |
| `test_per_proposal_threshold_below_min_fails` | Override below `bounds.min_approval_threshold_pct` returns `ParamOutOfBounds`. |
| `test_per_proposal_threshold_above_max_fails` | Override above `bounds.max_approval_threshold_pct` returns `ParamOutOfBounds`. |
| `test_per_proposal_voting_period_zero_fails` | Override of `0` for `voting_period` returns existing `InvalidVotingPeriod` (or `ParamOutOfBounds` if min=1). |
| `test_param_bounds_min_greater_than_max_fails` | Setting `min_quorum > max_quorum` in `update_config` returns `InvalidParamBounds`. |
| `test_param_bounds_zero_min_quorum_fails` | Setting `min_quorum = 0` in `update_config` returns `InvalidQuorum`. |
| `test_param_bounds_threshold_max_above_100_fails` | Setting `max_approval_threshold_pct = 101` returns `InvalidThreshold`. |
| `test_param_bounds_update_does_not_affect_inflight_proposal` | Update `param_bounds` after a proposal is created; proposal's `snap_*` fields are unchanged. |
| `test_per_proposal_emergency_short_timelock` | Proposal with `timelock_delay = bounds.min_timelock_delay` (short) passes through the full governance flow and executes correctly. |
| `test_per_proposal_strict_quorum_at_max_bound` | Proposal with `quorum = bounds.max_quorum`; exact vote count passes. |

---

## 6. Out of scope

- Per-proposal `param_bounds` themselves (bounds are global, set by admin only).
- Delegation or vote-weight changes — per-proposal params affect pass/fail
  criteria only, not how votes are counted.
- No implementation code in this PR — this document is the design proposal
  only. Implementation proceeds only after maintainer approval.
