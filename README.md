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
| Is there a mandatory delay? | No — upgrades are instant | Configurable timelock (default ≈ 24 h) |
| Is the upgrade auditable on-chain? | No proposal trail | Full proposal + vote history on-chain |
| Can the community stop a bad upgrade? | Only if you trust the admin | Admin can cancel; voters can reject |
| Can you upgrade a **different** contract? | No — only self-upgrade | Yes — cross-contract upgrade via `invoke_contract` |

**The real problem this solves:** A single admin key controlling an upgrade path
is a single point of failure and a source of trust risk for users. If the admin
key is compromised, the attacker can silently swap in malicious WASM. If the admin
is a team, users have to trust that the team will not rug them.

Soroban Upgrade Manager replaces that single-key trust with:
1. A **transparent proposal** that anyone can inspect before it executes.
2. A **vote** that requires a quorum of token holders to approve.
3. A **mandatory delay** between approval and execution, giving users time to exit
   if they disagree with the upgrade.

This is the same pattern used by Compound Governor Bravo and OpenZeppelin's
TimelockController, adapted for Soroban's contract model.

---

---

## ✨ Features

| Feature | Description |
|---|---|
| 🗳️ **Governance Voting** | Any address can propose an upgrade; token holders vote YES/NO |
| ⏱️ **Configurable Timelock** | Mandatory delay between vote passing and execution (default ≈ 24 h) |
| 🔗 **Cross-Contract Upgrade** | Executes `upgrade()` on the target contract after the timelock expires |
| 📊 **Quorum & Threshold** | Configurable minimum vote count and YES-vote percentage |
| 🚨 **Emergency Cancel** | Admin can cancel any Active or Queued proposal |
| 🔑 **Admin Transfer** | Admin role can be transferred to a multisig or DAO |
| 📖 **Full Audit Trail** | All proposals and votes are persisted on-chain |

---

## 🏗️ Architecture

```
soroban-upgrade-manager/
├── contracts/
│   └── upgrade-manager/
│       ├── src/
│       │   ├── lib.rs          # Contract entry point & public API
│       │   ├── governance.rs   # Proposal creation, voting, finalization
│       │   ├── timelock.rs     # Timelock enforcement helpers
│       │   └── types.rs        # Shared types, storage keys, errors
│       └── Cargo.toml
├── Cargo.toml                  # Workspace manifest
├── README.md
└── CONTRIBUTING.md
```

### Proposal Lifecycle

```
propose_upgrade()
      │
      ▼
  [Active] ──── vote() × N ────►  finalize_voting()
      │                                  │
      │                         pass?    │    fail?
      │                          ▼       │      ▼
      │                       [Queued]   │  [Defeated]
      │                          │       │
      │              timelock    │       │
      │              elapsed?    ▼       │
      │                       execute()  │
      │                          │       │
      │                          ▼       │
      └──── cancel() ──────► [Cancelled] │
                                     [Executed]
```

---

## 🚀 Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) 1.74+
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/stellar-cli) (`stellar` v22+)

### Build

```bash
git clone https://github.com/ogenyialice120/soroban-upgrade-manager.git
cd soroban-upgrade-manager

# Add the Wasm target
rustup target add wasm32-unknown-unknown

# Build
cargo build --target wasm32-unknown-unknown --release \
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
  --wasm target/wasm32-unknown-unknown/release/soroban_upgrade_manager.wasm \
  --source alice \
  --network testnet
```

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
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

> `timelock_delay` is in ledgers. At ~5 s/ledger, 17 280 ≈ 24 hours.

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
  --source alice \
  --network testnet \
  -- finalize \
  --proposal_id 0
```

### 5. Execute the upgrade

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- execute \
  --proposal_id 0
```

---

## 🔧 Configuration Reference

| Parameter | Type | Description |
|---|---|---|
| `timelock_delay` | `u32` | Ledgers between proposal creation and earliest execution. Min: 1. |
| `quorum` | `u32` | Minimum total votes (YES + NO) for a proposal to be considered. Min: 1. |
| `approval_threshold_pct` | `u32` | Percentage of YES votes required (1–100). E.g. `51` = simple majority. |

---

## 📋 Contract API

| Function | Description | Auth |
|---|---|---|
| `initialize` | Set admin and config | None (fails if already initialised) |
| `propose_upgrade` | Create an upgrade proposal | Proposer |
| `vote` | Cast YES/NO on a proposal | Voter |
| `finalize` | Close voting after timelock | Anyone |
| `execute` | Execute the upgrade | Anyone |
| `cancel` | Cancel a proposal | Admin |
| `update_config` | Change governance params | Admin |
| `transfer_admin` | Hand off admin role | Admin |
| `get_proposal` | Fetch proposal by ID | Read-only |
| `get_config` | Fetch governance config | Read-only |
| `get_admin` | Fetch admin address | Read-only |
| `time_until_executable` | Ledgers until timelock expires | Read-only |

---

## 📚 Documentation

| Document | Description |
|---|---|
| [`docs/walkthrough.md`](docs/walkthrough.md) | Step-by-step: propose → vote → timelock → execute on testnet |

---

## 🤝 Contributing

We welcome contributions! This project welcomes contributions from the community..

See [CONTRIBUTING.md](CONTRIBUTING.md) for full guidelines.

---

## 📄 License

MIT — see [LICENSE](LICENSE) for details.

