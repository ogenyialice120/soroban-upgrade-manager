# EMMY_CHANGELOG

This file is the single source of truth for all changes made to this repository
as part of the Stellar Wave Program resubmission audit. Entries are appended in
chronological order. Do not edit or remove prior entries.

---

## 2026-09-20 — docs/readme-and-walkthrough (PR #1)

### Changes

**README.md** — Added "Why Not Just Use Soroban's Native upgrade?" section.
- Side-by-side comparison table: native Soroban self-upgrade vs Soroban Upgrade Manager.
- Explains the single-point-of-failure problem with admin-key upgrades.
- Positions the contract as the Soroban equivalent of Compound Governor Bravo /
  OpenZeppelin TimelockController.
- Added Documentation table linking to the new walkthrough.

**docs/walkthrough.md** — New file (378 lines).
- Full step-by-step walkthrough: setup → upload WASM → deploy manager → deploy
  target → initialize → propose → vote × 3 → finalize → execute.
- All steps use real `stellar contract invoke` commands with named parameters.
- Covers failure modes (TimelockNotExpired, Defeated, NotQueued, AlreadyVoted)
  with resolution guidance.
- Documents what the target contract's `upgrade()` function must look like.
- Includes a timeline diagram and "What the upgrade() function looks like" section.

**Why:** The original README said "most teams handle upgrades manually" without
explaining the security model or why a timelock + vote is better. Reviewers need
to immediately understand the value proposition vs the Soroban native mechanism.

---

## 2026-09-20 — tests/governance-upgrade-integration (PR #2)

### Changes

**contracts/upgrade-manager/src/lib.rs** — Added integration test:
- `test_full_governance_upgrade_flow` — end-to-end test: deploy upgrade-manager,
  deploy a minimal upgradable target contract, propose, vote × 3, advance past
  timelock, finalize, execute. Verifies the target contract's WASM is replaced
  (post-upgrade call succeeds with new behavior).

**Why:** The existing tests covered all governance mechanics (propose, vote,
finalize, cancel) but none of them exercised `execute()` against a real target
contract with a real WASM replacement. The integration test proves the cross-contract
upgrade path actually works end-to-end, which is the core value proposition.
