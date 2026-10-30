# 🔄 Soroban Upgrade Manager

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Stellar](https://img.shields.io/badge/Built%20on-Stellar-blue)](https://stellar.org)
[![Soroban](https://img.shields.io/badge/Smart%20Contracts-Soroban-purple)](https://soroban.stellar.org)
[![CI](https://github.com/ogenyialice120/soroban-upgrade-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/ogenyialice120/soroban-upgrade-manager/actions/workflows/ci.yml)

A Soroban smart contract that manages **WASM upgrades for other contracts** through on-chain governance votes and a configurable **timelock delay** before execution.

---

## 🤔 Why Not Just Use Soroban's Native `update_current_contract_wasm`?

Soroban contracts can upgrade themselves by calling
`env.deployer().update_current_contract_wasm(new_hash)` directly. That works,
but it places **zero constraints** on who can trigger the upgrade and when.

In practice this means:

| Problem | Native Soroban | Soroban Upgrade Manager |
|---|---|---|
| Who can upgrade? | Any address with admin auth | Must pass a governance vote |
| Is there a mandatory delay? | No — upgrades are instant | Configurable timelock delay (in ledgers) |
| Is the upgrade auditable on-chain? | No proposal trail | Full proposal + vote history on-chain |
| Can the community stop a bad upgrade? | Only if you trust the admin | Voters can outvote a bad proposal; admin can cancel Active or Queued proposals |
| Can you upgrade a **different** contract? | No — only self-upgrade | Yes — cross-contract upgrade via `invoke_contract` |

**The real problem this solves:** A single admin key controlling an upgrade path
is a single point of failure and a source of trust risk for users. If the admin
key is compromised, the attacker can silently swap in malicious WASM. If the admin
is a team, users have to trust that the team will not rug them.

Soroban Upgrade Manager replaces that single-key trust with:
1. A **transparent proposal** that anyone can inspect before it executes.
2. A **vote** that requires a quorum of addresses to approve (one address, one vote).
3. A **mandatory delay** between approval and execution, giving users time to exit
   if they disagree with the upgrade.

This pattern is inspired by Compound Governor Bravo and OpenZeppelin's
TimelockController, adapted for Soroban's contract model. Unlike those systems,
this contract uses unweighted one-address-one-vote voting rather than
token-weighted voting.

---

## ✨ Features

| Feature | Description |
|---|---|
| 🗳️ **Governance Voting** | Any address can propose an upgrade; any address can vote YES/NO (one address, one vote) |
| ⏱️ **Configurable Timelock** | Mandatory delay between finalization and execution, set in ledgers |
| 🔗 **Cross-Contract Upgrade** | Executes `upgrade()` on the target contract after the timelock expires |
| 📊 **Quorum & Threshold** | Configurable minimum vote count and YES-vote percentage |
| 🚨 **Emergency Cancel** | Admin can cancel any Active or Queued proposal |
| 🔑 **Admin Transfer** | Admin role can be transferred to a multisig or DAO |
| 📖 **Full Audit Trail** | All proposals and votes are persisted on-chain |
| 📸 **Per-Proposal Config Snapshot** | Each proposal snapshots governance parameters at creation time; later config changes do not affect in-flight proposals |

---

## 🏗️ Architecture

```
soroban-upgrade-manager/
├── contracts/
│   ├── upgrade-manager/
│   │   ├── src/
│   │   │   ├── lib.rs          # Contract entry point & public API
│   │   │   ├── governance.rs    # Proposal creation, voting, finalization
│   │   │   ├── timelock.rs      # Timelock enforcement helpers
│   │   │   ├── types.rs         # Shared types, storage keys, errors
│   │   │   └── test.rs          # Test suite (66 tests)
│   │   ├── tests/fixtures/     # Committed example-target WASM + SHA256SUMS
│   │   └── Cargo.toml
│   └── example-target/          # Minimal upgradable contract for tests/walkthrough
│       ├── src/lib.rs
│       └── Cargo.toml
├── scripts/
│   └── build_fixtures.sh        # Build v1/v2 WASM fixtures
├── docs/
│   └── walkthrough.md           # Step-by-step deployment guide
├── Cargo.toml                   # Workspace manifest
├── README.md
├── CONTRIBUTING.md
└── SECURITY.md
```

### Proposal Lifecycle

```
propose_upgrade()           voting_ends_at
      │                      │
      ▼                      ▼
  [Active] ── vote() × N ──► finalize()
                               │
                    pass? ──────┴──── fail?
                     │                  │
                     ▼                  ▼
                 [Queued]          [Defeated]
                     │
              timelock_delay
              (from finalize)
                     │
                     ▼
                execute()
                     │
                     ▼
                [Executed]

  cancel() (admin only) ──► [Cancelled]  (from Active or Queued)
```

The voting period (`voting_period` ledgers) runs from proposal creation to
`voting_ends_at`. The timelock delay (`timelock_delay` ledgers) runs from
`finalize()` to `executable_at`. These are two separate windows.

---

## 🚀 Getting Started

### Prerequisites

- [Rust](https://rustup.rs/)
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/stellar-cli) (`stellar` v22+)

### Build

```bash
git clone https://github.com/ogenyialice120/soroban-upgrade-manager.git
cd soroban-upgrade-manager

# Add the Wasm target
rustup target add wasm32v1-none

# Build
cargo build --target wasm32v1-none --release \
  --manifest-path contracts/upgrade-manager/Cargo.toml
```

### Run Tests

```bash
cargo test
```

### Deploy to Testnet

```bash
# Configure network
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"

# Generate & fund account
stellar keys generate alice --network testnet
stellar keys fund alice --network testnet

# Deploy
stellar contract deploy \
  --wasm target/wasm32v1-none/release/soroban_upgrade_manager.wasm \
  --source alice \
  --network testnet
```

---
## Verified on Stellar testnet

On 2026-10-30 the contracts were deployed to Stellar testnet and a full governance upgrade was run by hand: a proposal upgraded a live example-target contract from version 1 to version 2 through the manager. This was a one-off manual run by a single account acting as admin, proposer and voter. It shows the mechanism works on a real network. It is not an automated test, and it does not make voting sybil-resistant (see Security model & limitations).

### Parameters used

- voting_period: 20
- timelock_delay: 20
- quorum: 1
- approval_threshold_pct: 51
- (ledgers are about 5 seconds each)

### Results

| Network | Item | Value |
|---|---|---|
| Stellar testnet | Admin account | `GBTKMKD6D3SB7LB5B4F2SO64B5GF2SROGK4LYGTNDARFB2WEJDMETWXI` |
| Stellar testnet | Upgrade manager contract | `CCSOYARHTTTE6I5WT27EHGQEHZZMNI6KNT6SCTWDYIK6TDJ3HA3RLYDW` |
| Stellar testnet | Manager WASM hash | `d4696206a5ba03d4326fea8da1f3e2ca5b693fe632a6dd8d459683dc994b25cd` |
| Stellar testnet | Example target contract | `CBIV5DVEAKHGS3KQMBJB6J4KPQIVOHKNFE2IDS7UADHWNBST466MKBWQ` |
| Stellar testnet | Target v1 WASM hash | `34fd6d1838f2d7d167500fe480061b5cd915b59320e2bdde5b433dde1aaec81f` |
| Stellar testnet | v2 WASM hash used in the proposal | `847f384de0399e007643b51c7b4b421e29a9e5f28f8b53f320593fbbee2d1f3c` |
| Stellar testnet | Final `version()` of the target | 2 (it returned 1 before the upgrade) |

### Transactions

| Step | Transaction |
|---|---|
| Upload manager WASM | [3a1862a30f6480b57520c7e13ab9101cfcd5953e15567d61b0b9b198c9014397](https://stellar.expert/explorer/testnet/tx/3a1862a30f6480b57520c7e13ab9101cfcd5953e15567d61b0b9b198c9014397) |
| Deploy manager | [606613e98d557ed19a081c2891853652faa8e7cff28f04208a7b646d13e67115](https://stellar.expert/explorer/testnet/tx/606613e98d557ed19a081c2891853652faa8e7cff28f04208a7b646d13e67115) |
| Upload example-target v1 WASM | [6d4f40b948bee5583fbde53ec6ff55d27a091d2233996bd32e9bb70b190dd1b5](https://stellar.expert/explorer/testnet/tx/6d4f40b948bee5583fbde53ec6ff55d27a091d2233996bd32e9bb70b190dd1b5) |
| Deploy example-target (constructor given the manager contract address) | [da63efc75d30179321c24602cd7d7b22942d97026a605bb3e99a965111c3cbf4](https://stellar.expert/explorer/testnet/tx/da63efc75d30179321c24602cd7d7b22942d97026a605bb3e99a965111c3cbf4) |
| initialize | [e40a2e352db4e91ed9d4616e6bc8818c629c7b9a88a53561338fc1fa84d416d2](https://stellar.expert/explorer/testnet/tx/e40a2e352db4e91ed9d4616e6bc8818c629c7b9a88a53561338fc1fa84d416d2) |
| propose_upgrade (proposal id 0) | [d0b54188fcdd1fac7a709b3712b22578cd2da052a825457af8d3c3cadcd54b60](https://stellar.expert/explorer/testnet/tx/d0b54188fcdd1fac7a709b3712b22578cd2da052a825457af8d3c3cadcd54b60) |
| vote (yes) | [a68cd50f5df5c46889dd8039d35836cb20961fb293f0122e212251e7ea7c96ea](https://stellar.expert/explorer/testnet/tx/a68cd50f5df5c46889dd8039d35836cb20961fb293f0122e212251e7ea7c96ea) |
| finalize (result Queued) | [478ce2d98566fa68aa852eaac29289fde832766306fd5b396cf7784673b43b14](https://stellar.expert/explorer/testnet/tx/478ce2d98566fa68aa852eaac29289fde832766306fd5b396cf7784673b43b14) |
| execute | [3184bc0713e1e083c4636c413625811f2df399c4ab41e4423c039d920d4d99ca](https://stellar.expert/explorer/testnet/tx/3184bc0713e1e083c4636c413625811f2df399c4ab41e4423c039d920d4d99ca) |

The v2 upload transaction hash was not recorded.

### How to reproduce this run

1. Install the Stellar CLI with the official script:
   ```bash
   curl -fsSL https://github.com/stellar/stellar-cli/raw/main/install.sh | sh
   ```
   Note that `cargo install stellar-cli` can run out of memory in small environments and needs a newer Rust than this repo pins.

2. Add the build target:
   ```bash
   rustup target add wasm32v1-none
   ```

3. Build both contracts:
   ```bash
   cargo build --release --target wasm32v1-none --manifest-path contracts/upgrade-manager/Cargo.toml
   cargo build --release --target wasm32v1-none --manifest-path contracts/example-target/Cargo.toml
   ```
   Output is in `target/wasm32v1-none/release/`. Warning: builds with `wasm32-unknown-unknown` on Rust 1.82+ are rejected by the network with "reference-types not enabled".

4. Create and fund a test account:
   ```bash
   stellar keys generate alice --network testnet
   stellar keys fund alice --network testnet
   ```

5. Deploy the manager into a shell variable `MANAGER_ID`:
   ```bash
   MANAGER_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/soroban_upgrade_manager.wasm --source alice --network testnet)
   ```

6. Deploy the target into `TARGET_ID`:
   ```bash
   TARGET_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/example_target.wasm --source alice --network testnet -- --manager "$MANAGER_ID")
   ```
7. Initialize and check:
   ```bash
   stellar contract invoke --id "$MANAGER_ID" --source alice --network testnet -- initialize --admin alice --voting_period 20 --timelock_delay 20 --quorum 1 --approval_threshold_pct 51
   stellar contract invoke --id "$MANAGER_ID" --source alice --network testnet -- get_config
   stellar contract invoke --id "$TARGET_ID" --source alice --network testnet -- version
   ```
   The last command prints `1`.

8. Build v2 and upload it:
   ```bash
   cargo build --release --target wasm32v1-none --manifest-path contracts/example-target/Cargo.toml --features v2
   V2_HASH=$(stellar contract upload --wasm target/wasm32v1-none/release/example_target.wasm --source alice --network testnet)
   ```

9. Propose and vote:
   ```bash
   stellar contract invoke --id "$MANAGER_ID" --source alice --network testnet -- propose_upgrade --proposer alice --target "$TARGET_ID" --new_wasm_hash "$V2_HASH" --description "upgrade target to v2"
   stellar contract invoke --id "$MANAGER_ID" --source alice --network testnet -- vote --voter alice --proposal_id 0 --approve true
   ```

10. Wait about 2 minutes (voting period is 20 ledgers), then finalize:
    ```bash
    stellar contract invoke --id "$MANAGER_ID" --source alice --network testnet -- finalize --proposal_id 0
    ```
    This prints `Queued`.

11. Wait about 2 more minutes (timelock is 20 ledgers after finalize), then execute and check version:
    ```bash
    stellar contract invoke --id "$MANAGER_ID" --source alice --network testnet -- execute --proposal_id 0
    stellar contract invoke --id "$TARGET_ID" --source alice --network testnet -- version
    ```
    The last command prints `2`.

The contract IDs above are testnet contracts and may be reset when the testnet is reset, so the links may stop working in the future.

---

## 📖 Usage

### 1. Initialize the contract

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

> Both `voting_period` and `timelock_delay` are in ledgers. At ~5 s/ledger,
> 17 280 ≈ 24 hours. The voting period is when votes are accepted; the timelock
> delay is the mandatory wait between finalization and execution.

### 2. Propose an upgrade

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- propose_upgrade \
  --proposer <PROPOSER_ADDRESS> \
  --target <TARGET_CONTRACT_ID> \
  --new_wasm_hash <WASM_HASH_HEX> \
  --description "Upgrade to v2: add rate limiting"
```

### 3. Vote

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source voter1 \
  --network testnet \
  -- vote \
  --voter <VOTER_ADDRESS> \
  --proposal_id 0 \
  --approve true
```

### 4. Finalize voting (after voting period ends)

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- finalize \
  --proposal_id 0
```

### 5. Execute the upgrade (after timelock expires)

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- execute \
  --proposal_id 0
```

---

## 🔧 Configuration Reference

| Parameter | Type | Description |
|---|---|---|
| `voting_period` | `u32` | Ledgers between proposal creation and end of voting. Votes accepted while `ledger.sequence() <= voting_ends_at`. Min: 1. |
| `timelock_delay` | `u32` | Ledgers between `finalize()` and earliest `execute()`. The timelock starts when `finalize()` is called, not when the proposal is created. Min: 1. |
| `quorum` | `u32` | Minimum total votes (YES + NO) for a proposal to be considered. Min: 1. |
| `approval_threshold_pct` | `u32` | Percentage of YES votes required (1–100). E.g. `51` = simple majority. |

---

## 📋 Contract API

| Function | Description | Auth |
|---|---|---|
| `initialize` | Set admin and config (voting_period, timelock_delay, quorum, threshold). Can only be called once. | Admin (must sign) |
| `propose_upgrade` | Create an upgrade proposal. Config is snapshotted at this point. | Proposer (must sign) |
| `vote` | Cast YES/NO on a proposal. One vote per address. Only during voting period. | Voter (must sign) |
| `finalize` | Close voting after voting period ends. Moves to Queued (pass) or Defeated (fail). Sets `executable_at`. | Anyone |
| `execute` | Execute the upgrade after timelock expires. Calls target's `upgrade()`. | Anyone |
| `cancel` | Cancel an Active or Queued proposal. | Admin (must sign) |
| `update_config` | Change governance params. Does not affect in-flight proposals (snapshotted). | Admin (must sign) |
| `transfer_admin` | Hand off admin role to a new address. | Admin (must sign) |
| `get_proposal` | Fetch proposal by ID. | Read-only |
| `get_config` | Fetch governance config. | Read-only |
| `get_admin` | Fetch admin address. | Read-only |
| `time_until_executable` | Ledgers until timelock expires. Returns 0 if not Queued or already executable. | Read-only |

---

## 🛡️ Security Model & Limitations

**This contract is unaudited.** It has not undergone a formal security audit.
Use at your own risk.

### What the contract provides

- **Governance gate:** A proposal must receive at least `quorum` total votes with
  at least `approval_threshold_pct` YES votes to pass. Each address gets one vote.
- **Timelock delay:** After a proposal passes finalization, there is a mandatory
  wait of `timelock_delay` ledgers before `execute()` can be called. This gives
  users time to review the upgrade and exit if they disagree.
- **Config snapshot:** Each proposal snapshots the governance parameters at
  creation time. A subsequent `update_config` call does not change the rules for
  in-flight proposals.
- **On-chain audit trail:** All proposals, votes, and state transitions are
  persisted on-chain and emit events.

### Limitations

- **Not sybil-resistant:** Voting is one-address-one-vote with no stake, token
  weight, or identity requirement. An attacker who creates many addresses can
  sway votes. Consider using this with a sybil-resistant identity system or
  replacing it with token-weighted voting in production.
- **Powerful admin key:** The admin can cancel any Active or Queued proposal,
  change governance parameters, and transfer admin ownership. If the admin key
  is compromised, the attacker can cancel legitimate proposals. A multisig or
  DAO should hold the admin role. The admin **cannot** execute upgrades without
  a passing vote.
- **Admin can cancel after approval:** The admin can cancel a Queued proposal
  even after it has passed the vote, during the timelock delay. This is a
  deliberate safety valve, but it means the admin can unilaterally block an
  approved upgrade.
- **No proposal deposit:** Creating a proposal is free, so spam is possible.
  An attacker can flood the contract with proposals to increase gas costs for
  legitimate users.
- **Timelock starts at finalize:** The mandatory delay begins when someone calls
  `finalize()`, not when the proposal is created. If no one finalizes, the
  proposal stays Active indefinitely.
- **Target must authenticate the manager:** The target contract's `upgrade()`
  function MUST call `manager.require_auth()` before
  `update_current_contract_wasm()`. If the target does not authenticate, anyone
  who can invoke the manager's `execute()` can bypass governance and upgrade the
  target directly. See `contracts/example-target/src/lib.rs` for a reference
  implementation.
- **No expiry:** Proposals do not expire. An Active proposal can be finalized
  at any time after the voting period ends, even years later.
- **No WASM compatibility check:** The contract does not verify that the new WASM
  is storage-layout-compatible with the existing target. An incompatible upgrade
  can brick the target contract. Run an upgrade-compatibility check before
  proposing (see Recommended Workflow below).

---

## 🔮 Possible Future Work

- **Sybil resistance:** Integrate with a Stellar identity or token-weighting
  system to prevent multi-address voting attacks.
- **Proposal deposits:** Require proposers to lock a deposit that is forfeited
  if the proposal fails, reducing spam.
- **Proposal expiry:** Automatically expire Active proposals that have not been
  finalized within a configurable window.
- **WASM compatibility check:** Add an on-chain or off-chain verification step
  that checks storage layout compatibility between the old and new WASM.

---

## 📋 Recommended Workflow

1. **Build both WASM versions** (old and new) of the target contract.
2. **Run an upgrade-compatibility check** using a tool such as
   [soroban-upgrade-safeguard](https://github.com/eigercoin/soroban-upgrade-safeguard)
   or an equivalent review process. This checks that the new WASM's storage
   layout is compatible with the existing contract's stored data.
3. **Upload the new WASM** to the network and record the hash.
4. **Create a proposal** with the new WASM hash. Include the compatibility-check
   report link and the WASM hash in the proposal description so voters can
   independently verify.
5. **Vote, finalize, and execute** following the lifecycle above.

> This is guidance, not enforcement. The contract does not verify the
> description or require a compatibility check. Voters should independently
> verify the proposal before voting.

---

## 📋 Error Codes

Every error returned by the contract is a `u32` from the `Error` enum in
[`types.rs`](contracts/upgrade-manager/src/types.rs).

| Code | Name | Trigger | Fix |
|------|------|---------|-----|
| 1 | `AlreadyInitialized` | `initialize` called when a Config entry already exists | Do not call `initialize` more than once |
| 2 | `Unauthorized` | Admin-only function called by a non-admin address | Use the admin address as `caller` |
| 3 | `ProposalNotFound` | `get_proposal`, `vote`, `finalize`, `execute`, or `cancel` with an unknown proposal ID | Use a valid proposal ID returned by `propose_upgrade` |
| 4 | `VotingClosed` | `vote` or `finalize` called on a proposal that is not Active | Only vote on Active proposals; only finalize Active proposals |
| 5 | `AlreadyVoted` | `vote` called by an address that already voted on this proposal | Each address may vote exactly once per proposal |
| 6 | `NotQueued` | `execute` or `time_until_executable` on a proposal that is not Queued | Only execute Queued proposals |
| 7 | `TimelockNotExpired` | `execute` called before `ledger.sequence() >= executable_at` | Wait until `time_until_executable` returns 0 |
| 8 | `ProposalDefeated` | *(never returned — `finalize()` returns `ProposalStatus::Defeated` as a success value, not this error)* | N/A |
| 9 | `DescriptionTooLong` | `propose_upgrade` with a description longer than 256 bytes | Shorten the description to ≤ 256 bytes |
| 10 | `InvalidThreshold` | `initialize` or `update_config` with `approval_threshold_pct` of 0 or > 100 | Use a value in the range 1–100 |
| 11 | `InvalidTimelockDelay` | `initialize` or `update_config` with `timelock_delay` of 0 | Use a value ≥ 1 |
| 12 | `InvalidQuorum` | `initialize` or `update_config` with `quorum` of 0 | Use a value ≥ 1 |
| 13 | `ProposalFinalized` | `cancel` called on a proposal that is Executed, Cancelled, or Defeated | Only cancel Active or Queued proposals |
| 14 | `NotInitialized` | Any function called before `initialize` | Call `initialize` first |
| 15 | `Overflow` | Arithmetic overflow in ledger sequence, vote count, or proposal ID computation | Should not occur in normal operation; indicates integer overflow |
| 16 | `VotingPeriodEnded` | `vote` called when `ledger.sequence() > voting_ends_at` | Vote before the voting period ends |
| 17 | `VotingStillActive` | `finalize` called when `ledger.sequence() <= voting_ends_at` | Wait until the voting period has ended |
| 18 | `EmptyDescription` | `propose_upgrade` with an empty description | Provide a non-empty description (1–256 bytes) |
| 19 | `InvalidVotingPeriod` | `initialize` or `update_config` with `voting_period` of 0 | Use a value ≥ 1 |

> **Note:** `ProposalDefeated` (code 8) is defined in the `Error` enum but is never
> returned by any function. When a proposal fails finalization, `finalize()`
> returns `Ok(ProposalStatus::Defeated)` as a success value, not an error.

---

## 📚 Documentation

| Document | Description |
|---|---|
| [`docs/walkthrough.md`](docs/walkthrough.md) | Step-by-step: deploy example-target, propose, vote, finalize, timelock, execute on testnet |
| [`docs/claims-audit.md`](docs/claims-audit.md) | Audit table: every behavioral claim mapped to its implementing function and test |

---

## 🤝 Contributing

We welcome contributions! Every PR must append a dated, reasoned entry to
`_CHANGELOG.md`. See [CONTRIBUTING.md](CONTRIBUTING.md) for full guidelines.

---

## 📄 License

MIT — see [LICENSE](LICENSE) for details.
