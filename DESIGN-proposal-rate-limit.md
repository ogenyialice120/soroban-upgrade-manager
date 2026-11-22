# Design: Configurable Proposal Creation Rate Limit

**Issue:** #118  
**Branch:** `design/proposal-rate-limit-118`  
**Status:** Design proposal — awaiting maintainer approval before code is written.

---

## Problem

`create_proposal` has no rate limit. Any authenticated address can submit proposals continuously, turning governance into a spam target. The README's "Security Model & Limitations" section acknowledges "No proposal deposit — spam is possible."

The touched guarantee: **anyone can create unlimited proposals at any ledger.**

---

## Proposed Design

### Per-address rate limit

A **per-address** rate limit is preferred over a global one. A global limit would let a single spammer block all other proposers; per-address limits cap the damage to the spammer's own addresses.

### Storage

Add a new persistent storage key per proposer:

```
DataKey::LastProposalLedger(Address) -> u32
```

This stores the ledger sequence of the proposer's most recent successful proposal. It is written in `create_proposal` after all other validations pass.

### New config field

```rust
pub struct Config {
    // ... existing fields ...
    /// Minimum ledger gap between successive proposals from the same address.
    /// Set to 0 to disable the rate limit (unlimited proposals allowed).
    /// At ~5 s/ledger, 720 ledgers ≈ 1 hour.
    pub proposal_min_interval: u32,
}
```

`proposal_min_interval = 0` disables the check entirely, preserving the current behaviour when deployers do not want a rate limit.

The field is a global config parameter (not snapshotted into proposals, because it only affects proposal creation, not an in-flight proposal's outcome).

### New DataKey variant

```rust
/// Last proposal ledger per proposer: (proposer)
LastProposalLedger(Address),
```

### Logic in `create_proposal`

Insert before the `load_config` call:

```rust
let config = load_config(env)?;
if config.proposal_min_interval > 0 {
    let key = DataKey::LastProposalLedger(proposer.clone());
    if let Some(last) = env.storage().persistent().get::<DataKey, u32>(&key) {
        let gap = env.ledger().sequence().saturating_sub(last);
        if gap < config.proposal_min_interval {
            return Err(Error::ProposalRateLimitExceeded);
        }
    }
}
// ... rest of create_proposal ...
// At the end, record the last-proposal ledger:
let key = DataKey::LastProposalLedger(proposer.clone());
env.storage().persistent().set(&key, &env.ledger().sequence());
env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
```

The write happens **after** the proposal is successfully saved so that a failed proposal (e.g. `EmptyTitle`) does not burn the proposer's rate-limit window.

### New error

```rust
/// Proposer has submitted a proposal too recently; must wait min_interval ledgers.
ProposalRateLimitExceeded = 25,
```

---

## Rationale: per-address vs global

| Dimension | Per-address | Global |
|---|---|---|
| Blast radius | One spammer blocks only themselves | One spammer blocks everyone |
| Sybil resistance | None — attacker uses fresh addresses | None — attacker uses fresh addresses |
| Legitimate proposers blocked | No | Yes, if attacker submits first |
| Implementation complexity | One key per address | One shared counter |

Per-address is strictly better for legitimate proposers. Sybil resistance is out of scope for both approaches (acknowledged in the README).

---

## Existing guarantee touched

**Guarantee:** Any address can create proposals at any time with no cooldown.  
**Effect:** An address that created a proposal within the last `proposal_min_interval` ledgers receives `ProposalRateLimitExceeded` until the interval elapses.

---

## Storage cost

One `u32` per proposer in persistent storage. Each write costs one Soroban storage entry. The TTL is `PERSISTENT_TTL_BUMP` (≈ 150 days), same as proposal entries.

---

## Tests needed

| Test | What it verifies |
|---|---|
| `test_rate_limit_allows_first_proposal` | First proposal from an address always succeeds regardless of `proposal_min_interval` |
| `test_rate_limit_blocks_immediate_second_proposal` | Second proposal within the interval returns `ProposalRateLimitExceeded` |
| `test_rate_limit_allows_after_interval_passes` | Second proposal after `proposal_min_interval` ledgers have elapsed succeeds |
| `test_rate_limit_independent_per_address` | Address A being rate-limited does not affect address B |
| `test_rate_limit_zero_disables_check` | `proposal_min_interval = 0` allows unlimited proposals from one address |
| `test_rate_limit_exact_boundary` | Proposal at exactly `proposal_min_interval` ledgers after the last one succeeds |
| `test_rate_limit_one_before_boundary` | Proposal at `proposal_min_interval - 1` ledgers returns `ProposalRateLimitExceeded` |
| `test_rate_limit_failed_proposal_does_not_set_window` | A proposal that fails validation (e.g. `EmptyTitle`) does not reset the rate-limit window |
| `test_rate_limit_update_config_changes_interval` | `update_config` with a new `proposal_min_interval` takes effect immediately for future proposals |
| `test_last_proposal_ledger_ttl_extended` | `DataKey::LastProposalLedger` entry has TTL ≥ `PERSISTENT_TTL_BUMP` after a proposal |

---

## Out of scope

- Global rate limit.
- Proposal deposit as an alternative spam prevention mechanism (separate issue).
- Restricting who can propose (e.g. allowlist). This design does not change the "anyone can propose" property; it only throttles the rate.
