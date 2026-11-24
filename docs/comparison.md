# Comparison with Other Upgrade Patterns

This page compares four ways to control a smart-contract upgrade:

1. **Native Soroban self-upgrade** — a contract calls
   `env.deployer().update_current_contract(...)` on itself.
2. **Soroban Upgrade Manager** — this repository's contract.
3. **Compound Governor Bravo** — Compound's on-chain governance system.
4. **OpenZeppelin TimelockController** — OpenZeppelin's timelock primitive.

The goal is to make the trade-offs explicit, not to claim that this contract
is universally better. It is a small, dependency-free contract with real
limitations, and several of those limitations are exactly the features the
other systems provide.

> **Caveat.** This contract is unaudited. This page describes the code in this
> repository as it exists today; it is not a security endorsement and not a
> substitute for reviewing the code. External projects are described from
> their published source and documentation, cited in
> [References](#references). They are actively developed, so check the cited
> version if precision matters.

One more caveat before the tables: **the four systems do not all upgrade WASM
in the same sense.** Compound Governor Bravo and OpenZeppelin TimelockController
execute arbitrary calls against arbitrary targets; "upgrading" is just one kind
of call they can make, and the actual upgrade logic lives in the target. Native
Soroban and this contract specifically replace a contract's WASM executable
(this contract does it by calling a target's `upgrade()` function).

---

## At a glance

| Dimension | Native Soroban self-upgrade | Soroban Upgrade Manager (this repo) | Compound Governor Bravo | OpenZeppelin TimelockController |
|---|---|---|---|---|
| **Who can upgrade** | Whatever the contract's own `upgrade()` function enforces; the host applies no policy. The Stellar example checks a stored admin [1], but a contract that omits the check is callable by anyone. | Nobody directly. `execute()` performs the upgrade only for a proposal that passed a vote and waited out the timelock. `execute()` and `finalize()` are permissionless; the admin cannot upgrade without a passing vote. | Whoever can get a proposal through governance. Creating one requires prior votes above `proposalThreshold` (or whitelist status); after it succeeds and is queued, anyone may call `execute()` [4]. | Scheduling requires `PROPOSER_ROLE`; executing requires `EXECUTOR_ROLE` (or anyone if that role is granted to `address(0)`). The timelock itself usually holds upgrade rights on the target [7][8]. |
| **Voting mechanism** | None. | One vote per address, plus optional global delegation: a delegate's ballot weighs `1 + delegators`. Quorum is on total YES+NO votes; the threshold is a percentage of YES out of YES+NO. No abstain, no stake, no token weighting [9]. | Token-weighted. Votes are read from `getPriorVotes` at the proposal's `startBlock`; support can be for, against, or abstain. Quorum and proposal threshold are configured per deployment (Compound's reference delegate hardcodes 400,000 COMP quorum) [4][6]. | None. It is a timelock primitive, normally paired with a Governor or a multisig that decides what to schedule [7][8]. |
| **Timelock** | None. The executable changes when the transaction succeeds [1][2]. | `timelock_delay` ledgers (minimum 1), starting when `finalize()` is called, not at proposal creation. No grace period and no expiry once queued [9]. | Separate `Timelock` contract. `eta = now + timelock.delay()` at queue time; execution is allowed from `eta` until `eta + GRACE_PERIOD`. Compound's Timelock uses a 2-day minimum delay and a 14-day grace period [5]. | `minDelay` in seconds; a scheduled operation may specify a longer delay but not a shorter one. Ready operations do not expire [7]. |
| **Cancel power** | No queued operation to cancel. If the upgrade transaction reverts, nothing changes. | Admin only, and only for `Active` or `Queued` proposals. A queued proposal can be cancelled during the timelock, including after it passed the vote. Executed, defeated, and already-cancelled proposals cannot be cancelled [9]. | The proposer can cancel before execution; anyone can cancel if the proposer's current votes fell below `proposalThreshold`; for whitelisted proposers, the `whitelistGuardian` can cancel under that condition. Executed proposals cannot be cancelled [4]. | `CANCELLER_ROLE` can cancel while an operation is `Waiting` or `Ready` [7]. |
| **Audit trail** | An automatic system `executable_update` event records the old and new executable. There is no proposal or vote record unless the contract adds one [1][3]. | Proposals are stored persistently (target, WASM hash, title, description, metadata hash, timestamps, tallies, status, snapshotted rules). A per-voter "has voted" marker is stored; vote direction and weight are emitted in the `vote_cast` event. Every transition emits an event [9][10]. | Proposals, actions, and per-voter receipts are on-chain; `VoteCast`, `ProposalCreated`, `ProposalQueued`, `ProposalExecuted`, and `ProposalCanceled` events; the Timelock emits queue/execute/cancel events [4][5]. | Operation IDs and states are on-chain; `CallScheduled`, `CallExecuted`, `Cancelled`, and `MinDelayChange` events [7]. |
| **Cross-contract support** | No. `update_current_contract` replaces the *calling* contract's executable; a contract cannot directly rewrite another contract's code [1][2]. | Yes. `execute()` calls `target.upgrade(new_wasm_hash)` on one target contract. The target must authenticate the manager, or governance can be bypassed [9][11]. | Yes. A proposal carries up to 10 `(target, value, signature, calldata)` actions, so it can call any contract [4]. | Yes. Single calls or atomic batches against arbitrary targets [7]. |

Time units differ across systems — blocks for Bravo, seconds for
TimelockController, ledgers for this contract — so the raw numbers are not
directly comparable.

---

## 1. Native Soroban self-upgrade

Soroban contracts can replace their own executable with
`env.deployer().update_current_contract(ContractExecutable::Wasm(hash))`; the
older name for the same operation is `update_current_contract_wasm` [1][2].
The new WASM must already be uploaded to the ledger, and the contract keeps its
address and storage. The update takes effect only after the invocation
finishes successfully [1][2].

The host function is deliberately narrow: it updates the *current* contract.
It cannot upgrade a different contract. To upgrade another contract you must
call a function on that contract which performs its own update — which is what
this repository's manager does.

There is no built-in authorization, voting, or delay. The Stellar guide's
example gates `upgrade()` behind an admin address with `require_auth()` [1],
and SEP-49 recommends exactly that, plus a recognizable `upgrade()` name [3].
A contract that forgets the auth check can be upgraded by anyone.

Auditability is better than "none": the SDK docs note that the update emits a
system `executable_update` event containing the old and new executables [1].
What is missing is the *decision* trail — no proposal, no votes, no reason, no
delay.

**Good fit when:** the upgrade authority is a single trusted operator, speed
matters more than review time, or the contract is early-stage and you accept
that trust model.

**Weak fit when:** users need time to exit before an upgrade, or you need
on-chain evidence of who approved what.

---

## 2. Soroban Upgrade Manager (this repository)

The manager adds a proposal-and-vote layer in front of a target's `upgrade()`
function.

### Who can upgrade

`propose_upgrade` is open to any authenticated address. Voting is open to any
authenticated address. `finalize` and `execute` require no authorization and
can be called by anyone once their time windows are satisfied [9][12].

The admin cannot execute an upgrade directly. `execute()` calls
`assert_timelock_passed`, which requires the proposal to be `Queued`, and a
proposal only becomes `Queued` through a successful `finalize()` [9][12].
The admin *can* cancel proposals, change governance parameters for future
proposals, and transfer the admin role.

One important dependency: the target contract must call
`manager.require_auth()` before updating its own WASM. If it does not, anyone
who can call the target's `upgrade()` can bypass the manager entirely. The
reference implementation is in
[`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs) [9][11].

### Voting mechanism

- Each address may vote once per proposal.
- A voter may delegate their vote globally with `delegate`; delegation is not
  transitive, and a delegator cannot vote directly while delegated. A
  delegate's vote weight is `1 +` the number of addresses delegated to it [9].
- `quorum` is the minimum **total** votes (YES + NO).
- `approval_threshold_pct` is the minimum YES share of YES + NO, computed as
  `yes * 100 >= total * threshold` [9].
- There is no abstain option, no stake, and no token balance check.

The README's shorthand "one address, one vote" refers to the absence of
token weighting; the delegation feature is the one place where one ballot can
carry more than one unit of weight.

### Timelock

`timelock_delay` is measured in ledgers and must be at least 1. It starts when
`finalize()` is called, not when the proposal is created, and sets
`executable_at = finalize_ledger + snap_timelock_delay` [9]. There is no grace
period: a queued proposal remains executable indefinitely until it is executed
or cancelled [9][13].

Because the delay starts at finalize, a proposal can sit `Active` indefinitely
after voting ends; the timelock has not started until someone calls
`finalize()` [13].

### Cancel power

`cancel` is admin-only and accepts `Active` or `Queued` proposals. It returns
`ProposalFinalized` for `Executed`, `Cancelled`, or `Defeated` proposals [9].
This means the admin can block an upgrade the voters already approved, during
the timelock. That is a deliberate safety valve and also a real centralization
risk; see [`docs/limitations.md`](limitations.md).

### Audit trail

Each proposal is stored persistently as a `Proposal` struct containing the
target, new WASM hash, title, description, metadata hash, creation and voting
timestamps, execution timestamp, vote tallies, status, proposer, and snapshot
of the governance rules [14]. A `Vote(proposal_id, voter)` marker records that
an address voted; the direction and weight are published in the `vote_cast`
event rather than stored per voter [9][10]. Every state transition emits an
event [10].

### Cross-contract support

The manager supports exactly one kind of cross-contract action: call
`upgrade(BytesN<32>)` on one target contract, with one WASM hash, once per
proposal. It cannot batch calls, call arbitrary functions, transfer value, or
upgrade several targets atomically. That narrowness is intentional, but it is
a capability gap compared with Bravo and TimelockController.

### Other deliberate omissions

- No token weighting or sybil resistance.
- No proposal deposit; a per-address `proposal_min_interval` rate limit is the
  only spam control [9][13].
- No proposal expiry.
- No storage-layout compatibility check.
- No separate roles: one admin holds cancel, config, and transfer powers.

---

## 3. Compound Governor Bravo

Governor Bravo is a token-weighted governance contract. Its reference
implementation is `GovernorBravoDelegate.sol` in the Compound protocol
repository [4], with parameter documentation in Compound's governance docs [6].

### Who can upgrade

Bravo does not upgrade WASM itself. It executes the actions in a successful
proposal against arbitrary targets [4]. So "who can upgrade" is really "who can
get an upgrade call through governance".

Creating a proposal requires the proposer's prior votes to exceed
`proposalThreshold`, unless the address is whitelisted. In the reference
implementation the threshold is governance-configurable between 1,000 and
100,000 COMP, and one address may have only one live proposal at a time [4].

After a proposal succeeds and is queued, `execute()` is callable by anyone,
subject to the timelock [4].

### Voting mechanism

Votes are token-weighted, read from `getPriorVotes(voter, proposal.startBlock)`
at the block when voting starts, so later token transfers do not change a
proposal's outcome [4][6]. Support can be for, against, or abstain, and the
reference implementation requires `forVotes > againstVotes` and
`forVotes >= quorumVotes` to succeed [4]. Quorum in that delegate is a
hardcoded 400,000 COMP (4% of supply); forks and other deployments commonly
change these parameters [4][6].

This gives economic weight to votes, but it also means governance power follows
token ownership. That is a different trust model from this contract, not simply
a stronger version of it.

### Timelock

Bravo queues actions into a separate Compound `Timelock` contract. It sets
`eta = block.timestamp + timelock.delay()` and the Timelock accepts
`executeTransaction` only from `eta` to `eta + GRACE_PERIOD` [4][5]. The
reference Timelock sets a 2-day minimum delay, a 30-day maximum, and a 14-day
grace period [5]. A queued proposal that is not executed within the grace
period expires and cannot be executed later [4][5].

### Cancel power

The proposer can cancel their own proposal at any point before execution.
Anyone else may cancel only if the proposer's current voting power has fallen
below `proposalThreshold`; for whitelisted proposers, the `whitelistGuardian`
can cancel under the same condition [4]. Executed proposals cannot be
cancelled [4].

There is no general admin veto over a proposal that retains its proposer's
support. This is a meaningful difference from this contract, where the admin
can cancel a queued proposal unconditionally.

### Audit trail

Proposals, actions, and per-voter receipts are stored on-chain, with events for
creation, voting, queueing, execution, and cancellation [4]. The Timelock emits
its own queue, execute, and cancel events [5].

### Cross-contract support

A proposal carries arrays of targets, values, signatures, and calldatas. The
reference implementation caps a proposal at 10 actions and executes them in
order [4]. This is broader than this contract: arbitrary calls, value
transfers, and multiple targets, all governed by one vote.

---

## 4. OpenZeppelin TimelockController

TimelockController is a reusable timelock, not a governance system. It is
commonly owned by a target contract and driven by a Governor or a multisig [7][8].

### Who can upgrade

TimelockController enforces role-based access:

- `PROPOSER_ROLE` schedules operations.
- `EXECUTOR_ROLE` executes ready operations. Granting this role to
  `address(0)` makes execution permissionless.
- `CANCELLER_ROLE` cancels pending operations.
- `DEFAULT_ADMIN_ROLE` grants and revokes the other roles; the contract is
  self-administered by default, so role changes should themselves go through
  the timelock [7][8].

If the timelock is the owner/admin of a target, then a scheduled,
delayed, role-authorized call is what upgrades it. The contract has no opinion
about *how* the decision was made.

### Voting mechanism

None. Any voting, quorum, or token-weighting logic must come from a Governor
placed in front of it, or from an off-chain process that controls the proposer
role [7][8]. Comparing this contract's built-in vote to TimelockController is
therefore comparing a governance system to a timelock component.

### Timelock

`minDelay` is in seconds. `schedule` and `scheduleBatch` accept a per-operation
delay, which must be at least `minDelay`; the operation becomes ready at
`block.timestamp + delay` [7]. Operations can declare a `predecessor` that must
be done first, and ready operations do not expire [7].

### Cancel power

`CANCELLER_ROLE` can cancel an operation while it is `Waiting` or `Ready`;
cancelling an unset or done operation reverts [7]. By default the constructor
grants both proposer and canceller roles to each proposer [7]. Who effectively
holds a veto depends entirely on how the deployer assigns roles.

### Audit trail

The operation ID (a hash of target, value, calldata, predecessor, and salt) and
its state are on-chain. Events cover scheduling (`CallScheduled`), execution
(`CallExecuted`), cancellation (`Cancelled`), and delay changes
(`MinDelayChange`) [7]. There is no vote record, because there is no voting.

### Cross-contract support

Yes: single calls and atomic batches to arbitrary targets, including value
transfers. This is the most general execution model of the four [7].

---

## Trade-offs in plain terms

| If you need… | Native Soroban | This contract | Bravo | TimelockController |
|---|---|---|---|---|
| No new trust assumptions beyond an admin key | ✅ simplest | ❌ adds voters/admin | ❌ adds token governance | ❌ adds role holders |
| On-chain proposal and vote record | ❌ | ✅ | ✅ | ❌ (only operations) |
| Enforced delay before code changes | ❌ | ✅ | ✅ | ✅ |
| Token-weighted or stake-based voting | ❌ | ❌ | ✅ | ❌ (bring your own Governor) |
| Sybil resistance from the mechanism itself | ❌ | ❌ | ⚠️ economic, not identity-based | ❌ |
| Arbitrary or batched cross-contract calls | ❌ | ❌ | ✅ up to 10 actions | ✅ single or batch |
| One target's WASM hash, minimal surface | ✅ self only | ✅ | ❌ broader | ❌ broader |
| Cancel an approved upgrade | n/a | ✅ admin veto | ⚠️ only proposer/fallen threshold | ✅ canceller role |
| No proposal expiry | n/a | ✅ (deliberate) | ❌ grace period | ✅ ready ops persist |

### Where this contract is a reasonable choice

- You control a single target contract and want a small governance layer that
  only knows how to upgrade that contract's WASM.
- You already have an off-chain process for deciding who votes, and you want
  the on-chain part to be simple and auditable.
- You want a mandatory review window without introducing a token.
- You accept the admin key as a safety valve and plan to hold it in a
  multisig or DAO.

### Where another pattern is likely better

- **Native self-upgrade:** you trust one operator and want the smallest
  possible upgrade surface.
- **Bravo-style governance:** you already have a governance token and want
  economic weight, abstain votes, and broad arbitrary-call execution.
- **TimelockController:** you want a well-tested timelock primitive and already
  have a decision mechanism (Governor, multisig, or DAO) to drive it.

---

## What this contract does not claim

This page is intentionally modest about the manager:

- It is **not sybil-resistant**. One address can create many addresses and
  vote with each. See [README — Security Model & Limitations](../README.md#-security-model--limitations)
  and [`docs/limitations.md`](limitations.md).
- It does **not** weight votes by tokens or stake.
- It does **not** verify that a new WASM is storage-layout compatible with the
  old one. An incompatible upgrade can brick the target. Use an off-chain
  compatibility check, as described in the README's Recommended Workflow.
- It does **not** make the admin powerless: the admin can cancel queued
  proposals and change rules for future proposals.
- It is **unaudited**.

---

## Verification

Claims about this contract above map to these implementation points:

| Claim | Code |
|---|---|
| Only a passed, queued, timelock-expired proposal can upgrade | `execute` → `assert_timelock_passed` in [`timelock.rs`](../contracts/upgrade-manager/src/timelock.rs), [`lib.rs`](../contracts/upgrade-manager/src/lib.rs) |
| Proposal snapshots governance rules at creation | `create_proposal` in [`governance.rs`](../contracts/upgrade-manager/src/governance.rs) |
| Vote weight is 1 plus delegators; one vote per address | `cast_vote`, `delegate`, `delegation_count` in [`governance.rs`](../contracts/upgrade-manager/src/governance.rs) |
| Timelock starts at finalize and is at least 1 ledger | `finalize_voting`, `validate_config` in [`governance.rs`](../contracts/upgrade-manager/src/governance.rs) |
| Admin can cancel only Active or Queued | `cancel_proposal` in [`governance.rs`](../contracts/upgrade-manager/src/governance.rs) |
| Cross-contract call is `target.upgrade(wasm_hash)` | `invoke_upgrade` in [`governance.rs`](../contracts/upgrade-manager/src/governance.rs) |
| Target must authenticate the manager | [`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs) |

The authoritative mapping from documentation claims to functions and tests is
[`docs/claims-audit.md`](claims-audit.md); `scripts/check_claims.sh` verifies
that the named tests exist.

---

## References

1. Stellar Developers, "Upgrading Wasm Bytecode for a Deployed Contract" —
   <https://developers.stellar.org/docs/build/guides/conventions/upgrading-contracts>
2. Stellar Protocol, CAP-0046-03 (contract executable update semantics) —
   <https://github.com/stellar/stellar-protocol/blob/master/core/cap-0046-03.md>
3. Stellar Ecosystem Proposals, SEP-0049 (upgrade function practice and
   automatic system event) —
   <https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0049.md>
4. Compound, `GovernorBravoDelegate.sol` —
   <https://github.com/compound-finance/compound-protocol/blob/master/contracts/Governance/GovernorBravoDelegate.sol>
5. Compound, `Timelock.sol` —
   <https://github.com/compound-finance/compound-protocol/blob/master/contracts/Timelock.sol>
6. Compound Documentation, "Governance" —
   <https://compound.finance/docs/governance>
7. OpenZeppelin Contracts v5.5.0, `TimelockController.sol` —
   <https://github.com/OpenZeppelin/openzeppelin-contracts/blob/v5.5.0/contracts/governance/TimelockController.sol>
8. OpenZeppelin Docs, "How to manage roles of a TimelockController" —
   <https://docs.openzeppelin.com/defender/guide/timelock-roles>
9. This repository, `contracts/upgrade-manager/src/governance.rs`.
10. This repository, [`docs/events-reference.md`](events-reference.md).
11. This repository, [`docs/writing-target-contracts.md`](writing-target-contracts.md).
12. This repository, `contracts/upgrade-manager/src/lib.rs`.
13. This repository, [`docs/limitations.md`](limitations.md).
14. This repository, `contracts/upgrade-manager/src/types.rs`.

External sources were checked on 2026-11-24.
