# Events Reference

The upgrade manager emits a Soroban event on every state transition. Each
event is published with a two-symbol topic tuple and a data value. The topic
symbols are created with `symbol_short!()` from `soroban_sdk`.

All `emit_*` functions and the inline `env.events().publish()` calls are in
`contracts/upgrade-manager/src/governance.rs`.

---

## 1. `initialized`

Emitted when the contract is successfully initialized.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("contract"), symbol_short!("init"))` |
| **Data** | `Address` — the admin address set during initialization |
| **Emitted by** | `emit_initialized` called from `initialize` in `lib.rs` |
| **When** | Once, on the first successful call to `initialize` |

**Verified by test:** `test_event_initialized`

---

## 2. `config_updated`

Emitted when the governance configuration is changed by the admin.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("config"), symbol_short!("updated"))` |
| **Data** | `Address` — the admin address that called `update_config` |
| **Emitted by** | `emit_config_updated` called from `update_config` in `lib.rs` |
| **When** | On every successful `update_config` call |

**Verified by test:** `test_event_config_updated`

---

## 3. `admin_transferred`

Emitted when the admin role is transferred to a new address.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("admin"), symbol_short!("transfer"))` |
| **Data** | `(Address, Address)` — `(old_admin, new_admin)` |
| **Emitted by** | `emit_admin_transferred` called from `transfer_admin` in `lib.rs` |
| **When** | On every successful `transfer_admin` call |

**Verified by test:** `test_event_admin_transferred`

---

## 4. `proposal_created`

Emitted when a new upgrade proposal is created.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("proposal"), symbol_short!("created"))` |
| **Data** | `(u64, Address, Address, BytesN<32>)` — `(proposal_id, proposer, target, new_wasm_hash)` |
| **Emitted by** | Inline `env.events().publish()` in `create_proposal` in `governance.rs` |
| **When** | On every successful `propose_upgrade` call |

**Verified by test:** `test_event_proposal_created`

---

## 5. `vote_cast`

Emitted when a vote is cast on a proposal.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("vote"), symbol_short!("cast"))` |
| **Data** | `(u64, Address, bool, u32)` — `(proposal_id, voter, approve, weight)` |
| **Emitted by** | Inline `env.events().publish()` in `cast_vote` in `governance.rs` |
| **When** | On every successful `vote` call |

`weight` is `1 + delegation_count(voter)`. For a voter with no delegators it
is always `1`.

**Verified by test:** `test_event_vote_cast`

---

## 6. `finalized`

Emitted when voting is finalized and the proposal moves to `Queued` or
`Defeated`.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("proposal"), symbol_short!("finalized"))` |
| **Data** | `(u64, ProposalStatus)` — `(proposal_id, status)` where `status` is `Queued` or `Defeated` |
| **Emitted by** | Inline `env.events().publish()` in `finalize_voting` in `governance.rs` |
| **When** | On every successful `finalize` call |

**Verified by test:** `test_event_finalized`

---

## 7. `executed`

Emitted when a queued proposal is executed (the WASM upgrade is triggered).

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("proposal"), symbol_short!("executed"))` |
| **Data** | `u64` — the proposal ID |
| **Emitted by** | `emit_executed` called from `execute` in `lib.rs` |
| **When** | On every successful `execute` call, before the cross-contract `invoke_upgrade` call |

The event is emitted before `invoke_upgrade` to follow the
checks-effects-interactions pattern. If `invoke_upgrade` fails, the
transaction reverts and the event is not persisted.

---

## 8. `cancelled`

Emitted when the admin cancels an `Active` or `Queued` proposal.

| Field | Value |
|---|---|
| **Topic** | `(symbol_short!("proposal"), symbol_short!("cancelled"))` |
| **Data** | `(u64, Address)` — `(proposal_id, admin_address)` |
| **Emitted by** | Inline `env.events().publish()` in `cancel_proposal` in `governance.rs` |
| **When** | On every successful `cancel` call |

**Verified by test:** `test_event_cancelled`

---

## Quick reference

| Event | Topic symbols | Data type |
|---|---|---|
| initialized | `("contract", "init")` | `Address` |
| config_updated | `("config", "updated")` | `Address` |
| admin_transferred | `("admin", "transfer")` | `(Address, Address)` |
| proposal_created | `("proposal", "created")` | `(u64, Address, Address, BytesN<32>)` |
| vote_cast | `("vote", "cast")` | `(u64, Address, bool, u32)` |
| finalized | `("proposal", "finalized")` | `(u64, ProposalStatus)` |
| executed | `("proposal", "executed")` | `u64` |
| cancelled | `("proposal", "cancelled")` | `(u64, Address)` |

## See also

- [contracts/upgrade-manager/src/governance.rs](../contracts/upgrade-manager/src/governance.rs) — all event emit calls
- [contracts/upgrade-manager/src/lib.rs](../contracts/upgrade-manager/src/lib.rs) — `execute` and admin entry points
- [contracts/upgrade-manager/src/types.rs](../contracts/upgrade-manager/src/types.rs) — `ProposalStatus` enum
