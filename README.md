# 🔄 Soroban Upgrade Manager

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Stellar](https://img.shields.io/badge/Built%20on-Stellar-blue)](https://stellar.org)
[![Soroban](https://img.shields.io/badge/Smart%20Contracts-Soroban-purple)](https://soroban.stellar.org)
[![good first issues](https://img.shields.io/github/issues/ogenyialice120/soroban-upgrade-manager/good%20first%20issue)](https://github.com/ogenyialice120/soroban-upgrade-manager/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22)
[![CI](https://github.com/ogenyialice120/soroban-upgrade-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/ogenyialice120/soroban-upgrade-manager/actions/workflows/ci.yml)

A Soroban smart contract that manages **WASM upgrades for other contracts** through on-chain governance votes and a configurable **timelock delay** before execution.

Most teams handle contract upgrades manually — a single admin key calls `upgrade()` directly. This contract replaces that pattern with a transparent, auditable process: a vote must pass, and a mandatory delay must elapse before any code change goes live. No more surprise upgrades.

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
| `initialize` | Set admin and config | None (once only) |
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

## 🤝 Contributing

We welcome contributions! This project participates in the **[Stellar Wave Program](https://www.drips.network/wave/stellar)** — fix issues, earn rewards.

See [CONTRIBUTING.md](CONTRIBUTING.md) for full guidelines.

**Quick start:**
1. Browse [open issues](https://github.com/ogenyialice120/soroban-upgrade-manager/issues) labelled `good first issue` or `Stellar Wave`
2. Comment on the issue to apply
3. Fork → branch → PR

---

## 📄 License

MIT — see [LICENSE](LICENSE) for details.

---

## 🌊 Stellar Wave Program

This repository participates in the **[Stellar Wave Program](https://www.drips.network/wave/stellar)** by Drips Network. Contributors who resolve issues during an active Wave earn Points that translate to real rewards from the Stellar Development Foundation.

**Fix. Merge. Earn. 🌊**
