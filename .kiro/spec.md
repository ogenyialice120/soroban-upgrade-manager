# Soroban Upgrade Manager — Hardening Spec

## Overview

This spec governs four phases of hardening work on `soroban-upgrade-manager`.
The product idea is fixed: a governance contract that upgrades other contracts
via `execute()` calling the target's `upgrade(new_wasm_hash)`.

## Requirements

### Correctness
- R1: Timelock enforced BETWEEN approval and execution (not between creation and finalize)
- R2: Voting period ends before finalize is allowed; finalize sets executable_at
- R3: Proposal rules snapshotted at creation; config changes don't affect in-flight proposals
- R4: All state-changing functions require proper auth
- R5: initialize requires admin.require_auth()
- R6: cancel only allows Active or Queued proposals
- R7: Non-empty description required on propose_upgrade
- R8: execute sets status to Executed BEFORE invoking target (checks-effects-interactions)
- R9: unwrap() replaced with proper error handling (NotInitialized)
- R10: Checked arithmetic throughout (Overflow error)
- R11: TTL extended on all state-changing calls
- R12: Events emitted for all state changes

### Tests
- T1: Real v1→v2 WASM upgrade via example-target contract
- T2: Failed upgrade leaves proposal Queued
- T3: TTL test for proposal and vote entries
- T4: Auth tests without mock_all_auths
- T5: Boundary ledger tests (voting_ends_at exactly)
- T6: Config snapshot isolation test
- T7: Events tests

### Documentation
- D1: Walkthrough uses example-target (not second upgrade-manager)
- D2: README lifecycle diagram matches actual code
- D3: Voting described as "any address, one vote" not "token holders"
- D4: Security model & limitations section
- D5: Error code reference table
- D6: Claims audit matrix
- D7: CONTRIBUTING.md clone URL and issue template references fixed

### CI
- C1: fmt --check and clippy -D warnings in CI
- C2: claims check script
- C3: fixture drift detection
- C4: rust-toolchain.toml pinned
- C5: Release workflow for WASM artifacts
- C6: pre-commit config

## Design

### Phase 1 Changes

**types.rs additions:**
- `voting_period: u32` field on `Config`
- `voting_ends_at: u32` field on `Proposal`
- Snapshot fields on `Proposal`: `voting_period`, `timelock_delay`, `quorum`, `approval_threshold_pct`
- New errors: `VotingPeriodEnded`, `VotingStillActive`, `NotInitialized`, `Overflow`, `EmptyDescription`
- Fix `quorum` doc comment (counts total votes, not YES)

**governance.rs changes:**
- `load_config` returns `Result<Config, Error>` (NotInitialized)
- `load_admin` returns `Result<Address, Error>` (NotInitialized)
- `create_proposal`: checks non-empty description, sets `voting_ends_at`, snapshots config
- `cast_vote`: checks `ledger.sequence() <= voting_ends_at`
- `finalize_voting`: checks `ledger.sequence() > voting_ends_at`, sets `executable_at = finalize_ledger + timelock_delay` using snapshotted values
- `require_admin(env, caller)` helper
- `validate_config(...)` shared helper
- TTL extension on all writes
- Events on all state changes

**timelock.rs changes:**
- `ledgers_until_executable`: return 0 if not Queued

**lib.rs changes:**
- `initialize`: add `voting_period` param, call `admin.require_auth()`
- Use `require_admin` helper everywhere
- `execute`: set Executed status BEFORE invoke_contract
- `get_config` and `get_admin` propagate NotInitialized

### Phase 2: Tests & Fixtures

- `contracts/example-target/` — minimal upgradable contract with version() and upgrade()
- `scripts/build_fixtures.sh` — builds v1 and v2 WASM
- `contracts/upgrade-manager/tests/fixtures/` — committed WASM files
- `contracts/upgrade-manager/src/test.rs` — all tests moved here

### Phase 3: Docs

- `docs/walkthrough.md` — rewritten for example-target
- `README.md` — corrected claims, security model section
- `docs/claims-audit.md` — claims matrix
- `CONTRIBUTING.md` — clone URL fixed
- `SECURITY.md` — new file

### Phase 4: CI

- `.github/workflows/ci.yml` — fmt, clippy, fixture drift, claims check
- `scripts/check_claims.sh` — claims matrix validation
- `rust-toolchain.toml` — pinned toolchain
- `.pre-commit-config.yaml`

## Tasks (in order)

### Phase 1
1.1 Fix timelock placement (voting_period, voting_ends_at, finalize sets executable_at)
1.2 Snapshot rules into proposal struct
1.3 Auth fixes (initialize requires auth, require_admin helper, cancel restriction, EmptyDescription)
1.4 Robustness (NotInitialized, Overflow, validate_config, TTL, events, doc fixes)
1.5 Update existing tests to compile and pass with new signatures

### Phase 2
2.1 Move tests to src/test.rs
2.2 Add example-target contract
2.3 Build fixtures script and commit fixtures
2.4 Add real WASM upgrade integration test
2.5 Add failed-upgrade test
2.6 Add TTL test
2.7 Add auth tests without mock_all_auths
2.8 Add boundary/edge case tests

### Phase 3
3.1 Rewrite docs/walkthrough.md
3.2 Update README.md
3.3 Fix CONTRIBUTING.md
3.4 Add SECURITY.md
3.5 Build claims audit matrix

### Phase 4
4.1 Update CI workflow
4.2 Add check_claims.sh script
4.3 Add rust-toolchain.toml
4.4 Add fixture drift CI step
4.5 Add release workflow
4.6 Add .gitignore and .pre-commit-config.yaml
