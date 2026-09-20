# Walkthrough: Governance-Gated WASM Upgrade on Testnet

This guide walks through the complete lifecycle of a governed contract upgrade using
`soroban-upgrade-manager` on Stellar Testnet. It covers every step from uploading
the new WASM binary through proposal creation, voting, timelock expiry, and execution.

---

## Prerequisites

- Stellar CLI v22+ — https://developers.stellar.org/docs/tools/developer-tools/stellar-cli
- Rust with `wasm32-unknown-unknown` target:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- A funded testnet account (steps below create and fund one automatically)

---

## Overview

```
┌─────────────────────────────────────────────────────────────────┐
│  1. Upload new WASM to Stellar network                          │
│  2. Deploy upgrade-manager contract                             │
│  3. Deploy the target contract (the one to be upgraded)         │
│  4. Initialize upgrade-manager (admin, timelock, quorum)        │
│  5. propose_upgrade() — create a governance proposal            │
│  6. vote() × N — token holders cast YES/NO votes               │
│  7. finalize() — close voting after timelock elapses           │
│  8. execute() — run the cross-contract upgrade                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Step 0: Setup

```bash
# Configure testnet
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"

# Generate and fund accounts
stellar keys generate admin    --network testnet
stellar keys generate voter1   --network testnet
stellar keys generate voter2   --network testnet
stellar keys generate voter3   --network testnet
stellar keys fund admin   --network testnet
stellar keys fund voter1  --network testnet
stellar keys fund voter2  --network testnet
stellar keys fund voter3  --network testnet

# Store addresses for convenience
ADMIN_ADDR=$(stellar keys address admin   --network testnet)
VOTER1=$(stellar keys address voter1  --network testnet)
VOTER2=$(stellar keys address voter2  --network testnet)
VOTER3=$(stellar keys address voter3  --network testnet)

echo "Admin: $ADMIN_ADDR"
echo "Voter1: $VOTER1"
echo "Voter2: $VOTER2"
echo "Voter3: $VOTER3"
```

---

## Step 1: Build and Upload the New WASM

Before proposing an upgrade, the new WASM binary must be uploaded to the
Stellar network. This produces a 32-byte hash that the proposal references.

```bash
# Build the upgrade-manager (or your target contract)
cargo build --target wasm32-unknown-unknown --release \
  --manifest-path contracts/upgrade-manager/Cargo.toml

WASM_PATH="target/wasm32-unknown-unknown/release/soroban_upgrade_manager.wasm"

# Upload the WASM to the network — this does NOT deploy a contract.
# It registers the binary and returns the hash.
NEW_WASM_HASH=$(stellar contract upload \
  --wasm "$WASM_PATH" \
  --source admin \
  --network testnet)

echo "Uploaded WASM hash: $NEW_WASM_HASH"
```

> The WASM hash is a content-addressed key: the same binary always produces the
> same hash. You can verify this hash in Stellar Explorer before voting begins.

---

## Step 2: Deploy the Upgrade Manager

```bash
MANAGER_WASM="target/wasm32-unknown-unknown/release/soroban_upgrade_manager.wasm"

MANAGER_ID=$(stellar contract deploy \
  --wasm "$MANAGER_WASM" \
  --source admin \
  --network testnet)

echo "Upgrade Manager contract ID: $MANAGER_ID"
```

---

## Step 3: Deploy the Target Contract

For this walkthrough, we deploy a second instance of the upgrade-manager as the
"target" contract to be upgraded. In production this would be your actual application
contract.

```bash
TARGET_ID=$(stellar contract deploy \
  --wasm "$MANAGER_WASM" \
  --source admin \
  --network testnet)

echo "Target contract ID: $TARGET_ID"
```

> **Storage layout note:** If you are upgrading a production contract, the new WASM
> must be storage-layout-compatible with the existing contract's stored data. Adding
> new storage keys is safe; removing or retyping existing keys will cause panics when
> the contract reads old storage. Confirm any layout changes before proposing.

---

## Step 4: Initialize the Upgrade Manager

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- initialize \
  --admin "$ADMIN_ADDR" \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

Configuration used here:
| Parameter | Value | Meaning |
|---|---|---|
| `timelock_delay` | `17280` | ≈ 24 hours at 5 s/ledger |
| `quorum` | `3` | At least 3 total votes required |
| `approval_threshold_pct` | `51` | Simple majority (>51% YES) |

---

## Step 5: Propose an Upgrade

```bash
PROPOSAL_ID=$(stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- propose_upgrade \
  --proposer "$ADMIN_ADDR" \
  --target "$TARGET_ID" \
  --new_wasm_hash "$NEW_WASM_HASH" \
  --description "Upgrade target to v2: add governance config validation")

echo "Proposal ID: $PROPOSAL_ID"
# Expected: 0
```

At this point the proposal is in **Active** status. Voters have until
`executable_at` ledger (created_at + 17280) to cast their votes.

Verify the proposal:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
```

---

## Step 6: Vote

Each voter can vote YES (`true`) or NO (`false`) exactly once.

```bash
# Voter 1 votes YES
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source voter1 \
  --network testnet \
  -- vote \
  --voter "$VOTER1" \
  --proposal_id 0 \
  --approve true

# Voter 2 votes YES
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source voter2 \
  --network testnet \
  -- vote \
  --voter "$VOTER2" \
  --proposal_id 0 \
  --approve true

# Voter 3 votes YES
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source voter3 \
  --network testnet \
  -- vote \
  --voter "$VOTER3" \
  --proposal_id 0 \
  --approve true
```

Check current vote counts:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
# yes_votes: 3, no_votes: 0, status: Active
```

Check how many ledgers remain until execution:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- time_until_executable \
  --proposal_id 0
# Returns remaining ledgers (starts at 17280, counts down to 0)
```

---

## Step 7: Finalize Voting

After the timelock delay has elapsed (~24 hours), call `finalize` to close voting
and move the proposal to **Queued** (if quorum and threshold are met) or **Defeated**.

```bash
# Wait until time_until_executable returns 0, then:
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- finalize \
  --proposal_id 0
# Expected result: "Queued"
```

If the proposal is **Defeated** (quorum not met or threshold not reached), the
upgrade will not be executed. You can create a new proposal after investigating why.

---

## Step 8: Execute the Upgrade

Once finalized as **Queued**, anyone can call `execute`. The upgrade manager calls
the target contract's `upgrade()` function with the new WASM hash, which internally
calls `env.deployer().update_current_contract_wasm()`.

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- execute \
  --proposal_id 0
```

After this transaction confirms, the target contract's WASM is replaced with the
new version. Verify:

```bash
# Check proposal is now Executed
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
# status: Executed
```

---

## Emergency Cancel

If a proposal needs to be stopped (before or after voting, before execution), the
admin can cancel it:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- cancel \
  --caller "$ADMIN_ADDR" \
  --proposal_id 0
```

Only `Active` and `Queued` proposals can be cancelled. `Executed` and `Cancelled`
proposals are immutable.

---

## Full Timeline

```
Day 0  │  Upload WASM → get hash
       │  Deploy upgrade-manager + target contract
       │  Initialize upgrade-manager
       │  propose_upgrade() → Proposal #0 is Active
       │
       │  Voters cast YES/NO votes throughout the day
       │
Day 1  │  timelock_delay (17280 ledgers ≈ 24 h) elapses
       │
       │  finalize() → Proposal #0 moves to Queued
       │
       │  (Optional: additional review period)
       │
       │  execute() → Target contract WASM is replaced
       │             Proposal #0 moves to Executed
```

---

## Failure Modes

| Scenario | Error | Resolution |
|---|---|---|
| `finalize` before timelock | `TimelockNotExpired` | Wait until `time_until_executable` returns 0 |
| `finalize` with <3 votes | `Defeated` | Create a new proposal; improve voter outreach |
| `finalize` with <51% YES | `Defeated` | Create a new proposal with revised upgrade |
| `execute` on non-Queued proposal | `NotQueued` | Check proposal status with `get_proposal` |
| Trying to vote twice | `AlreadyVoted` | Each address may vote exactly once per proposal |
| WASM hash not uploaded | Transaction error | Run `stellar contract upload` first |
| Target contract missing `upgrade()` | Invocation panic | Ensure target implements the standard Soroban upgrade interface |

---

## What the `upgrade()` Function Looks Like on the Target

For the upgrade manager to succeed, the target contract must expose an `upgrade`
function that calls `env.deployer().update_current_contract_wasm()`:

```rust
// In your target contract's lib.rs:
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
    // Add auth here if needed (e.g., only callable from the upgrade manager)
    env.deployer().update_current_contract_wasm(new_wasm_hash);
}
```

This is the standard Soroban upgrade interface. The upgrade manager calls it via
`env.invoke_contract(&target, &symbol_short!("upgrade"), vec![&env, hash.to_val()])`.

---

## See Also

- [README.md](../README.md) — overview, features, configuration reference
- [contracts/upgrade-manager/src/lib.rs](../contracts/upgrade-manager/src/lib.rs) — contract source
- [contracts/upgrade-manager/src/governance.rs](../contracts/upgrade-manager/src/governance.rs) — voting logic
- [contracts/upgrade-manager/src/timelock.rs](../contracts/upgrade-manager/src/timelock.rs) — timelock enforcement
