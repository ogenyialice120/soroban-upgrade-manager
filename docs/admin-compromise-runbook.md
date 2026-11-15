# Admin Key-Compromise Runbook

This runbook documents the incident response procedure if the `admin` key of a
deployed `soroban-upgrade-manager` contract is suspected or confirmed to be
compromised.

---

## 1. Blast Radius: What a Compromised Admin Key Can and Cannot Do

As documented in [`README.md` (Security Model & Limitations)](../README.md#%EF%B8%8F-security-model--limitations),
the `admin` role is a safety valve rather than an unilateral upgrade authority.

### What the attacker CAN do

| Function | Impact | On-Chain Event Emitted |
|---|---|---|
| `cancel(caller, proposal_id)` | Cancel any `Active` or `Queued` proposal (even after it has passed governance voting and is waiting in the timelock queue), causing a denial-of-service against legitimate upgrades. | `("proposal", "cancelled")` with `(proposal_id, caller)` |
| `update_config(caller, ...)` | Set extreme governance parameters (e.g., `voting_period = u32::MAX`, `timelock_delay = u32::MAX`, `quorum = u32::MAX`, or `approval_threshold_pct = 100`) to block future proposals, or lower `voting_period = 1`, `timelock_delay = 1`, `quorum = 1`, `approval_threshold_pct = 1` for new proposals. | `("config", "updated")` with `caller` |
| `transfer_admin(caller, new_admin)` | Transfer the `admin` role to an attacker-controlled address, locking out the original admin. | `("admin", "transfer")` with `(old_admin, new_admin)` |

### What the attacker CANNOT do

- **Cannot unilaterally execute an upgrade without a passing vote:** `execute(proposal_id)` requires the proposal to be in `ProposalStatus::Queued` with `ledger.sequence() >= executable_at` (`assert_timelock_passed`). An admin cannot bypass `propose_upgrade` -> `vote` -> `finalize` -> `timelock_delay`.
- **Cannot alter parameters of in-flight proposals:** Every proposal snapshots `snap_voting_period`, `snap_timelock_delay`, `snap_quorum`, and `snap_threshold_pct` when `propose_upgrade` is called. Calling `update_config` does not affect already-created proposals.
- **Cannot vote or propose on behalf of other addresses:** `propose_upgrade` and `vote` enforce `proposer.require_auth()` and `voter.require_auth()`.
- **Cannot revert or cancel an already-executed upgrade:** Calling `cancel` on a proposal with `ProposalStatus::Executed`, `Defeated`, or `Cancelled` fails with `Error::ProposalFinalized` (code `13`).

> **Caution on lowered config + sybil voting:** Because voting in `soroban-upgrade-manager` is one-address-one-vote (`README.md#limitations`), if an attacker calls `update_config` to set `voting_period = 1`, `timelock_delay = 1`, `quorum = 1`, `approval_threshold_pct = 1`, the attacker can immediately submit a malicious proposal using their own address, vote YES with 1 address, finalize after 1 ledger, and execute 1 ledger later (~10 seconds total). Treat any unauthorized `update_config` or `transfer_admin` event as a **critical severity incident**.

---

## 2. Immediate Actions (First 5–15 Minutes)

### Step 1: Check current on-chain admin and config state

Query the live contract state to see whether the attacker has already rotated the
admin address or altered governance parameters:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --network <testnet|mainnet> \
  -- get_admin

stellar contract invoke \
  --id "$MANAGER_ID" \
  --network <testnet|mainnet> \
  -- get_config
```

### Step 2: Rotate admin immediately if the original admin still has control

If `get_admin` still returns your admin address (for example, after a key leak
before the attacker calls `transfer_admin`, or when threshold signers on a multisig
can still out-sign a compromised signer):

1. Generate or use a pre-provisioned hardware/multisig recovery address (`$NEW_ADMIN`).
2. Invoke `transfer_admin` immediately:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source "$OLD_ADMIN_SECRET" \
  --network <testnet|mainnet> \
  -- transfer_admin \
  --caller "$OLD_ADMIN_ADDRESS" \
  --new_admin "$NEW_ADMIN_ADDRESS"
```

3. If the attacker modified `Config`, restore safe governance parameters using `$NEW_ADMIN`:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source "$NEW_ADMIN_SECRET" \
  --network <testnet|mainnet> \
  -- update_config \
  --caller "$NEW_ADMIN_ADDRESS" \
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

4. Inspect all recent proposals (`get_proposal --proposal_id <N>`) and call `cancel`
   on any suspicious `Active` or `Queued` proposals created during the exposure window:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source "$NEW_ADMIN_SECRET" \
  --network <testnet|mainnet> \
  -- cancel \
  --caller "$NEW_ADMIN_ADDRESS" \
  --proposal_id <SUSPICIOUS_PROPOSAL_ID>
```

---

## 3. Recovery Steps (If the Attacker Already Called `transfer_admin`)

If `get_admin` returns an attacker-controlled address, the compromised `upgrade-manager`
instance can no longer be trusted to manage target contracts. Follow these steps:

### Step 1: Deploy and initialize a fresh `upgrade-manager` contract

1. Deploy a new `soroban-upgrade-manager` instance using a clean deployer account:

```bash
NEW_MANAGER_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/soroban_upgrade_manager.wasm \
  --source "$RECOVERY_DEPLOYER" \
  --network <testnet|mainnet>)
```

2. Immediately initialize the new manager with a multisig admin address in the same
   operational window so `initialize` cannot be front-run:

```bash
stellar contract invoke \
  --id "$NEW_MANAGER_ID" \
  --source "$MULTISIG_ADMIN" \
  --network <testnet|mainnet> \
  -- initialize \
  --admin "$MULTISIG_ADMIN_ADDRESS" \
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

### Step 2: Migrate or protect the target contract

Depending on how your target contract stores its manager reference:

- **Case A — Target contract supports rotating its manager address (or has a separate emergency guardian):**
  Point the target contract's stored `manager` address to `$NEW_MANAGER_ID` so the old compromised manager can no longer pass `manager.require_auth()` inside `target.upgrade()`.
- **Case B — Target contract only upgrades via the old manager (like `example-target`):**
  - If an honest proposal is already `Queued` on the old manager and the attacker has not cancelled it, execute a migration WASM upgrade as soon as `time_until_executable` reaches `0` that updates `DataKey::Manager` in the target contract to `$NEW_MANAGER_ID`.
  - If the attacker has locked the old manager by cancelling all proposals or setting hostile config parameters, deploy a new target contract instance wired to `$NEW_MANAGER_ID`, migrate state/users, and publish a deprecation notice for the old contract IDs.

### Step 3: Re-propose pending legitimate upgrades

Re-submit any legitimate proposals that were cancelled during the incident to
`$NEW_MANAGER_ID` using `propose_upgrade` and notify voters of the new manager
contract address.

---

## 4. Prevention & Operational Hardening

1. **Use a Stellar Multisig or DAO for `admin`:**
   Never use a single hot key for `admin` in production. Use a Stellar multi-signature
   account (e.g., 2-of-3 or 3-of-5 hardware-backed signers) or a governance DAO contract.
2. **Monitor Contract Events in Real Time:**
   Subscribe to Soroban contract events on `$MANAGER_ID` and alert immediately on:
   - `("admin", "transfer")` — Admin ownership change (`transfer_admin`).
   - `("config", "updated")` — Governance parameter change (`update_config`).
   - `("proposal", "cancelled")` — Proposal cancellation (`cancel`).
   - `("proposal", "created")` — Unexpected upgrade proposals (`propose_upgrade`).
3. **Verify `get_config` and `get_admin` Before Voting:**
   Voters should verify that `get_admin` matches the expected multisig address and
   that the proposal's snapshotted parameters (`snap_voting_period`, `snap_timelock_delay`,
   `snap_quorum`, `snap_threshold_pct` via `get_proposal`) match expected security baselines.
