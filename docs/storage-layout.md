# Storage Layout and TTL Reference

This page documents the contract's on-chain storage layout: which `DataKey`
variants are stored in instance vs persistent storage, the TTL bump constants,
when TTL is extended, and how long each key survives without activity.

Source of truth: `contracts/upgrade-manager/src/types.rs` (keys and constants)
and `contracts/upgrade-manager/src/governance.rs` (read/write helpers).

---

## TTL constants

Defined in `types.rs`:

| Constant | Value (ledgers) | Approximate duration |
|----------|----------------|---------------------|
| `INSTANCE_TTL_BUMP` | `518_400` | ≈ 30 days at ~5 s/ledger |
| `PERSISTENT_TTL_BUMP` | `2_592_000` | ≈ 150 days at ~5 s/ledger |

The 150-day persistent TTL is long enough to outlive any realistic proposal
lifecycle (voting period + timelock delay + execution window).

---

## DataKey variants and storage types

| DataKey variant | Storage type | TTL constant | TTL extended on |
|-----------------|-------------|--------------|-----------------|
| `Admin` | Instance | `INSTANCE_TTL_BUMP` | Write (`save_admin`) |
| `Config` | Instance | `INSTANCE_TTL_BUMP` | Write (`save_config`) |
| `ProposalCount` | Instance | `INSTANCE_TTL_BUMP` | Write (`next_proposal_id`) |
| `Proposal(u64)` | Persistent | `PERSISTENT_TTL_BUMP` | Write (`save_proposal`) and read (`load_proposal`) |
| `Vote(u64, Address)` | Persistent | `PERSISTENT_TTL_BUMP` | Write (`record_vote`) |
| `Delegation(Address)` | Persistent | `PERSISTENT_TTL_BUMP` | Write (`save_delegation`) |
| `DelegationCount(Address)` | Persistent | `PERSISTENT_TTL_BUMP` | Write (`increment_delegation_count`, `decrement_delegation_count`) |

---

## Per-key details

### `Admin` — Instance storage

Stores the admin `Address`. Written by `save_admin` (called from `initialize`
and `transfer_admin`). Read by `load_admin` (called by `require_admin` on every
admin-gated function). TTL is bumped to `INSTANCE_TTL_BUMP` on every write.

### `Config` — Instance storage

Stores the `Config` struct (`voting_period`, `timelock_delay`, `quorum`,
`approval_threshold_pct`). Written by `save_config` (called from `initialize`
and `update_config`). Read by `load_config` (called by `create_proposal` and
`get_config`). TTL is bumped to `INSTANCE_TTL_BUMP` on every write.

### `ProposalCount` — Instance storage

Stores a `u64` counter tracking the next proposal ID. Written by
`next_proposal_id` (called from `create_proposal`). TTL is bumped to
`INSTANCE_TTL_BUMP` on every write. Instance storage TTL is also bumped inside
`save_proposal`, keeping `ProposalCount` alive as long as proposals are being
written.

### `Proposal(u64)` — Persistent storage

Stores the full `Proposal` struct keyed by proposal ID. Written by
`save_proposal` (called from `create_proposal`, `cast_vote`, `finalize_voting`,
`cancel_proposal`, and `execute`). Read by `load_proposal` (called by any
function that inspects or mutates a proposal). TTL is bumped to
`PERSISTENT_TTL_BUMP` on **both write and read**, ensuring that an active
proposal is not evicted while it is being accessed.

### `Vote(u64, Address)` — Persistent storage

Stores a `bool` (`true`) recording that a specific voter has voted on a specific
proposal. Written by `record_vote` (called from `cast_vote`). Read (existence
check only) by `has_voted`. TTL is bumped to `PERSISTENT_TTL_BUMP` on write.
Vote records must outlive the proposal to prevent double-voting after a storage
eviction and re-creation scenario.

### `Delegation(Address)` — Persistent storage

Stores the delegate `Address` that a voter has delegated to. Written by
`save_delegation` (called from `delegate`). Removed by `remove_delegation`
(called from `revoke_delegation`). Read by `load_delegation`. TTL is bumped to
`PERSISTENT_TTL_BUMP` on write.

### `DelegationCount(Address)` — Persistent storage

Stores a `u32` counter of how many voters have delegated to a given delegate.
Written by `increment_delegation_count` and `decrement_delegation_count` (called
from `delegate` and `revoke_delegation`). Read by `delegation_count` (called
from `cast_vote` to compute vote weight). TTL is bumped to `PERSISTENT_TTL_BUMP`
on write.

---

## When TTL is extended

The contract extends TTL proactively so that storage entries do not expire
during normal use:

- **On every write** to any storage key — the corresponding `extend_ttl` call
  immediately follows the `set` call in every `save_*` and `record_*` helper.
- **On read of `Proposal(u64)`** — `load_proposal` calls `extend_ttl` after
  retrieving the proposal. This ensures that frequently-read proposals (e.g.,
  one being voted on over many ledgers) stay alive without requiring a write.
- Instance storage TTL is also bumped inside `save_proposal` (via
  `env.storage().instance().extend_ttl`) to keep `Admin`, `Config`, and
  `ProposalCount` alive while proposals are active.

No TTL extension occurs on reads of `Admin`, `Config`, `Vote`, `Delegation`, or
`DelegationCount`. These entries are only refreshed when written.

---

## Storage type rationale

**Instance storage** is used for singleton contract-level data (`Admin`,
`Config`, `ProposalCount`) that is accessed on almost every transaction. Instance
storage has a shorter TTL budget but lower per-entry cost.

**Persistent storage** is used for per-proposal and per-voter data (`Proposal`,
`Vote`, `Delegation`, `DelegationCount`) that grows with the number of proposals
and voters. Persistent storage supports a longer TTL (150 days here), making it
suitable for data that must survive long proposal lifecycles.
