# Governance Glossary

This glossary defines every governance term used in the Soroban Upgrade Manager.
Terms are grouped by theme: proposal lifecycle, governance parameters, proposal
statuses, storage concepts, Soroban platform concepts, and security patterns.
Each entry gives a plain definition, its meaning in this project, and the exact
code location where it is defined or enforced.

---

## Proposal Lifecycle

### Proposal

**Definition.** A request to replace the WASM bytecode of a target contract
with a new version, subject to a vote and a timelock delay.

**In this project.** A proposal is created by calling `propose_upgrade()`.
It records the target address, the hash of the new WASM already uploaded to
the network, a title, a description, an optional off-chain metadata hash, and
snapshotted copies of the governance parameters in force at creation time.
Every proposal has a numeric ID that increments from 0.

**Code reference.** `Proposal` struct —
`contracts/upgrade-manager/src/types.rs`.
Created by `create_proposal()` in `contracts/upgrade-manager/src/governance.rs`.
Exposed through `propose_upgrade()` in `contracts/upgrade-manager/src/lib.rs`.

---

### Proposer

**Definition.** The address that creates a governance proposal.

**In this project.** Any authenticated address may propose an upgrade; there
is no token or stake requirement. The proposer must sign the `propose_upgrade()`
transaction (`proposer.require_auth()` in `lib.rs`). The proposer's address is
stored in `Proposal.proposer` and emitted in the `proposal_created` event.

**Code reference.** `Proposal.proposer` field —
`contracts/upgrade-manager/src/types.rs`.
Auth check — `lib.rs` `propose_upgrade()`.

---

### Voter

**Definition.** An address that casts a YES or NO vote on an active proposal.

**In this project.** Voting is one-address-one-vote (not token-weighted). Any
authenticated address may vote exactly once per proposal, as long as the voting
period has not ended. If an address has delegated its vote to another address,
it cannot vote directly — its delegate votes on its behalf and the delegate's
vote weight is increased by the number of delegators.

**Code reference.** `cast_vote()` in
`contracts/upgrade-manager/src/governance.rs`.
Vote records stored under `DataKey::Vote(proposal_id, address)` in
`contracts/upgrade-manager/src/types.rs`.

---

### Voting Period

**Definition.** The window (measured in ledgers) during which votes may be
cast on an Active proposal.

**In this project.** Set in `Config.voting_period` and snapshotted into each
proposal as `Proposal.snap_voting_period` at creation time. The deadline is
`Proposal.voting_ends_at = created_at + snap_voting_period`. Votes are accepted
while `ledger.sequence() <= voting_ends_at`; `finalize()` is only allowed after
`voting_ends_at`. Changing `Config` after a proposal is created does not affect
the proposal's deadline.

**Code reference.** `Config.voting_period` and `Proposal.voting_ends_at`,
`Proposal.snap_voting_period` —
`contracts/upgrade-manager/src/types.rs`.
Voting deadline enforced in `cast_vote()` and `finalize_voting()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Timelock Delay

**Definition.** A mandatory wait period (measured in ledgers) between the
moment a proposal passes its vote and the earliest point at which execution
is permitted.

**In this project.** Set in `Config.timelock_delay` and snapshotted into each
proposal as `Proposal.snap_timelock_delay`. The timelock starts at
*finalization*, not at creation. When `finalize()` is called on a passing
proposal, `Proposal.executable_at` is set to `finalize_ledger + snap_timelock_delay`.
Execution is blocked until `ledger.sequence() >= executable_at`. This gives
the community time to react (e.g., cancel via admin) before an approved upgrade
is irreversibly applied.

**Code reference.** `Config.timelock_delay`, `Proposal.executable_at`,
`Proposal.snap_timelock_delay` —
`contracts/upgrade-manager/src/types.rs`.
`executable_at` set in `finalize_voting()` —
`contracts/upgrade-manager/src/governance.rs`.
Enforced by `assert_timelock_passed()` —
`contracts/upgrade-manager/src/timelock.rs`.

---

### Cross-Contract Invocation

**Definition.** A call from one Soroban contract to a function on another
contract during the same transaction.

**In this project.** When `execute()` is called, the upgrade manager invokes
`target.upgrade(new_wasm_hash)` on the target contract via
`env.invoke_contract()`. This cross-contract call is what actually replaces the
target's WASM bytecode. The target contract must implement an `upgrade()`
function that authenticates the manager and calls
`env.deployer().update_current_contract_wasm(hash)`.

**Code reference.** `invoke_upgrade()` in
`contracts/upgrade-manager/src/governance.rs`.
Called from `execute()` in `contracts/upgrade-manager/src/lib.rs`.
See also `docs/checks-effects-interactions.md`.

---

### WASM Hash

**Definition.** The SHA-256 hash of a compiled WASM binary that has been
uploaded to the Stellar network's ledger.

**In this project.** A proposer uploads the new contract binary to the network
before creating a proposal, then supplies the resulting 32-byte hash as the
`new_wasm_hash` argument to `propose_upgrade()`. The hash is stored in
`Proposal.new_wasm_hash` and passed through to `invoke_upgrade()` at execution
time. The network uses this hash to look up and load the WASM; if the hash does
not correspond to an uploaded binary, the upgrade call fails.

**Code reference.** `Proposal.new_wasm_hash: BytesN<32>` —
`contracts/upgrade-manager/src/types.rs`.
Passed to `invoke_upgrade()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Upgrade

**Definition.** The act of replacing a deployed contract's WASM bytecode with
a new version while preserving its on-chain address and storage.

**In this project.** The upgrade manager orchestrates upgrades through the full
governance lifecycle: propose → vote → finalize → execute. The actual bytecode
replacement happens inside the target contract's `upgrade()` function
(via `env.deployer().update_current_contract_wasm(hash)`), triggered by a
cross-contract call from `execute()`. Storage is preserved across the upgrade,
so the new WASM must be storage-layout compatible with the old version.

**Code reference.** `execute()` in `contracts/upgrade-manager/src/lib.rs`.
Target contract pattern in `contracts/example-target/src/lib.rs`.
Storage compatibility guidance in `docs/wasm-compatibility.md`.

---

## Governance Parameters

### Quorum

**Definition.** The minimum total number of votes (YES + NO combined) that
must be cast for a proposal to be eligible to pass.

**In this project.** Set in `Config.quorum`. A proposal that reaches
`voting_ends_at` with fewer than `snap_quorum` total votes is marked Defeated
regardless of how the votes split. This prevents a small group from passing a
proposal unnoticed.

**Code reference.** `Config.quorum`, `Proposal.snap_quorum` —
`contracts/upgrade-manager/src/types.rs`.
Evaluated in `finalize_voting()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Approval Threshold

**Definition.** The minimum percentage of YES votes out of all votes cast
(YES + NO) required for a proposal to pass.

**In this project.** Stored as `Config.approval_threshold_pct` (an integer
1–100 representing a percentage, e.g. `51` for a simple majority). The check
in `finalize_voting()` is:

```
yes_votes * 100 >= total_votes * approval_threshold_pct
```

Both quorum and approval threshold must be met for a proposal to move to
Queued; either failing condition results in Defeated.

**Code reference.** `Config.approval_threshold_pct`,
`Proposal.snap_threshold_pct` —
`contracts/upgrade-manager/src/types.rs`.
Pass condition in `finalize_voting()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Config Snapshot

**Definition.** A copy of the governance parameters (voting period, timelock
delay, quorum, approval threshold) taken from `Config` at the moment a proposal
is created and stored immutably within the proposal itself.

**In this project.** The fields `Proposal.snap_voting_period`,
`Proposal.snap_timelock_delay`, `Proposal.snap_quorum`, and
`Proposal.snap_threshold_pct` are written once in `create_proposal()` and never
updated. This means that calling `update_config()` after a proposal is created
does not change the rules under which that proposal is evaluated — in-flight
proposals are always governed by the rules that were in effect when they were
created.

**Code reference.** `Proposal.snap_voting_period`,
`Proposal.snap_timelock_delay`, `Proposal.snap_quorum`,
`Proposal.snap_threshold_pct` —
`contracts/upgrade-manager/src/types.rs`.
Snapshot written in `create_proposal()` —
`contracts/upgrade-manager/src/governance.rs`.
See also `docs/design/per-proposal-voting-params.md`.

---

## Proposal Statuses

The lifecycle of a proposal is tracked by the `ProposalStatus` enum defined in
`contracts/upgrade-manager/src/types.rs`.

### Active

**Definition.** The proposal is open for voting.

**In this project.** A proposal enters the `Active` state when created by
`propose_upgrade()`. It remains Active until `finalize()` is called after the
voting period ends, or until the admin calls `cancel()`.

**Code reference.** `ProposalStatus::Active` —
`contracts/upgrade-manager/src/types.rs`.
Set in `create_proposal()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Queued

**Definition.** Voting passed; the proposal is waiting for the timelock delay
to expire before it can be executed.

**In this project.** A proposal becomes Queued when `finalize()` determines it
met both quorum and the approval threshold. `Proposal.executable_at` is set at
this point. An admin can still cancel a Queued proposal.

**Code reference.** `ProposalStatus::Queued` —
`contracts/upgrade-manager/src/types.rs`.
Set in `finalize_voting()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Defeated

**Definition.** The voting period ended without the proposal meeting quorum or
the approval threshold.

**In this project.** `finalize()` sets the status to Defeated when
`total_votes < snap_quorum` or `yes_votes * 100 < total_votes * snap_threshold_pct`.
A Defeated proposal cannot be executed, re-opened, or cancelled.

**Code reference.** `ProposalStatus::Defeated` —
`contracts/upgrade-manager/src/types.rs`.
Set in `finalize_voting()` —
`contracts/upgrade-manager/src/governance.rs`.

---

### Executed

**Definition.** The timelock expired and the WASM upgrade was successfully
applied to the target contract.

**In this project.** `execute()` sets the status to Executed before invoking
the target's `upgrade()` function (checks-effects-interactions pattern). If the
target call fails, the transaction reverts and the proposal returns to Queued.
An Executed proposal is terminal — it cannot be cancelled or re-executed.

**Code reference.** `ProposalStatus::Executed` —
`contracts/upgrade-manager/src/types.rs`.
Set in `execute()` in `contracts/upgrade-manager/src/lib.rs`.

---

### Cancelled

**Definition.** The proposal was cancelled by the admin before execution.

**In this project.** The admin can cancel any Active or Queued proposal by
calling `cancel()`. Cancelled proposals are terminal and cannot be
re-opened or executed.

**Code reference.** `ProposalStatus::Cancelled` —
`contracts/upgrade-manager/src/types.rs`.
Set in `cancel_proposal()` —
`contracts/upgrade-manager/src/governance.rs`.

---

## Storage Concepts

### Instance Storage

**Definition.** A per-contract storage tier on Stellar that holds a single
key/value map for the contract. It has a shorter default TTL than persistent
storage and is loaded together with the contract's WASM on every invocation.

**In this project.** The upgrade manager stores its singleton data in instance
storage: `Admin`, `Config`, and `ProposalCount`. The TTL is extended by
`INSTANCE_TTL_BUMP` (518,400 ledgers ≈ 30 days) on every write. If the contract
is not interacted with for more than 30 days, instance storage may be archived
by the network; it must then be restored before the contract can be used again.

**Code reference.** `INSTANCE_TTL_BUMP = 518_400` in
`contracts/upgrade-manager/src/types.rs`.
`DataKey::Admin`, `DataKey::Config`, `DataKey::ProposalCount` stored in
instance storage — `contracts/upgrade-manager/src/governance.rs`.
Full layout documented in `docs/storage-layout.md`.

---

### Persistent Storage

**Definition.** A per-contract storage tier on Stellar that holds individual
key/value entries, each with its own TTL. Entries are not loaded automatically
on every invocation.

**In this project.** Per-proposal and per-voter data is kept in persistent
storage: `Proposal(u64)`, `Vote(u64, Address)`, `Delegation(Address)`, and
`DelegationCount(Address)`. The TTL is extended by `PERSISTENT_TTL_BUMP`
(2,592,000 ledgers ≈ 150 days) on every write and also on every read of a
proposal, so an actively-voted proposal is not archived while voting is ongoing.

**Code reference.** `PERSISTENT_TTL_BUMP = 2_592_000` in
`contracts/upgrade-manager/src/types.rs`.
`DataKey::Proposal`, `DataKey::Vote`, `DataKey::Delegation`,
`DataKey::DelegationCount` stored in persistent storage —
`contracts/upgrade-manager/src/governance.rs`.
Full layout documented in `docs/storage-layout.md`.

---

### TTL (Time to Live)

**Definition.** The number of ledgers remaining before a storage entry (or
contract instance) may be archived by the Stellar network. Archived entries
must be restored before they can be read.

**In this project.** Two TTL constants control how long entries survive without
activity:

| Constant | Ledgers | Approximate duration |
|---|---|---|
| `INSTANCE_TTL_BUMP` | 518,400 | ≈ 30 days |
| `PERSISTENT_TTL_BUMP` | 2,592,000 | ≈ 150 days |

The 150-day persistent TTL is long enough to outlive any realistic proposal
lifecycle. TTL is extended proactively on every write (and on reads of
`Proposal` entries) so that normal usage keeps all storage entries alive.

**Code reference.** `INSTANCE_TTL_BUMP`, `PERSISTENT_TTL_BUMP` —
`contracts/upgrade-manager/src/types.rs`.
`extend_ttl` calls in `save_config()`, `save_admin()`, `save_proposal()`,
`record_vote()`, `load_proposal()` —
`contracts/upgrade-manager/src/governance.rs`.

---

## Security Patterns

### Checks-Effects-Interactions

**Definition.** A code-ordering discipline that structures state-changing
functions in three phases: first verify all preconditions (checks), then write
the new state (effects), and only then make external calls (interactions). This
prevents re-entrancy and ensures that state is consistent from the moment of
the external call.

**In this project.** `execute()` follows this pattern explicitly:

1. **Check** — `assert_timelock_passed()` verifies the proposal is Queued and
   the timelock has elapsed.
2. **Effect** — `proposal.status = ProposalStatus::Executed` and `save_proposal()`
   are called before the external call.
3. **Interaction** — `invoke_upgrade()` crosses the contract boundary to call
   `target.upgrade(new_wasm_hash)`.

If the target call fails, Soroban's atomic transaction model reverts the entire
transaction including the `save_proposal` write, so the proposal returns to
Queued automatically.

**Code reference.** `execute()` in `contracts/upgrade-manager/src/lib.rs`.
Full explanation and test coverage in `docs/checks-effects-interactions.md`.
Test: `test_failed_upgrade_leaves_proposal_queued` in
`contracts/upgrade-manager/src/test.rs`.

---

## See Also

- `contracts/upgrade-manager/src/types.rs` — `ProposalStatus`, `Config`,
  `Proposal`, `DataKey`, TTL constants, `Error` variants.
- `contracts/upgrade-manager/src/governance.rs` — `create_proposal()`,
  `cast_vote()`, `finalize_voting()`, `cancel_proposal()`, `invoke_upgrade()`.
- `contracts/upgrade-manager/src/lib.rs` — public contract interface including
  `execute()`.
- `contracts/upgrade-manager/src/timelock.rs` — `assert_timelock_passed()`,
  `ledgers_until_executable()`.
- `docs/checks-effects-interactions.md` — in-depth explanation of the
  checks-effects-interactions pattern in `execute()`.
- `docs/storage-layout.md` — full storage key/TTL reference.
- `docs/wasm-compatibility.md` — storage-layout compatibility guidance for
  upgrade proposals.
- `docs/walkthrough.md` — step-by-step usage guide.
