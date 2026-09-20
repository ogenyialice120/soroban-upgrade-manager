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

**contracts/upgrade-manager/src/lib.rs** — Added integration tests:
- `UpgradableTarget` — minimal inline test contract exposing `version()` and
  `upgrade(new_wasm_hash)`, the standard Soroban upgrade interface.
- `test_full_governance_upgrade_flow` — end-to-end: deploy manager + target,
  initialize, propose, vote × 3, advance past timelock, finalize → Queued,
  execute → Executed. Verifies the cross-contract invoke_contract("upgrade")
  path works correctly.
- `test_execute_fails_on_active_proposal` — execute before finalize returns NotQueued.
- `test_execute_fails_on_defeated_proposal` — execute on a Defeated proposal
  returns NotQueued.
- `test_execute_fails_before_timelock` — attempts execute on a proposal that
  hasn't been finalized yet returns NotQueued.

**Why:** The existing tests covered all governance mechanics (propose, vote,
finalize, cancel) but none exercised execute() against a real target contract.
The integration test proves the cross-contract upgrade path works end-to-end.
