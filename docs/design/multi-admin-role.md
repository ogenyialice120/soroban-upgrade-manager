# Design: Configurable Multi-Admin Role (Issue #121)

**Status:** Proposed — awaiting maintainer approval before any code is written.

**Related issue:** #121
**Branch:** `design/multi-admin-role`
**Type:** Manager contract authorization model
**Date:** 2026-11-22

---

## 1. Problem

The contract has a single admin. `require_admin` (in `governance.rs`) checks one
stored address; `save_admin` stores one address; `transfer_admin` moves it. The
README already recommends that "a multisig or DAO should hold the admin role,"
but nothing enforces that on-chain — a single compromised key can cancel
proposals, change config, and transfer the admin role.

A multi-admin model with M-of-N consensus for sensitive actions would reduce
single-key risk and make the recommendation first-class.

## 2. Existing guarantee touched

> **Single admin.** `require_admin(caller)` authenticates `caller` and compares
> it to one stored `DataKey::Admin`; `transfer_admin` replaces that one
> address. Admin-only actions (`cancel`, `update_config`, `transfer_admin`)
> require exactly that one signature.

This design replaces "one admin signs" with "M of N admins approve." Two
implementations are presented; the guarantee changes differ:

- **Option A (off-chain):** the on-chain guarantee is unchanged (still one admin
  address); the M-of-N enforcement moves to the Stellar account level.
- **Option B (in-contract):** the on-chain guarantee changes from one stored
  address to a stored admin *set* with an M-of-N approval flow. This is a
  breaking change to `initialize` / `transfer_admin` signatures and to
  `DataKey::Admin` storage.

## 3. Design

Two options are evaluated; **Option A is recommended as the default** and
**Option B as an opt-in** for the cases A cannot cover. The maintainer picks one
(or A-now / B-later).

### 3.1 Option A — off-chain (Stellar account multisig) [recommended default]

Keep the contract exactly as-is. The single admin `Address` is a **Stellar
account** configured with M-of-N signers using Stellar's native account-level
multisig (master weight + signer weights + thresholds for low/med/high
operations). `require_auth()` already demands the admin sign; with account
multisig the network enforces M-of-N at the account level, and the contract
still sees only the single admin address.

- **Pros:** zero contract change; uses battle-tested Stellar multisig; no
  on-chain complexity, no extra storage, no drift; works today and matches the
  README's existing guidance.
- **Cons:** only works when the admin is an EOA (a regular account). If the
  admin must be a contract (e.g. a DAO) or the M-of-N policy must be auditable
  and enforced on-chain regardless of key custody, account multisig does not
  apply. Admin-set membership lives off-chain and is not queryable from the
  contract.

This is the simplest thing that satisfies the hint ("off-chain multisig is
simpler and sufficient") for the common case.

### 3.2 Option B — in-contract M-of-N admin set [opt-in for on-chain enforcement]

For teams that need the policy enforced on-chain or whose admin is a
contract/DAO, implement M-of-N inside the contract.

**Why in-contract and not Soroban "native" multi-sig?** Soroban's native
multi-sig is *account-level* (the network checks signers for an account), not
*per-contract-action*. There is no host primitive for "M of these N specific
addresses must approve *this* contract call." So in-contract M-of-N requires an
**approval accumulator**: a proposed action accumulates approvals from distinct
admins until it reaches the threshold, then executes.

#### 3.2.1 Storage

```rust
#[contracttype]
pub enum DataKey {
    ...
    /// The admin set (membership). N = map.len().
    Admins,            // Map<Address, bool>  (replaces DataKey::Admin)
    /// M: how many distinct admin approvals an admin action needs.
    AdminThreshold,    // u32
    /// A pending admin action awaiting M-of-N approvals.
    AdminAction(u64),
    /// Next admin-action ID.
    AdminActionCount,   // u64
}

#[contracttype]
pub struct AdminAction {
    pub id: u64,
    pub kind: AdminActionKind,   // what to do once approved
    pub approvals: Map<Address, bool>, // which admins approved
    pub created_at: u32,
    pub executed: bool,
}

#[contracttype]
pub enum AdminActionKind {
    CancelProposal(u64),
    UpdateConfig(Config),
    AddAdmin(Address),
    RemoveAdmin(Address),
    SetThreshold(u32),
}
```

`Admins` replaces the single `Admin` entry. N is derived from `Admins.len()`.
`AdminThreshold` (M) is configurable, `1 <= M <= N`, `N >= 1` always.

#### 3.2.2 Initialization

`initialize(env, admins: Vec<Address>, threshold: u32, config...)`:
- validate `admins` is non-empty, has no duplicates, and `1 <= threshold <= admins.len()`;
- store `Admins` (as a `Map`) and `AdminThreshold`;
- store config and emit `initialized` (with the admin set, not one address).

(Backward compatibility: a migration could accept the legacy single admin and
seed a 1-of-1 set, but that is out of scope here.)

#### 3.2.3 The M-of-N approval flow

```text
propose_admin_action(caller, kind)  -> action_id   // caller must be an admin; records caller's approval
approve_admin_action(caller, id)                  // caller must be an admin; adds caller's approval (idempotent)
execute_admin_action(id)                          // anyone calls; applies kind iff approvals.len() >= M; marks executed
```

- `caller.require_auth()` on propose/approve; membership check against `Admins`.
- One approval per admin per action (`approvals[caller] = true`; re-approving is
  a no-op, not a second vote).
- `execute_admin_action` is open (anyone) but only takes effect once
  `approvals.len() >= AdminThreshold`; otherwise it reverts. This matches the
  pattern of `finalize`/`execute` (anyone may call; the on-chain state decides).

#### 3.2.4 Which actions need M-of-N vs. single-admin?

Under Option B **all** current admin actions move behind M-of-N:

| Action | Today | Option B |
|---|---|---|
| `cancel` | single admin | M-of-N (`AdminActionKind::CancelProposal`) |
| `update_config` | single admin | M-of-N (`UpdateConfig`) |
| admin-set mutation (`add`/`remove` admin, `set_threshold`) | n/a (`transfer_admin`) | M-of-N (`AddAdmin`/`RemoveAdmin`/`SetThreshold`) |
| read-only (`get_config`, `get_admin`, `get_proposal`, …) | anyone | anyone (unchanged) |

Rationale: the whole point is that no single key can do a sensitive thing.
Read-only queries stay open. Proposing an admin action is restricted to admins
(so a non-admin cannot spam the action log; spam is bounded and each action
needs M approvals to do anything anyway).

#### 3.2.5 Admin-set mutation safety

- `AddAdmin(addr)`: reject if `addr` already in `Admins` (no duplicates). After
  adding, `N` grows; if `M` would exceed new `N` it must be re-set first.
- `RemoveAdmin(addr)`: reject if it would make `N < 1` (never remove the last
  admin) or `N < M` (never drop below the threshold). An admin may remove
  themselves as long as those invariants hold.
- `SetThreshold(m)`: reject unless `1 <= m <= N`.
- Because mutations themselves require M-of-N, a minority cannot pack the admin
  set or lower the threshold to seize control.

#### 3.2.6 Action expiry

Each `AdminAction` is persistent with the existing `PERSISTENT_TTL_BUMP`. An
optional `expires_at` (created_at + a configurable admin-action window) lets
stale proposals lapse; `execute_admin_action` rejects expired actions. v1 may
omit expiry (rely on TTL eviction) to keep it minimal — listed as a follow-up.

### 3.3 Recommendation

- **Default: Option A.** For the common case (admin is an EOA), off-chain
  account multisig is simpler, sufficient, and needs no contract change — it
  already works with the current contract and matches the README's guidance.
- **Option B when** the admin must be a contract/DAO, or the M-of-N policy must
  be enforced and auditable on-chain regardless of key custody. Accept the
  added complexity (approval flow + storage). It can ship later without
  disturbing Option A users by introducing the new entrypoints alongside the
  existing single-admin path, then deprecating the latter.

Choosing A-now / B-later is viable because B is additive to the authorization
model, not a fork of the proposal lifecycle.

## 4. Tests needed

For **Option A** the contract is unchanged, so tests are operational
(account-level signer configuration), not contract tests; document the expected
account setup.

For **Option B**:

1. `initialize` with an admin set + threshold; reject empty set, duplicates,
   `threshold < 1`, `threshold > N`.
2. Propose an admin action by an admin; the proposer's approval is recorded.
3. Collect `M` distinct admin approvals; `execute_admin_action` applies the
   action (e.g. `CancelProposal` actually cancels; `UpdateConfig` actually
   changes config).
4. Fewer than `M` approvals → `execute_admin_action` reverts (no effect).
5. Non-admin `propose`/`approve` rejected.
6. Duplicate approval by the same admin does not double-count (one admin, one
   approval per action).
7. `AddAdmin`: rejects duplicates; new admin can then propose/approve.
8. `RemoveAdmin`: rejects removing the last admin and rejects dropping `N < M`;
   self-removal allowed when invariants hold.
9. `SetThreshold`: rejects `m < 1` and `m > N`.
10. Admin-set mutations themselves require M-of-N (a single admin cannot
    add/remove an admin or change the threshold).
11. Read-only getters remain open to anyone and unchanged.
12. (Optional) expired action cannot execute; TTL/`expires_at` behavior.

## 5. Out of scope

- Implementing both options simultaneously (one is chosen; A-now/B-later is the
  pragmatic path).
- Token-weighted admin voting (admins are equal weight, M-of-N by count).
- Changing the proposal *voter* model (this design is only about the admin
  role).
- Admin-action expiry as a v1 requirement (follow-up).
- Code: no contract code is written until this design is approved by a
  maintainer.
