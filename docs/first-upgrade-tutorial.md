# First Upgrade Tutorial

This tutorial walks a complete beginner — no Stellar CLI experience assumed —
through their first governed contract upgrade on the Stellar **testnet**. By the
end you will have deployed two contracts, run a governance vote, waited out a
timelock, and verified that an `example-target` contract upgraded itself from
version 1 to version 2.

It uses short, fast parameters (`voting_period=20`, `timelock_delay=20` ledgers)
so the whole run takes about five minutes of waiting, not two days. Everything
here runs on testnet with free test funds. No real money is involved.

> Already comfortable with the Stellar CLI and want the full lifecycle with
> production-sized parameters and multiple voters? See
> [`docs/walkthrough.md`](walkthrough.md). This tutorial is a simpler,
> self-contained subset; the walkthrough has the deeper explanations.

---

## What you will do

1. Install the Stellar CLI, Rust, and build the contracts.
2. Create and fund a free testnet account.
3. Deploy the **upgrade manager** contract.
4. Deploy the **example-target** contract (version 1) and confirm it returns
   `1`.
5. Initialize the manager with fast parameters.
6. Build version 2 of the target, upload it, and record its hash.
7. Propose the upgrade, vote YES, and wait out the voting period.
8. Finalize, wait out the timelock, execute, and confirm the target now returns
   `2`.

A single test account (`alice`) plays every role: admin, proposer, and voter.
Because the quorum is set to `1`, one YES vote is enough to pass. This keeps the
tutorial solo and simple. The trade-off — that one-address-one-vote voting is
not sybil-resistant — is explained in the README's
[Security Model & Limitations](../README.md#-security-model--limitations).

### How long the waits take

Stellar testnet produces a new ledger roughly every 5–7 seconds. The
`voting_period` and `timelock_delay` are measured in ledgers, so:

- `voting_period=20` ledgers ≈ 100–140 seconds (~2 minutes).
- `timelock_delay=20` ledgers ≈ another 100–140 seconds.

You will wait twice for about two minutes each. The tutorial says exactly when.

---

## Step 1: Install the Stellar CLI

Install the official Stellar CLI with its install script (Homebrew users on
macOS can instead run `brew install stellar-cli`):

```bash
curl -fsSL https://github.com/stellar/stellar-cli/raw/main/install.sh | sh
```

Verify it is installed:

```bash
stellar --version
```

> Building the CLI from source with `cargo install stellar-cli` can run out of
> memory in small environments and needs a newer Rust than this repo pins. Use
> the install script above unless you have a specific reason not to.

---

## Step 2: Install Rust and the WASM build target

The contracts are written in Rust and compiled to WebAssembly. Install Rust via
[rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

This repository pins a Rust version in `rust-toolchain.toml`, so once you are in
the repo directory `rustup` will select it automatically. Add the WASM target the
network requires:

```bash
rustup target add wasm32v1-none
```

> The target is `wasm32v1-none`, not `wasm32-unknown-unknown`. Stellar's network
> rejects WASM built with `wasm32-unknown-unknown` on Rust 1.82+ with the error
> "reference-types not enabled".

---

## Step 3: Clone the repo and build the contracts

```bash
git clone https://github.com/ogenyialice120/soroban-upgrade-manager.git
cd soroban-upgrade-manager
```

Build the upgrade manager and the version-1 example target:

```bash
# Upgrade manager
cargo build --release --target wasm32v1-none \
  --manifest-path contracts/upgrade-manager/Cargo.toml

# Example target (version 1, no features)
cargo build --release --target wasm32v1-none \
  --manifest-path contracts/example-target/Cargo.toml
```

The compiled WASM files land in `target/wasm32v1-none/release/`:

- `soroban_upgrade_manager.wasm` — the upgrade manager.
- `example_target.wasm` — the example target (version 1).

You will build version 2 of the target later in Step 6.

---

## Step 4: Configure the testnet network

Tell the Stellar CLI how to reach the public testnet (you only do this once per
machine):

```bash
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"
```

---

## Step 5: Create and fund a test account

Generate a key named `alice` on testnet and fund it with free test lumens via
Friendbot:

```bash
stellar keys generate alice --network testnet
stellar keys fund alice --network testnet
```

`stellar keys generate` creates a keypair stored under the name `alice`.
`stellar keys fund` requests free test tokens from Friendbot. You can refer to
this account by the name `alice` in later commands; the CLI resolves the name
to its address automatically.

See the address behind the name (a `G…` string):

```bash
stellar keys address alice --network testnet
```

---

## Step 6: Deploy the upgrade manager

Deploy the manager contract. It has no constructor, so no constructor arguments
are needed:

```bash
MANAGER_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/soroban_upgrade_manager.wasm \
  --source alice \
  --network testnet)

echo "Upgrade manager contract ID: $MANAGER_ID"
```

The output is a `C…` contract ID. We save it in `MANAGER_ID` to reuse in the
next steps.

---

## Step 7: Deploy the example target (version 1)

The example target's constructor takes one argument: the manager contract ID
that is allowed to upgrade it. Pass it with `-- --manager "$MANAGER_ID"` (the
`--` separates the CLI's own flags from the constructor arguments):

```bash
TARGET_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/example_target.wasm \
  --source alice \
  --network testnet \
  -- \
  --manager "$MANAGER_ID")

echo "Example target contract ID: $TARGET_ID"
```

The target stores the manager address at deploy time, so only the manager can
trigger an upgrade later. Confirm the target is running version 1:

```bash
stellar contract invoke \
  --id "$TARGET_ID" \
  --source alice \
  --network testnet \
  -- version
# Expected: 1
```

---

## Step 8: Initialize the upgrade manager

Initialize the manager with one admin (`alice`) and the fast parameters. Every
parameter except `proposal_min_interval` must be at least 1; `proposal_min_interval`
may be 0 to disable the rate limit:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- initialize \
  --admin alice \
  --voting_period 20 \
  --timelock_delay 20 \
  --quorum 1 \
  --approval_threshold_pct 51 \
  --proposal_min_interval 0
```

What each value means for this tutorial:

| Parameter | Value | Meaning |
|---|---|---|
| `voting_period` | `20` | Votes are accepted for ~2 minutes (20 ledgers) |
| `timelock_delay` | `20` | ~2-minute wait between finalize and execute |
| `quorum` | `1` | One total vote is enough to pass |
| `approval_threshold_pct` | `51` | At least 51% of votes must be YES |
| `proposal_min_interval` | `0` | No rate limit on proposing (disabled) |

`initialize` can only be called once. The admin must sign, which `--source
alice` provides. You can read the saved config back to confirm:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- get_config
```

---

## Step 9: Build version 2 of the target and upload it

Now build version 2 of the same example target by enabling the `v2` feature.
This **overwrites** `example_target.wasm` with the v2 binary, so capture the v1
hash or redeploy only if needed:

```bash
cargo build --release --target wasm32v1-none \
  --manifest-path contracts/example-target/Cargo.toml \
  --features v2
```

Upload the v2 WASM to the network. Uploading registers the binary and returns its
hash; it does **not** deploy a contract:

```bash
V2_HASH=$(stellar contract upload \
  --wasm target/wasm32v1-none/release/example_target.wasm \
  --source alice \
  --network testnet)

echo "v2 WASM hash: $V2_HASH"
```

`V2_HASH` is a 64-character hex string. The proposal in the next step references
this hash; executing the proposal will swap the target's WASM to this binary.

---

## Step 10: Propose the upgrade

Create a proposal to upgrade the target to the v2 hash. The `propose_upgrade`
function requires seven arguments — `proposer`, `target`, `new_wasm_hash`,
`title`, `description`, and `metadata_hash` — so the command includes all of
them. Use 64 hex zeros for `--metadata_hash` when there is no off-chain metadata:

```bash
PROPOSAL_ID=$(stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- propose_upgrade \
  --proposer alice \
  --target "$TARGET_ID" \
  --new_wasm_hash "$V2_HASH" \
  --title "Upgrade example-target to v2" \
  --description "Upgrade example-target from v1 to v2" \
  --metadata_hash 0000000000000000000000000000000000000000000000000000000000000000)

echo "Proposal ID: $PROPOSAL_ID"
# Expected: 0
```

The proposal is now **Active**. Inspect it:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
```

You should see `status: Active` and `yes_votes: 0`.

---

## Step 11: Vote YES

Cast a YES vote. `alice` is both the admin and the only voter here, and quorum
is 1, so one vote is enough:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- vote \
  --voter alice \
  --proposal_id 0 \
  --approve true
```

Each address may vote only once per proposal. Confirm the vote was recorded:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
# yes_votes: 1, no_votes: 0, status: Active
```

---

## Step 12: Wait for the voting period, then finalize

The voting period is 20 ledgers (~2 minutes). Wait about two minutes, then close
voting. `finalize` can be called by anyone and only succeeds once the voting
period has ended:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- finalize \
  --proposal_id 0
```

- If the voting period has ended and the vote met quorum and threshold, this
  prints `Queued`.
- If you finalize too early, it returns `VotingStillActive` — wait a little
  longer and try again.

On success the proposal moves to **Queued** and `executable_at` is set to
`finalize_ledger + timelock_delay`. The timelock starts now, not when the
proposal was created.

You can ask how many ledgers remain until execution is allowed:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- time_until_executable \
  --proposal_id 0
# Returns remaining ledgers; 0 means execution is permitted
```

---

## Step 13: Wait for the timelock, then execute

The timelock is another 20 ledgers (~2 minutes). Wait about two minutes until
`time_until_executable` returns `0`, then execute. `execute` can be called by
anyone once the timelock has expired:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- execute \
  --proposal_id 0
```

- If the timelock has not expired, it returns `TimelockNotExpired` — wait a
  little longer and try again.
- If the proposal is not Queued (for example still Active), it returns
  `NotQueued`.

On success, the manager calls the target's `upgrade(new_wasm_hash)` function via
a cross-contract call, the target authenticates that the call came from the
stored manager, and the target swaps its WASM to the v2 binary.

---

## Step 14: Verify the version changed

Confirm the target now runs version 2:

```bash
stellar contract invoke \
  --id "$TARGET_ID" \
  --source alice \
  --network testnet \
  -- version
# Expected: 2
```

And confirm the proposal is now `Executed`:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source alice \
  --network testnet \
  -- get_proposal \
  --proposal_id 0
# status: Executed
```

That is the full lifecycle: deploy, initialize, propose, vote, finalize, wait,
execute, verify. The target went from returning `1` to returning `2`.

---

## Recap of the two time windows

```
 propose_upgrade   voting_ends_at            executable_at
       │                 │                          │
       ▼                 ▼                          ▼
   [Active] ─ 20 ledgers ─► finalize() ─ 20 ledgers ─► execute()
                              │
                  pass ─► [Queued] ── wait ──► execute() ──► [Executed]
```

- **Voting period** (`voting_period=20`): ledgers between proposal creation and
  the end of voting. Votes are accepted while
  `ledger.sequence() <= voting_ends_at`.
- **Timelock delay** (`timelock_delay=20`): ledgers between `finalize()` and the
  earliest `execute()`. The timelock starts when someone calls `finalize()`,
  not when the proposal is created.

These are two separate windows. If nobody calls `finalize()`, a passed proposal
stays Active indefinitely — see the README limitation "Timelock starts at
finalize".

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `VotingStillActive` from `finalize` | Voting period not over yet | Wait ~2 minutes and retry |
| `VotingPeriodEnded` from `vote` | Voting period already ended | You can no longer vote on this proposal |
| `TimelockNotExpired` from `execute` | Timelock not over yet | Wait ~2 minutes and retry |
| `NotQueued` from `execute` | Proposal was never finalized (still Active) | Run `finalize` first |
| `AlreadyVoted` from `vote` | This address already voted | Use a different voter address |
| `AlreadyInitialized` from `initialize` | Manager already initialized | Skip; the manager is set up |
| Deploy fails with "reference-types not enabled" | Built with `wasm32-unknown-unknown` | Use `wasm32v1-none` (Step 2) |
| `stellar: command not found` | CLI not on PATH | Re-run the install script or open a new shell |

---

## What to read next

- [`docs/walkthrough.md`](walkthrough.md) — the full lifecycle with
  production-sized parameters (`voting_period=17280`, `timelock_delay=17280`,
  quorum 3) and three separate voter accounts. Read this once you are comfortable
  with the CLI and want the production setup.
- [README.md](../README.md) — overview, configuration reference, the full API
  table, error codes, and the security model.
- [`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs)
  — the minimal target contract this tutorial deploys, showing the
  `manager.require_auth()` + `update_current_contract_wasm` pattern your own
  target contracts must follow.
