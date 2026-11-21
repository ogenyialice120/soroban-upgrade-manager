# Frequently Asked Questions

Answers to common questions about `soroban-upgrade-manager`. Where relevant,
each answer cites the source code or existing documentation.

---

## Governance model

### Q1: Can I use token-weighted voting instead of one-address-one-vote?

No, not with this contract as deployed. The voting model is strictly
one-address-one-vote: each address that calls `vote()` contributes exactly 1
vote (or more if other addresses have delegated to it). There is no concept of
token balance or stake.

If you need token-weighted voting, you would need to replace the `cast_vote`
function in `governance.rs` with one that reads a token balance from an
external contract and uses it as the vote weight. This is listed as a possible
future enhancement in the [README](../README.md#-possible-future-work) but is
not implemented.

---

### Q2: Is the voting sybil-resistant?

No. Any address can vote, and creating additional addresses is free. An
attacker who controls multiple addresses can reach any quorum value and cast as
many votes as they want. The contract has no way to distinguish a genuine
participant from an attacker operating fifty addresses from the same key pair.

This is documented in the [Security Model & Limitations](../README.md#%EF%B8%8F-security-model--limitations)
section of the README. For production high-value contracts, combine this
contract with an external sybil-resistant identity system, or use
token-weighted voting where economic stake makes multi-address attacks expensive.

**Impact on parameter choice:** Quorum values are most useful as a signal of
genuine community engagement in honest deployments, not as a hard security
gate. The timelock delay is a more reliable protection because it gives everyone
time to notice and respond to a suspicious proposal regardless of how votes
were cast. See [docs/choosing-parameters.md](choosing-parameters.md) for more
detail.

---

### Q3: What happens if the admin key is compromised?

An attacker with the admin key can:

- Cancel any Active or Queued proposal (including ones that have already passed
  the vote and are waiting in the timelock).
- Call `update_config` to change governance parameters (affects future proposals
  only, not in-flight ones).
- Call `transfer_admin` to hand the admin role to another address.

An attacker with the admin key **cannot**:

- Execute an upgrade without a passing vote. `execute()` requires the proposal
  to be in Queued status, which requires a successful `finalize()`, which
  requires the voting period to have ended and the quorum/threshold to have been
  met.
- Vote on proposals. Voting requires a separate `voter.require_auth()` call.

**Recommended mitigation:** Transfer the admin role to a multisig wallet
(multiple signers required) so that no single key compromise gives full admin
control. See the README for the `transfer_admin` API.

---

### Q4: How do I recover from a bad upgrade?

The contract itself does not provide a rollback mechanism. Once `execute()` has
been called and the target contract's WASM is replaced, the upgrade is final at
the contract level.

Your options depend on the target contract's design:

1. **Propose a new upgrade:** Create a new proposal with the previous WASM hash
   (which must still be accessible on-chain, since uploaded WASMs are stored by
   hash). Vote it through, finalize, and execute to restore the old code.
2. **Redeploy the target:** If the bad upgrade broke the target contract in a
   way that makes it uncallable, you may need to deploy a fresh instance and
   redirect users.

**Prevention is better than recovery:** Follow the [Recommended Workflow](../README.md#-recommended-workflow)
in the README: run a storage-layout compatibility check before proposing any
upgrade, and include the WASM hash and a compatibility report link in the
proposal description so voters can independently verify.

---

### Q5: What if no one calls `finalize()` after the voting period ends?

The proposal stays in Active status indefinitely. There is no automatic
expiration. `finalize()` is a permissionless call — anyone can call it after
`ledger.sequence() > voting_ends_at`. The proposal only moves to Queued or
Defeated when someone explicitly calls `finalize()`.

This is documented in the README as a known limitation: "Proposals do not
expire. An Active proposal can be finalized at any time after the voting period
ends, even years later."

If you want to prevent stale proposals from being finalized unexpectedly, the
admin can cancel Active proposals using `cancel()`.

---

## Proposal lifecycle

### Q6: When does the timelock start?

The timelock starts when `finalize()` is called, **not** when the proposal is
created or when the voting period ends. The `executable_at` field is set to
`finalize_ledger + snap_timelock_delay` inside `finalize_voting()` in
`governance.rs`.

Before `finalize()` is called, `executable_at` is 0. `time_until_executable`
returns 0 for proposals that are not in Queued status.

**Practical implication:** If the voting period ends and no one calls
`finalize()` for a week, the timelock clock has not yet started. The timelock
delay begins only after finalization.

See the [walkthrough](walkthrough.md) Step 7 and Step 8 for the full sequence.

---

### Q7: Can a proposal be voted on after the voting period ends?

No. `vote()` checks that `ledger.sequence() <= voting_ends_at`. A vote cast
after the voting period ends returns `VotingPeriodEnded` (error code 16). The
voting boundary is inclusive: a vote cast at exactly `voting_ends_at` succeeds;
a vote cast at `voting_ends_at + 1` or later fails.

This is tested by `test_vote_at_voting_ends_at_succeeds` and
`test_vote_after_voting_period_ends_fails` in `src/test.rs`.

---

### Q8: Can `finalize()` be called at the exact ledger where voting ends?

No. `finalize()` requires `ledger.sequence() > voting_ends_at` (strict
greater-than). Calling `finalize()` at `voting_ends_at` returns
`VotingStillActive` (error code 17). The first ledger where `finalize()` is
valid is `voting_ends_at + 1`.

This is tested by `test_finalize_at_voting_ends_at_fails` and
`test_finalize_at_voting_ends_at_plus_one_succeeds` in `src/test.rs`.

---

### Q9: What happens to config changes made mid-proposal?

Config changes made via `update_config` do not affect proposals that are
already in flight. Each proposal snapshots the governance rules at creation
time into the fields `snap_voting_period`, `snap_timelock_delay`, `snap_quorum`,
and `snap_threshold_pct`. These snapshot values are used for all subsequent
checks (vote boundary, finalization pass/fail, timelock duration) regardless
of what `update_config` does later.

This is tested by `test_config_snapshot_isolation` in `src/test.rs` and is
documented in the Phase 1 changelog entry.

---

### Q10: When exactly can `execute()` be called?

`execute()` requires:

1. The proposal is in Queued status (set by `finalize()`).
2. `ledger.sequence() >= executable_at` (the timelock has expired).

`executable_at` is set by `finalize()` to `finalize_ledger + snap_timelock_delay`.
Calling `execute()` one ledger before `executable_at` returns
`TimelockNotExpired` (error code 7).

`execute()` is permissionless — anyone can call it once these conditions are
met, not just the proposer or admin.

---

### Q11: Can the admin cancel a proposal that has already been approved by voters?

Yes. The admin can cancel any Active or Queued proposal, including ones that
have passed the vote and are waiting in the timelock delay. This is a
deliberate safety valve documented in the README's Security Model section:

> "The admin can cancel a Queued proposal even after it has passed the vote,
> during the timelock delay. This is a deliberate safety valve, but it means
> the admin can unilaterally block an approved upgrade."

To reduce this risk, transfer the admin role to a multisig that requires
multiple keyholders to agree before cancelling.

---

### Q12: Can anyone propose a new proposal at any time, causing spam?

Yes. There is no proposal deposit or restriction on who can create a proposal.
Any authenticated address can call `propose_upgrade()`. An attacker can flood
the contract with proposals to increase gas costs for legitimate users reviewing
the proposal list.

This is documented in the README as a known limitation. It is listed as a
possible future mitigation (proposal deposits). For now, the admin can cancel
Active proposals to remove spam, and the `get_proposals()` function supports
pagination to bound the gas cost of listing proposals.

---

## Security and authentication

### Q13: Does the target contract need to do anything special to work with the upgrade manager?

Yes. The target contract's `upgrade()` function **must** authenticate that the
call comes from the upgrade manager before calling
`env.deployer().update_current_contract_wasm()`. If it does not, anyone who can
call `execute()` could bypass governance by calling the target's `upgrade()`
function directly.

The reference implementation in `contracts/example-target/src/lib.rs`:

```rust
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
    let manager: Address = env
        .storage()
        .instance()
        .get(&symbol_short!("manager"))
        .expect("manager not set");
    manager.require_auth();
    env.deployer().update_current_contract_wasm(new_wasm_hash);
}
```

The manager address is stored at deploy time in the target's constructor and
is never changeable without its own upgrade. The upgrade manager's `execute()`
call satisfies `manager.require_auth()` via the cross-contract invocation
authorization chain.

---

### Q14: How do I verify the WASM hash in a proposal before voting?

Before voting, compare the `new_wasm_hash` in the proposal against the hash
you independently compute from the source code:

```bash
# Build the WASM from source
cargo build --target wasm32v1-none --release \
  --manifest-path contracts/example-target/Cargo.toml

# Compute the SHA-256 of the compiled WASM
sha256sum target/wasm32v1-none/release/example_target.wasm
```

Compare the output against `new_wasm_hash` from `get_proposal()`. If the
hashes match, you are voting for exactly the WASM binary you built from source.

Also check the hash against what was uploaded to the network:

```bash
stellar contract upload \
  --wasm target/wasm32v1-none/release/example_target.wasm \
  --source alice \
  --network testnet
# Returns the same hash if the binary was already uploaded
```

The walkthrough in `docs/walkthrough.md` includes this step.

---

### Q15: Can I use a multisig as the admin?

Yes. After deployment, call `transfer_admin` to hand the admin role to a
multisig contract address:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source current_admin \
  --network testnet \
  -- transfer_admin \
  --caller "$CURRENT_ADMIN" \
  --new_admin "$MULTISIG_ADDR"
```

Once transferred, all admin-gated functions (`cancel`, `update_config`,
`transfer_admin`) require the multisig to authorize the transaction, meaning
multiple keyholders must agree. This is the recommended configuration for
production deployments.

---

## Storage and upgrades

### Q16: What happens if the new WASM is storage-layout-incompatible with the existing contract?

The upgrade manager does not check storage compatibility. If the new WASM
reads storage keys under different types than those stored by the old version,
the target contract will panic the first time it tries to read those keys after
the upgrade.

This is documented in the README:

> "The contract does not verify that the new WASM is storage-layout-compatible
> with the existing target. An incompatible upgrade can brick the target
> contract."

Always run a storage-layout compatibility check before proposing an upgrade.
The README's [Recommended Workflow](../README.md#-recommended-workflow) section
describes this step. Including a compatibility report link in the proposal
description allows voters to independently verify compatibility before voting.

---

### Q17: How do I deploy on testnet?

The walkthrough in [docs/walkthrough.md](walkthrough.md) covers the full
testnet deployment sequence, including:

- Building both WASM versions.
- Deploying the upgrade manager and example-target.
- Initializing the manager with governance parameters.
- Creating a proposal, voting, finalizing, and executing.

For quick reference, the testnet deployment from 2026-10-30 is documented in
the README's [Verified on Stellar testnet](../README.md#verified-on-stellar-testnet)
section with transaction hashes.

---

## See also

- [README.md — Security Model & Limitations](../README.md#%EF%B8%8F-security-model--limitations)
- [docs/walkthrough.md — Failure Modes](walkthrough.md#failure-modes)
- [docs/choosing-parameters.md](choosing-parameters.md)
- [contracts/upgrade-manager/src/governance.rs](../contracts/upgrade-manager/src/governance.rs) — voting and finalization logic
- [contracts/example-target/src/lib.rs](../contracts/example-target/src/lib.rs) — reference target implementation
