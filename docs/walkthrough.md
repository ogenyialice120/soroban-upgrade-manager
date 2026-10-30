# Walkthrough: Governance-Gated WASM Upgrade on Testnet

This guide walks through the complete lifecycle of a governed contract upgrade using
`soroban-upgrade-manager` on Stellar Testnet. It covers every step from deploying the
example-target contract through proposal creation, voting, finalization, timelock
expiry, and execution.

---

## Prerequisites

- Stellar CLI v22+ — https://developers.stellar.org/docs/tools/developer-tools/stellar-cli
- Rust with `wasm32v1-none` target:
  ```bash
  rustup target add wasm32v1-none
  ```
- A funded testnet account (steps below create and fund one automatically)

---

## Overview

The upgrade manager governs upgrades to a **target** contract. In this walkthrough
the target is the bundled `example-target` contract — a minimal contract with a
`version()` function and an `upgrade(new_wasm_hash)` function that authenticates the
upgrade manager before replacing its WASM.

```
1. Build and upload the new (v2) target WASM → get hash
2. Deploy the upgrade-manager contract
3. Deploy example-target (constructor stores the manager address)
4. Initialize upgrade-manager (admin, voting_period, timelock_delay, quorum, threshold)
5. propose_upgrade() — create a governance proposal (Active)
6. vote() × N — any address casts one YES/NO vote during the voting period
7. finalize() — after the voting period ends; moves to Queued (pass) or Defeated (fail)
8. Wait for the timelock delay to elapse (starts at finalize time)
9. execute() — anyone calls; manager invokes target.upgrade(new_wasm_hash)
```

### Two distinct time windows

```
 propose_upgrade          voting_ends_at              executable_at
     │                         │                            │
     ▼                         ▼                            ▼
  [Active] ── voting_period ──► finalize() ── timelock_delay ──► execute()
                                 │
                    pass? ──► [Queued] ── wait ──► execute() ──► [Executed]
                    fail? ──► [Defeated]
```

- **Voting period** (`voting_period`): ledgers between proposal creation and the
  end of voting. Votes are accepted while `ledger.sequence() <= voting_ends_at`.
- **Timelock delay** (`timelock_delay`): ledgers between finalization and earliest
  execution. `executable_at` is set to `finalize_ledger + timelock_delay` when
  `finalize()` is called. The timelock starts when someone calls `finalize()`, not
  when the proposal is created.

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

## Step 1: Build and Upload the New (v2) Target WASM

Before proposing an upgrade, the new WASM binary must be uploaded to the
Stellar network. This produces a 32-byte hash that the proposal references.

```bash
# Build the v2 target WASM (example-target with --features v2)
cargo build --target wasm32v1-none --release \
  --manifest-path contracts/example-target/Cargo.toml \
  --features v2

V2_WASM_PATH="target/wasm32v1-none/release/example_target.wasm"

# Upload the WASM to the network — this does NOT deploy a contract.
# It registers the binary and returns the hash.
NEW_WASM_HASH=$(stellar contract upload \
  --wasm "$V2_WASM_PATH" \
  --source admin \
  --network testnet)

echo "Uploaded v2 WASM hash: $NEW_WASM_HASH"
```

> The WASM hash is a content-addressed key: the same binary always produces the
> same hash. You can verify this hash in Stellar Explorer before voting begins.

---

## Step 2: Build and Deploy the Upgrade Manager

```bash
# Build the upgrade-manager WASM
cargo build --target wasm32v1-none --release \
  --manifest-path contracts/upgrade-manager/Cargo.toml

MANAGER_WASM="target/wasm32v1-none/release/soroban_upgrade_manager.wasm"

MANAGER_ID=$(stellar contract deploy \
  --wasm "$MANAGER_WASM" \
  --source admin \
  --network testnet)

echo "Upgrade Manager contract ID: $MANAGER_ID"
```

---

## Step 3: Deploy the Target Contract (example-target)

The target contract is the contract you want to upgrade. In this walkthrough we
use the bundled `example-target` contract, which has:

- `__constructor(manager)` — stores the upgrade-manager address at deploy time.
- `version()` — returns 1 (or 2 when built with `--features v2`).
- `upgrade(new_wasm_hash)` — authenticates the stored manager, then calls
  `env.deployer().update_current_contract_wasm(new_wasm_hash)`.
- `get_manager()` — returns the stored manager address.

```bash
# Build the v1 target WASM (no features)
cargo build --target wasm32v1-none --release \
  --manifest-path contracts/example-target/Cargo.toml

TARGET_WASM="target/wasm32v1-none/release/example_target.wasm"

# Deploy example-target, passing the manager address to the constructor.
# The constructor stores the manager without requiring auth (the manager is a
# contract address and cannot sign), so no key needs to sign for the constructor.
TARGET_ID=$(stellar contract deploy \
  --wasm "$TARGET_WASM" \
  --source admin \
  --network testnet \
  -- \
  --manager "$MANAGER_ID")

echo "Target contract ID: $TARGET_ID"
```

Verify the target is running v1:

```bash
stellar contract invoke \
  --id "$TARGET_ID" \
  --network testnet \
  -- version
# Expected: 1
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
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

Configuration used in this walkthrough:

| Parameter | Value | Meaning |
|---|---|---|
| `voting_period` | `17280` | ≈ 24 hours at 5 s/ledger — ledgers during which votes are accepted |
| `timelock_delay` | `17280` | ≈ 24 hours — ledgers between `finalize()` and earliest `execute()` |
| `quorum` | `3` | At least 3 total votes (YES + NO) required for the proposal to pass |
| `approval_threshold_pct` | `51` | At least 51% of total votes must be YES |

All parameters must be >= 1 (`approval_threshold_pct` must be 1–100).
The `initialize` function can only be called once; the admin must sign the
transaction.

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
  --description "Upgrade example-target from v1 to v2")

echo "Proposal ID: $PROPOSAL_ID"
# Expected: 0
```

At this point the proposal is in **Active** status. The voting period ends at
`voting_ends_at` = `created_at + voting_period` (in ledgers). Anyone can vote
while `ledger.sequence() <= voting_ends_at`.

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

Each address can vote YES (`true`) or NO (`false`) exactly once per proposal.
There is no token-weighted voting — every address gets one equal vote.

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

---

## Step 7: Finalize Voting

After the **voting period** has ended (`ledger.sequence() > voting_ends_at`),
anyone can call `finalize` to close voting and determine the outcome.

The proposal passes if:
1. Total votes (YES + NO) >= `quorum` (3 in this walkthrough), AND
2. YES votes as a percentage of total votes >= `approval_threshold_pct` (51%).

On pass, the proposal moves to **Queued** and `executable_at` is set to
`current_ledger + timelock_delay`. On fail, it moves to **Defeated**.

```bash
# Wait for the voting period to elapse (~24 hours), then:
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- finalize \
  --proposal_id 0
# Expected result: "Queued"
```

> `finalize` does not require authentication — anyone can call it. It only
> succeeds if the voting period has ended. If called too early, it returns
> `VotingStillActive`.

If the proposal is **Defeated** (quorum not met or threshold not reached), the
upgrade will not be executed. You can create a new proposal after investigating why.

---

## Step 8: Wait for the Timelock

Once the proposal is **Queued**, the timelock delay begins counting down from
the ledger at which `finalize()` was called. Anyone can call `execute()` once
`ledger.sequence() >= executable_at`.

Check how many ledgers remain until execution:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- time_until_executable \
  --proposal_id 0
# Returns remaining ledgers (0 means execution is permitted)
```

> `time_until_executable` returns 0 for proposals that are not Queued, or when
> the timelock has already expired.

---

## Step 9: Execute the Upgrade

Once the timelock has expired, anyone can call `execute`. The upgrade manager
calls the target contract's `upgrade()` function via cross-contract invocation,
passing the new WASM hash. The target contract authenticates that the call comes
from the stored manager address, then calls
`env.deployer().update_current_contract_wasm(new_wasm_hash)`.

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network testnet \
  -- execute \
  --proposal_id 0
```

> `execute` does not require authentication — anyone can call it. It only
> succeeds if the proposal is Queued and the timelock has expired.

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

# Verify the target now runs v2
stellar contract invoke \
  --id "$TARGET_ID" \
  --network testnet \
  -- version
# Expected: 2
```

---

## Emergency Cancel

If a proposal needs to be stopped, the admin can cancel any **Active** or
**Queued** proposal:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- cancel \
  --caller "$ADMIN_ADDR" \
  --proposal_id 0
```

Only Active and Queued proposals can be cancelled. Cancelling a Defeated,
Executed, or already-Cancelled proposal returns `ProposalFinalized`.

---

## Full Timeline

```
Day 0  │  Upload v2 WASM → get hash
       │  Deploy upgrade-manager + example-target
       │  Initialize upgrade-manager (voting_period=17280, timelock_delay=17280)
       │  propose_upgrade() → Proposal #0 is Active
       │
       │  Voters cast YES/NO votes (any address, one vote each)
       │
Day 1  │  voting_period (17280 ledgers ≈ 24 h) elapses
       │
       │  finalize() → Proposal #0 moves to Queued
       │  executable_at = finalize_ledger + timelock_delay
       │
Day 2  │  timelock_delay (17280 ledgers ≈ 24 h) elapses
       │
       │  execute() → Manager calls target.upgrade(new_wasm_hash)
       │             Target WASM is replaced (v1 → v2)
       │             Proposal #0 moves to Executed
```

---

## Failure Modes

| Scenario | Error | Resolution |
|---|---|---|
| `vote` after voting period ends | `VotingPeriodEnded` | Votes are only accepted while `ledger.sequence() <= voting_ends_at` |
| `finalize` before voting period ends | `VotingStillActive` | Wait until `ledger.sequence() > voting_ends_at` |
| `finalize` with < 3 votes | Proposal is **Defeated** (not an error) | Create a new proposal; improve voter outreach |
| `finalize` with < 51% YES | Proposal is **Defeated** (not an error) | Create a new proposal with revised upgrade |
| `execute` before timelock expires | `TimelockNotExpired` | Wait until `time_until_executable` returns 0 |
| `execute` on non-Queued proposal | `NotQueued` | Check proposal status with `get_proposal` |
| Voting twice | `AlreadyVoted` | Each address may vote exactly once per proposal |
| Cancelling a Defeated/Executed/Cancelled proposal | `ProposalFinalized` | Only Active and Queued proposals can be cancelled |
| WASM hash not uploaded | Transaction error | Run `stellar contract upload` first |
| Target contract missing `upgrade()` | Host-level error (reverts transaction) | Ensure target implements `upgrade(new_wasm_hash)` |

---

## What the `upgrade()` Function Looks Like on the Target

For the upgrade manager to succeed, the target contract must expose an `upgrade`
function that authenticates the upgrade manager and then calls
`env.deployer().update_current_contract_wasm()`:

```rust
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
    let manager: Address = env
        .storage()
        .instance()
        .get(&symbol_short!("manager"))
        .expect("manager not set");
    // Only the stored manager may trigger an upgrade.
    manager.require_auth();
    env.deployer().update_current_contract_wasm(new_wasm_hash);
}
```

The upgrade manager calls it via
`env.invoke_contract(&target, &symbol_short!("upgrade"), vec![&env, hash.to_val()])`.

See [`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs)
for the full reference implementation.

---

## See Also

- [README.md](../README.md) — overview, features, configuration reference
- [contracts/upgrade-manager/src/lib.rs](../contracts/upgrade-manager/src/lib.rs) — contract source
- [contracts/upgrade-manager/src/governance.rs](../contracts/upgrade-manager/src/governance.rs) — voting logic
- [contracts/upgrade-manager/src/timelock.rs](../contracts/upgrade-manager/src/timelock.rs) — timelock enforcement
- [contracts/example-target/src/lib.rs](../contracts/example-target/src/lib.rs) — example target contract
