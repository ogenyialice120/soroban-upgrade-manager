# Limitations Explained

The README's
[Security Model & Limitations](../README.md#-security-model--limitations)
section lists eight limitations in brief. This page explains each one in depth:
why it exists, what the concrete risk is, what a deployer can do about it today,
and whether it is planned to be addressed (cross-referencing the README's
[Possible Future Work](../README.md#-possible-future-work)).

The tone is deliberately factual. None of these are defects to be papered over,
and none are emergencies to be panicked over — each is a consequence of a
deliberate design choice that keeps the contract small and dependency-free.

The contract is **unaudited**. Treat this page as context for your own risk
assessment, not as a security endorsement.

---

## Summary

| # | Limitation | Planned mitigation in README "Possible Future Work"? |
|---|---|---|
| 1 | Not sybil-resistant | Yes — sybil resistance |
| 2 | Powerful admin key | No (mitigate off-chain with a multisig/DAO) |
| 3 | Admin can cancel after approval | No (an unimplemented design exists; see below) |
| 4 | No proposal deposit | Yes — proposal deposits |
| 5 | Timelock starts at finalize | No (deliberate design choice; mitigate with monitoring) |
| 6 | Target must authenticate the manager | No (it is the target contract's responsibility) |
| 7 | No expiry | Yes — proposal expiry |
| 8 | No WASM compatibility check | Yes — WASM compatibility check |

---

## 1. Not sybil-resistant

**Why it exists.** Voting is one-address-one-vote. There is no stake weight, no
token balance, and no identity requirement behind a vote, so the contract stays
free of token or identity dependencies. This keeps the manager usable for any
target contract without forcing the deployer to also deploy a governance token.

**Risk.** An attacker who creates many Stellar accounts can cast many votes and
sway the outcome of a proposal. With a low `quorum` and low
`approval_threshold_pct`, a single attacker controlling enough accounts can pass
a malicious upgrade, or outvote a legitimate one.

**Off-chain mitigations.**
- Set a meaningfully high `quorum` and `approval_threshold_pct` so an attacker
  must control many accounts, not just one.
- Restrict who is allowed to propose and vote using an off-chain allow-list or
  a known voter set, and only call `vote` from those addresses.
- Use this manager as a layer beneath a sybil-resistant identity or token-weight
  system that gates who may interact with it.
- Hold the admin role in a multisig or DAO so a sybil attack that passes a bad
  proposal can still be cancelled (see limitation 2).

**Planned?** Yes. The README's Possible Future Work lists **sybil resistance**:
integrating with a Stellar identity or token-weighting system to prevent
multi-address voting attacks.

---

## 2. Powerful admin key

**Why it exists.** A single admin role owns the safety valves: cancelling
Active or Queued proposals, changing governance parameters, and transferring
the admin role. Concentrating these in one role keeps the contract simple and
avoids a second governance layer for administrative actions. The admin
**cannot** execute upgrades without a passing vote — `execute` requires the
proposal to be `Queued`, which requires quorum and threshold to have been met.

**Risk.** If the admin key is compromised, the attacker can cancel legitimate
proposals, change `quorum`/`threshold`/timelock to favorable values, and
transfer admin ownership to themselves. They cannot silently swap the target's
WASM, but they can block any upgrade they dislike and manipulate the rules for
future proposals.

**Off-chain mitigations.**
- Hold the admin role in a **multisig** or a **DAO**, so no single key can
  exercise these powers alone.
- Use a hardware-backed or rotation-enforced key for the admin.
- Monitor admin actions on-chain (the contract emits `config_updated`,
  `admin_transferred`, and `proposal_cancelled` events) and alert on
  unexpected admin activity.
- Keep `timelock_delay` long enough that a malicious config change or cancel
  can be noticed and responded to before an affected proposal executes.

**Planned?** Not in the README's Possible Future Work. The intended mitigation
is operational (multisig/DAO admin), not a contract change.

---

## 3. Admin can cancel after approval

**Why it exists.** The admin can cancel a `Queued` proposal — one that already
passed the vote — during the timelock delay. This is a deliberate safety valve:
it lets the admin stop an upgrade that turns out to be dangerous after voting
closed but before it executes.

**Risk.** The same power means the admin can unilaterally block an upgrade that
the voters approved. A compromised or malicious admin can hold the contract in
a state where approved upgrades never execute. Voters have no on-chain
recourse against an admin that cancels every Queued proposal.

**Off-chain mitigations.**
- Use a multisig or DAO as admin so a single party cannot cancel unilaterally.
- Publish and follow a clear cancellation policy so cancellations are
  auditable and explainable to voters.
- Monitor `proposal_cancelled` events and alert when a Queued proposal is
  cancelled.
- Keep the admin role with a trusted, accountable entity, and document who can
  exercise the veto.

**Planned?** Not listed in the README's Possible Future Work. An optional,
**unimplemented** design for bounding this power — a per-deployment `veto`
window that closes the admin's ability to cancel a Queued proposal once the
timelock elapses — is sketched in `DESIGN-veto-window.md`. It is a design
document only; no such code is in the contract today.

---

## 4. No proposal deposit

**Why it exists.** Creating a proposal is free. Requiring a deposit would mean
introducing a token dependency (something to deposit and to forfeit), which the
contract deliberately avoids to stay usable by any target contract. A
per-address `proposal_min_interval` rate limit is the only anti-spam mechanism
on-chain.

**Risk.** Spam is possible even with the rate limit. An attacker who creates
many addresses can still flood the contract with proposals — one per address per
`proposal_min_interval` ledgers each. A flood of proposals clutters the audit
trail and can obscure a genuine proposal.

**Off-chain mitigations.**
- Set `proposal_min_interval` high enough to limit throughput from a single
  address (at ~5 s/ledger, `720` ≈ 1 hour between proposals per address).
- Use an off-chain allow-list of authorized proposers.
- Index and filter proposals off-chain so a flood does not drown out the
  genuine ones.
- Monitor proposal creation volume and alert on abnormal spikes.

**Planned?** Yes. The README's Possible Future Work lists **proposal deposits**:
requiring proposers to lock a deposit that is forfeited if the proposal fails,
reducing spam.

---

## 5. Timelock starts at finalize

**Why it exists.** The mandatory `timelock_delay` is measured from the moment
someone calls `finalize()`, not from proposal creation. `finalize()` cannot be
called until the voting period ends, so the design separates two windows —
voting, then a delay before execution — and lets the delay begin only once the
result is known. This matches the README's two-window lifecycle and the audit
trail in `docs/claims-audit.md`.

**Risk.** If nobody calls `finalize()`, a proposal that passed the vote stays
`Active` indefinitely and the timelock never starts. An approved upgrade can be
left dormant, and the timelock's protection (time to review and exit) only
begins when a finalize eventually happens — which could be much later than the
vote.

**Off-chain mitigations.**
- Run a bot or watcher that calls `finalize()` automatically once the voting
  period ends, so the timelock starts predictably.
- Monitor for proposals past their `voting_ends_at` that are still `Active`,
  and alert.
- Document who is responsible for finalizing in your deployment.

**Planned?** Not in the README's Possible Future Work. Starting the timelock at
finalize is a deliberate design choice, not a defect to be removed.

---

## 6. Target must authenticate the manager

**Why it exists.** The manager triggers an upgrade by calling the target's
`upgrade(new_wasm_hash)` function via cross-contract invocation. Soroban
cross-contract calls do not automatically authenticate the *caller contract*
to the callee — the target must check for itself that the call came from the
manager it trusts. This is the target contract's responsibility, not the
manager's, because the manager cannot know each target's desired policy.

**Risk.** If a target's `upgrade()` does **not** call `manager.require_auth()`
before `update_current_contract_wasm()`, then anyone who can reach the target's
`upgrade()` can bypass governance entirely and swap the target's WASM directly.
The manager's vote, timelock, and quorum protections become irrelevant for that
target.

**Off-chain mitigations.**
- Audit the target contract before deploying it: confirm `upgrade()` loads the
  stored manager and calls `manager.require_auth()` before
  `update_current_contract_wasm()`.
- Use the reference implementation in
  [`contracts/example-target/src/lib.rs`](../contracts/example-target/src/lib.rs)
  as the correct pattern, and `contracts/bad-target/` as the anti-example to
  avoid.
- Store the manager address at deploy time in the target's constructor (as
  `example-target` does), so there is no separate initializer that could be
  front-run.

**Planned?** Not in the README's Possible Future Work. This is a property of
the target contract, not the manager, so it cannot be fixed inside the manager.

---

## 7. No expiry

**Why it exists.** Proposals do not expire. Once a proposal is `Active`, it can
be finalized at any time after the voting period ends — the contract applies no
time limit on when `finalize()` may be called. This avoids a forced-expiry
mechanism and its storage/parameter costs.

**Risk.** An `Active` proposal that received its votes can sit dormant and be
finalized weeks or years later, long after the context that made it relevant has
changed. Stale votes can be "revived" to execute an upgrade the current
community would not approve of, and the long gap makes the original vote
unrepresentative.

**Off-chain mitigations.**
- Track each proposal's `voting_ends_at` and finalize or cancel proposals
  promptly rather than leaving them dormant.
- Use the admin cancel (limitation 3) to retire stale `Active` proposals that
  should not be revived.
- Monitor for old `Active` proposals and alert if one is finalized long after
  its voting period.

**Planned?** Yes. The README's Possible Future Work lists **proposal expiry**:
automatically expiring `Active` proposals that have not been finalized within a
configurable window.

---

## 8. No WASM compatibility check

**Why it exists.** The contract does not verify that the new WASM is
storage-layout-compatible with the existing target. On-chain verification of
arbitrary WASM storage layouts is infeasible within a small contract, so the
contract trusts the proposer and voters to verify compatibility off-chain.

**Risk.** An incompatible upgrade can brick the target contract: if the new
WASM reads or writes storage keys that no longer match the old layout (renamed,
retyped, or removed keys), the target panics on the first access to old data
and becomes unusable. The governance vote and timelock do not protect against
this — they protect against unauthorized upgrades, not against broken ones.

**Off-chain mitigations.**
- Run an upgrade-compatibility check before proposing, using a tool such as
  [soroban-upgrade-safeguard](https://github.com/eigercoin/soroban-upgrade-safeguard)
  or an equivalent review, and link the report in the proposal description.
- Stage the upgrade on a testnet copy of the target first.
- If a storage key must be renamed or restructured, migrate it in the **old**
  contract's `upgrade()` before the WASM swap (see
  [`contracts/migration-target/`](../contracts/migration-target/) for a worked
  v1→v2 example).
- Include the WASM hash and the compatibility report in the proposal
  `description` so voters can independently verify before voting.

**Planned?** Yes. The README's Possible Future Work lists a **WASM compatibility
check**: an on-chain or off-chain verification step that checks storage-layout
compatibility between the old and new WASM.

---

## See Also

- [README — Security Model & Limitations](../README.md#-security-model--limitations) — the brief version this page expands on.
- [README — Possible Future Work](../README.md#-possible-future-work) — planned mitigations cross-referenced above.
- [README — Recommended Workflow](../README.md#-recommended-workflow) — the off-chain compatibility-check workflow for limitation 8.
- [`docs/walkthrough.md`](walkthrough.md) — the full governance lifecycle, including the storage-layout note for limitation 8.
- [`docs/claims-audit.md`](claims-audit.md) — each limitation claim mapped to the function and test that verifies it.
- [SECURITY.md](../SECURITY.md) — how to report a vulnerability privately.
