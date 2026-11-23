# API Reference

This page documents every public function in the `UpgradeManager` contract.
Each entry lists the exact parameter types from
`contracts/upgrade-manager/src/lib.rs`, the return type, every error variant
the function can return (with its numeric code from `types.rs`), auth
requirements, and a `stellar contract invoke` example.

Stellar type mapping used throughout: `Address` → Strkey string
(`G…` for accounts, `C…` for contracts); `BytesN<32>` → 64-character lowercase
hex string; `String` → plain UTF-8 text; `u32`/`u64` → integer.

---

## Table of Contents

- [Initialization](#initialization)
  - [initialize](#initialize)
- [Admin Functions](#admin-functions)
  - [update\_config](#update_config)
  - [transfer\_admin](#transfer_admin)
- [Proposal Lifecycle](#proposal-lifecycle)
  - [propose\_upgrade](#propose_upgrade)
  - [vote](#vote)
  - [delegate](#delegate)
  - [revoke\_delegation](#revoke_delegation)
  - [finalize](#finalize)
  - [execute](#execute)
  - [cancel](#cancel)
- [Read-only Queries](#read-only-queries)
  - [get\_proposal](#get_proposal)
  - [get\_proposals](#get_proposals)
  - [get\_config](#get_config)
  - [get\_admin](#get_admin)
  - [time\_until\_executable](#time_until_executable)
- [Error Reference](#error-reference)

---

## Initialization

### initialize

**Category**: Initialization

**Auth**: Admin must sign — `admin.require_auth()` is called before any state
is written, preventing front-running.

**Description**: Initialises the contract with an admin address and governance
configuration. Can only be called once; a second call returns
`AlreadyInitialized`. The admin address, voting parameters, and timelock delay
are stored in instance storage. An `initialized` event is emitted on success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `admin` | `Address` | Address that can cancel proposals, update config, and transfer admin. Must sign. |
| `voting_period` | `u32` | Ledgers during which votes are accepted. Must be ≥ 1. At ~5 s/ledger, 17 280 ledgers ≈ 24 h. |
| `timelock_delay` | `u32` | Ledgers between finalization and earliest execution. Must be ≥ 1. |
| `quorum` | `u32` | Minimum total votes (YES + NO) needed for a proposal to pass. Must be ≥ 1. |
| `approval_threshold_pct` | `u32` | Percentage of YES/(YES+NO) required to pass (1–100). E.g., 51 = simple majority. |
| `proposal_min_interval` | `u32` | Minimum ledgers between successive proposals from the same address. 0 disables the rate limit. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `AlreadyInitialized` | 1 | A `Config` entry already exists in instance storage. |
| `InvalidVotingPeriod` | 19 | `voting_period` is 0. |
| `InvalidTimelockDelay` | 11 | `timelock_delay` is 0. |
| `InvalidQuorum` | 12 | `quorum` is 0. |
| `InvalidThreshold` | 10 | `approval_threshold_pct` is 0 or greater than 100. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_KEY" \
  --network testnet \
  -- initialize \
  --admin GADMIN_ADDRESS_HERE \
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51 \
  --proposal_min_interval 720
```

---

## Admin Functions

### update_config

**Category**: Admin

**Auth**: Admin must sign — `caller.require_auth()` is called inside
`require_admin`.

**Description**: Replaces the governance configuration. Only the current admin
can call this. Config changes apply to new proposals only; each existing
proposal snapshots its rules at creation time, so in-flight proposals are
unaffected. A `config_updated` event is emitted on success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `caller` | `Address` | Current admin address. Must sign and must equal the stored admin. |
| `voting_period` | `u32` | New voting period in ledgers. Must be ≥ 1. |
| `timelock_delay` | `u32` | New timelock delay in ledgers. Must be ≥ 1. |
| `quorum` | `u32` | New minimum total votes. Must be ≥ 1. |
| `approval_threshold_pct` | `u32` | New approval threshold percentage (1–100). |
| `proposal_min_interval` | `u32` | New per-address proposal interval. 0 disables rate limiting. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | No `Config` entry found; `initialize` has not been called. |
| `Unauthorized` | 2 | `caller` does not equal the stored admin. |
| `InvalidVotingPeriod` | 19 | `voting_period` is 0. |
| `InvalidTimelockDelay` | 11 | `timelock_delay` is 0. |
| `InvalidQuorum` | 12 | `quorum` is 0. |
| `InvalidThreshold` | 10 | `approval_threshold_pct` is 0 or greater than 100. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_KEY" \
  --network testnet \
  -- update_config \
  --caller GADMIN_ADDRESS_HERE \
  --voting_period 34560 \
  --timelock_delay 17280 \
  --quorum 5 \
  --approval_threshold_pct 66 \
  --proposal_min_interval 1440
```

---

### transfer_admin

**Category**: Admin

**Auth**: Current admin must sign — `caller.require_auth()` is called inside
`require_admin`.

**Description**: Transfers the admin role to a new address. Only the current
admin can call this. The change takes effect immediately; the old admin loses
all privileged access. An `admin_transferred` event is emitted on success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `caller` | `Address` | Current admin address. Must sign and must equal the stored admin. |
| `new_admin` | `Address` | Address that will become the new admin. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | No `Admin` entry found; `initialize` has not been called. |
| `Unauthorized` | 2 | `caller` does not equal the stored admin. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_KEY" \
  --network testnet \
  -- transfer_admin \
  --caller GADMIN_ADDRESS_HERE \
  --new_admin GNEW_ADMIN_ADDRESS_HERE
```

---

## Proposal Lifecycle

### propose_upgrade

**Category**: Proposal Lifecycle

**Auth**: Proposer must sign — `proposer.require_auth()` is called before
creating the proposal.

**Description**: Creates a new upgrade proposal. Any authenticated address may
propose. The current governance config (voting period, timelock delay, quorum,
approval threshold) is snapshotted into the proposal so that a later
`update_config` cannot change the rules for this proposal. If
`proposal_min_interval > 0`, the proposer must wait that many ledgers since
their last successful proposal. A `proposal_created` event is emitted on
success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `proposer` | `Address` | Address creating the proposal. Must sign. |
| `target` | `Address` | Contract address to be upgraded. |
| `new_wasm_hash` | `BytesN<32>` | SHA-256 hash of the new WASM binary, already uploaded to the network. |
| `title` | `String` | Short title for the proposal. 1–128 bytes. |
| `description` | `String` | Human-readable description. 1–256 bytes. |
| `metadata_hash` | `BytesN<32>` | SHA-256 hash of off-chain metadata (changelog URI, compatibility report, etc.). Use all zeros if there is no off-chain metadata. |

**Returns**: `Result<u64, Error>` — the new proposal ID (starts at 0,
increments by 1).

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | Contract has not been initialised. |
| `EmptyTitle` | 20 | `title` is empty (zero bytes). |
| `TitleTooLong` | 21 | `title` exceeds 128 bytes. |
| `EmptyDescription` | 18 | `description` is empty (zero bytes). |
| `DescriptionTooLong` | 9 | `description` exceeds 256 bytes. |
| `ProposalRateLimitExceeded` | 25 | Proposer submitted another proposal before `proposal_min_interval` ledgers have elapsed. |
| `Overflow` | 15 | Ledger arithmetic overflow computing `voting_ends_at` or the proposal counter. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$PROPOSER_KEY" \
  --network testnet \
  -- propose_upgrade \
  --proposer GPROPOSER_ADDRESS_HERE \
  --target CTARGET_CONTRACT_ID_HERE \
  --new_wasm_hash <WASM_HASH_HEX> \
  --title "Upgrade to v2" \
  --description "Applies security patch described in CHANGELOG v2.0.0" \
  --metadata_hash 0000000000000000000000000000000000000000000000000000000000000000
```

---

### vote

**Category**: Proposal Lifecycle

**Auth**: Voter must sign — `voter.require_auth()` is called.

**Description**: Casts a YES or NO vote on an active proposal. Each address
may vote exactly once per proposal. Voting is accepted only while
`ledger.sequence() <= voting_ends_at`. If the voter has delegated their vote,
this call returns `HasDelegated` — the delegate must vote instead. If the voter
is a delegate, their vote carries a weight of 1 (themselves) plus the number
of addresses that have delegated to them. A `vote_cast` event is emitted on
success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `voter` | `Address` | Address casting the vote. Must sign. |
| `proposal_id` | `u64` | ID of the proposal to vote on. |
| `approve` | `bool` | `true` for YES, `false` for NO. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `ProposalNotFound` | 3 | No proposal exists with the given ID. |
| `VotingClosed` | 4 | Proposal status is not `Active` (already finalized, cancelled, or executed). |
| `VotingPeriodEnded` | 16 | `ledger.sequence() > voting_ends_at`; the voting window has closed. |
| `HasDelegated` | 22 | Voter has an active delegation; the delegate must vote on their behalf. |
| `AlreadyVoted` | 5 | This address has already voted on this proposal. |
| `Overflow` | 15 | Vote count arithmetic overflow. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$VOTER_KEY" \
  --network testnet \
  -- vote \
  --voter GVOTER_ADDRESS_HERE \
  --proposal_id 0 \
  --approve true
```

---

### delegate

**Category**: Proposal Lifecycle

**Auth**: Caller must sign — `caller.require_auth()` is called inside the
`delegate` helper.

**Description**: Delegates `caller`'s voting power to `delegate_to` for all
proposals. The delegate casts votes on the caller's behalf; the caller cannot
vote directly while delegated. Delegation is global (applies to all proposals)
and not transitive — a delegate who has themselves delegated cannot receive
further delegations. If `caller` already has an active delegation, it is
replaced atomically. A `delegated` event is emitted on success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `caller` | `Address` | Address delegating their vote. Must sign. |
| `delegate_to` | `Address` | Address that will receive the delegated voting power. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `CannotDelegateToSelf` | 23 | `caller` and `delegate_to` are the same address. |
| `HasDelegated` | 22 | `delegate_to` has itself delegated to someone else; transitive delegation is not permitted. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$CALLER_KEY" \
  --network testnet \
  -- delegate \
  --caller GCALLER_ADDRESS_HERE \
  --delegate_to GDELEGATE_ADDRESS_HERE
```

---

### revoke_delegation

**Category**: Proposal Lifecycle

**Auth**: Caller must sign — `caller.require_auth()` is called inside the
`revoke_delegation` helper.

**Description**: Revokes `caller`'s active delegation, restoring their ability
to vote directly on future proposals. The delegation count on the former
delegate is decremented. A `revoked` event is emitted on success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `caller` | `Address` | Address revoking their delegation. Must sign. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotDelegating` | 24 | `caller` has no active delegation; nothing to revoke. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$CALLER_KEY" \
  --network testnet \
  -- revoke_delegation \
  --caller GCALLER_ADDRESS_HERE
```

---

### finalize

**Category**: Proposal Lifecycle

**Auth**: Anyone — no authentication required.

**Description**: Closes voting on a proposal and transitions it to `Queued` or
`Defeated`. May be called by anyone once `ledger.sequence() > voting_ends_at`.
Pass conditions: total votes (YES + NO) ≥ snapshotted `quorum` AND
`yes_votes * 100 ≥ total_votes * snap_threshold_pct`. On pass,
`executable_at` is set to `current_ledger + snap_timelock_delay` and status
becomes `Queued`. On fail, status becomes `Defeated`. A `finalized` event is
emitted on success.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `proposal_id` | `u64` | ID of the proposal to finalize. |

**Returns**: `Result<ProposalStatus, Error>` — the new status (`Queued` or
`Defeated`).

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `ProposalNotFound` | 3 | No proposal exists with the given ID. |
| `VotingClosed` | 4 | Proposal status is not `Active`. |
| `VotingStillActive` | 17 | `ledger.sequence() <= voting_ends_at`; the voting period has not ended yet. |
| `Overflow` | 15 | Ledger arithmetic overflow computing `executable_at`. |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- finalize \
  --proposal_id 0
```

---

### execute

**Category**: Proposal Lifecycle

**Auth**: Anyone — no authentication required.

**Description**: Executes a queued proposal by calling `upgrade(new_wasm_hash)`
on the target contract via cross-contract invocation. Can be called by anyone
after `ledger.sequence() >= executable_at`. The proposal status is set to
`Executed` and saved **before** the cross-contract call
(checks-effects-interactions pattern). If the target call fails, the entire
transaction reverts and the proposal returns to `Queued`. An `executed` event
is emitted before the cross-contract call. The target contract's `upgrade`
function must authenticate the upgrade-manager as its caller.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `proposal_id` | `u64` | ID of the queued proposal to execute. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `ProposalNotFound` | 3 | No proposal exists with the given ID. |
| `NotQueued` | 6 | Proposal status is not `Queued`. |
| `TimelockNotExpired` | 7 | `ledger.sequence() < executable_at`; the timelock has not elapsed. |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- execute \
  --proposal_id 0
```

---

### cancel

**Category**: Proposal Lifecycle

**Auth**: Admin must sign — `caller.require_auth()` is called inside
`require_admin`.

**Description**: Cancels an `Active` or `Queued` proposal. Only the admin can
call this. A `cancelled` event is emitted on success. Proposals that are
already `Executed`, `Cancelled`, or `Defeated` cannot be cancelled.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `caller` | `Address` | Current admin address. Must sign and must equal the stored admin. |
| `proposal_id` | `u64` | ID of the proposal to cancel. |

**Returns**: `Result<(), Error>` — `Ok(())` on success.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | Contract has not been initialised. |
| `Unauthorized` | 2 | `caller` does not equal the stored admin. |
| `ProposalNotFound` | 3 | No proposal exists with the given ID. |
| `ProposalFinalized` | 13 | Proposal is already `Executed`, `Cancelled`, or `Defeated`. |

**CLI Example**:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_KEY" \
  --network testnet \
  -- cancel \
  --caller GADMIN_ADDRESS_HERE \
  --proposal_id 0
```

---

## Read-only Queries

### get_proposal

**Category**: Read-only

**Auth**: Anyone — no authentication required.

**Description**: Returns the full `Proposal` struct for the given ID. Reading
the proposal also extends its persistent storage TTL.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `proposal_id` | `u64` | ID of the proposal to retrieve. |

**Returns**: `Result<Proposal, Error>` — the `Proposal` struct, which contains:
`id`, `target`, `new_wasm_hash`, `created_at`, `voting_ends_at`,
`executable_at` (0 until finalized), `yes_votes`, `no_votes`, `status`,
`proposer`, `title`, `description`, `metadata_hash`, and four snapshotted
governance parameters (`snap_voting_period`, `snap_timelock_delay`,
`snap_quorum`, `snap_threshold_pct`).

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `ProposalNotFound` | 3 | No proposal exists with the given ID. |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
```

---

### get_proposals

**Category**: Read-only

**Auth**: Anyone — no authentication required.

**Description**: Returns a paginated list of proposals in ascending ID order,
starting from `offset` (inclusive) and returning at most `limit` entries.
`limit` is capped at 50 (`MAX_PROPOSALS_PAGE_LIMIT`) to bound gas costs.
Returns an empty vector if `offset >= total proposal count` or if `limit` is 0.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `offset` | `u64` | Starting proposal ID (inclusive). |
| `limit` | `u32` | Maximum number of proposals to return. Capped at 50. |

**Returns**: `Result<Vec<Proposal>, Error>` — a vector of `Proposal` structs.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | Contract has not been initialised. |
| `ProposalNotFound` | 3 | A proposal ID in the requested range is missing from storage (should not occur in normal operation). |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- get_proposals \
  --offset 0 \
  --limit 10
```

---

### get_config

**Category**: Read-only

**Auth**: Anyone — no authentication required.

**Description**: Returns the current governance configuration. This reflects
the live config; individual proposals may use different values if the config
was changed after they were created (proposals snapshot the config at creation
time).

**Parameters**: None.

**Returns**: `Result<Config, Error>` — the `Config` struct with fields:
`voting_period`, `timelock_delay`, `quorum`, `approval_threshold_pct`,
`proposal_min_interval`.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | Contract has not been initialised. |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- get_config
```

---

### get_admin

**Category**: Read-only

**Auth**: Anyone — no authentication required.

**Description**: Returns the current admin address.

**Parameters**: None.

**Returns**: `Result<Address, Error>` — the admin's Strkey address.

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `NotInitialized` | 14 | Contract has not been initialised. |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- get_admin
```

---

### time_until_executable

**Category**: Read-only

**Auth**: Anyone — no authentication required.

**Description**: Returns the number of ledgers remaining until a queued
proposal's timelock expires and `execute` can be called. Returns `0` if the
timelock has already elapsed or if the proposal is not in the `Queued` state
(e.g., still `Active`, `Defeated`, `Executed`, or `Cancelled`). This function
does not modify any state.

**Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `proposal_id` | `u64` | ID of the proposal to query. |

**Returns**: `Result<u32, Error>` — ledgers remaining until `executable_at`
(`0` if execution is already permitted or the proposal is not `Queued`).

**Errors**:

| Error | Code | Trigger |
|-------|------|---------|
| `ProposalNotFound` | 3 | No proposal exists with the given ID. |

**CLI Example**:

```bash
# No authentication required; --source is used only to pay fees.
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$FEE_PAYER_KEY" \
  --network testnet \
  -- time_until_executable \
  --proposal_id 0
```

---

## Error Reference

All 25 error variants are listed in the `Error` enum in
`contracts/upgrade-manager/src/types.rs` with their `#[repr(u32)]` codes.
The README's error-code table cross-references each code with its trigger
condition and resolution guidance. The table also notes that `ProposalDefeated`
(code 8) is defined in the enum but is never returned — `finalize()` returns
`ProposalStatus::Defeated` as a success value rather than surfacing this error.
