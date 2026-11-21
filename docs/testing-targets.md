# Testing Target Contracts with the Upgrade Manager

A guide for target contract developers who want to write integration tests
against `soroban-upgrade-manager`. It covers writing a test helper contract,
running the full governance flow in tests, and understanding the test
environment's limitations.

All referenced test functions and helper contracts exist in
`contracts/upgrade-manager/src/test.rs`.

---

## 1. Overview

The upgrade manager governs WASM upgrades to a **target** contract through this
lifecycle:

```
initialize() → propose_upgrade() → vote() → finalize() → execute()
```

To test your target contract with the manager, you:

1. Write a test helper contract that mirrors your real target
   (including `upgrade()` that authenticates the manager).
2. Deploy both the manager and the helper target in a `soroban_sdk::testutils`
   environment.
3. Run the full governance flow: initialize, propose, vote, advance past the
   voting period, finalize, advance past the timelock, execute.
4. Assert the proposal transitions to `Executed` and the target behaves as
   expected.

---

## 2. Writing a test helper contract

Each helper contract lives in its own Rust sub-module inside `test.rs` because
`#[contractimpl]` emits module-level symbols (e.g. `__upgrade`, `__version`)
that would collide if all contracts were in the same scope.

### 2.1 Minimal mock target (no auth check)

Use this for simple flow tests where you don't need to verify manager auth:

```rust
/// A minimal mock target with no auth check on upgrade().
#[cfg(test)]
mod mock_target_contract {
    use soroban_sdk::{contract, contractimpl, BytesN, Env};

    #[contract]
    pub struct MockTarget;

    #[contractimpl]
    impl MockTarget {
        pub fn version(_env: Env) -> u32 {
            1
        }
        pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
            env.deployer().update_current_contract_wasm(new_wasm_hash);
        }
    }
}
```

### 2.2 Fixture target with manager auth (recommended)

Use this to mirror a real target whose `upgrade()` authenticates the manager
via `manager.require_auth()` before calling `update_current_contract_wasm()`:

```rust
/// Target that mirrors example-target's manager-auth upgrade() pattern.
#[cfg(test)]
mod fixture_target_contract {
    use soroban_sdk::{
        contract, contractimpl, symbol_short, Address, BytesN, Env,
    };

    #[contract]
    pub struct FixtureTarget;

    #[contractimpl]
    impl FixtureTarget {
        pub fn __constructor(env: Env, manager: Address) {
            // No require_auth here — mock_all_auths doesn't cover
            // register_at constructors.
            env.storage()
                .instance()
                .set(&symbol_short!("manager"), &manager);
        }
        pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
            let manager: Address = env
                .storage()
                .instance()
                .get(&symbol_short!("manager"))
                .expect("manager not set");
            manager.require_auth();
            env.deployer().update_current_contract_wasm(new_wasm_hash);
        }
        pub fn version(_env: Env) -> u32 {
            1
        }
        pub fn get_manager(env: Env) -> Address {
            env.storage()
                .instance()
                .get(&symbol_short!("manager"))
                .expect("manager not set")
        }
    }
}
```

### 2.3 Target with no upgrade() function

Use this to test the failure path when the target does not implement
`upgrade()`:

```rust
/// A contract that deliberately has NO upgrade() function.
#[cfg(test)]
mod no_upgrade_contract {
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct NoUpgradeFn;

    #[contractimpl]
    impl NoUpgradeFn {
        pub fn version(_env: Env) -> u32 {
            42
        }
    }
}

### Key points for helper contracts

| Consideration | Guidance |
|---------------|----------|
| **Constructor auth** | The real `example-target` requires admin auth in its constructor. Test proxies omit `manager.require_auth()` in `__constructor` because `mock_all_auths` does not cover `register_at` constructors. |
| **`upgrade()` auth** | Your real target's `upgrade()` **must** call `manager.require_auth()` before `update_current_contract_wasm()`. Otherwise anyone who can call `execute()` can bypass governance. |
| **Separate modules** | Put each helper in its own `#[cfg(test)] mod` to avoid `#[contractimpl]` symbol collisions. |
| **Client types** | Soroban SDK auto-generates `FixtureTargetClient`, `MockTargetClient`, etc. from each `#[contract]` type. |

---

## 3. Full governance flow in tests

The standard test setup in `test.rs` is:

```rust
fn setup() -> (Env, Address, Address, UpgradeManagerClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(UpgradeManager, ());
    let client = UpgradeManagerClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
    (env, admin, cid, client)
}
```

Config used: `voting_period=100`, `timelock_delay=17_280`, `quorum=3`,
`approval_threshold_pct=51`.

### 3.1 Step-by-step flow

Here is the complete flow, using `FixtureTarget`:

```rust
let env = Env::default();
env.mock_all_auths();

// 1. Deploy the manager
let manager_id = env.register(UpgradeManager, ());
let manager = UpgradeManagerClient::new(&env, &manager_id);

// 2. Deploy the target with the manager address as constructor arg
let target_id = env.register_at(
    &Address::generate(&env),
    FixtureTarget,
    (manager_id.clone(),),
);
let target = FixtureTargetClient::new(&env, &target_id);

// 3. Initialize the manager
let admin = Address::generate(&env);
manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);

// 4. Upload the new WASM hash (empty bytes as placeholder — see §4)
let new_hash = env
    .deployer()
    .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

// 5. Propose the upgrade
let pid = manager.propose_upgrade(
    &admin,
    &target_id,
    &new_hash,
    &title(&env, "v2 upgrade"),
    &desc(&env, "Upgrade to v2"),
    &zero_hash(&env),
);

// 6. Vote (quorum=3, threshold=51 → need ≥3 YES votes of ≥3 total = 100%)
for _ in 0..3 {
    manager.vote(&Address::generate(&env), &pid, &true);
}

// 7. Advance past the voting period (voting_period=100)
advance_past_voting(&env, &manager_id, pid);

// 8. Finalize — moves to Queued
assert_eq!(manager.finalize(&pid), ProposalStatus::Queued);

// 9. Advance past the timelock (timelock_delay=17_280)
advance_past_timelock(&env, &manager_id, pid);

// 10. Execute — calls target.upgrade(new_hash), proposal → Executed
manager.execute(&pid);

// 11. Assertions
assert_eq!(manager.get_proposal(&pid).status, ProposalStatus::Executed);
assert_eq!(target.version(), 1u32); // version unchanged in mock host
```

### Helper functions

```rust
fn advance_past_voting(env: &Env, cid: &Address, proposal_id: u64) {
    const ADVANCE: u32 = 101; // 1 past voting_period=100
    const BUMP: u32 = ADVANCE + 30_000;
    env.as_contract(cid, || {
        env.storage().instance().extend_ttl(BUMP, BUMP);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
    });
    env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
}

fn advance_past_timelock(env: &Env, cid: &Address, proposal_id: u64) {
    const ADVANCE: u32 = 17_382; // 101 (past voting) + 17_281 (past timelock)
    const BUMP: u32 = ADVANCE + 30_000;
    env.as_contract(cid, || {
        env.storage().instance().extend_ttl(BUMP, BUMP);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
    });
    env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
}
```

The TTL bumps prevent storage entries from expiring while the test advances
the ledger clock.


---

## 4. Test environment limitations

### 4.1 soroban-wasmi rejects modern WASM

The test host uses `soroban-wasmi 0.31.1` (via `soroban-env-host 22.1.3`),
which rejects WASM binaries compiled by modern Rust toolchains (Rust ≥ 1.85.0)
because they emit multi-byte LEB128 zeros for a reserved table-index byte in
`call_indirect` instructions.

**Consequence:** You cannot upload real target fixture WASMs
(`target_v1.wasm`, `target_v2.wasm`) via `upload_contract_wasm()` in tests.
Instead, use empty bytes as a placeholder hash:

```rust
let placeholder_hash = env
    .deployer()
    .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
```

The real fixture WASMs are committed to `tests/fixtures/` with checksums in
`tests/fixtures/SHA256SUMS` (built by `scripts/build_fixtures.sh`), but they
are used only for checksum verification in CI, not for upload in tests.

### 4.2 The mock host does not hot-swap WASM

The test host runs contracts natively (as Rust code), not through a WASM
interpreter. Calling `env.deployer().update_current_contract_wasm(hash)` does
not actually replace the target's running code. Therefore:

- `version()` will still return `1` after `execute()` in tests — it cannot
  change to `2` because the mock host doesn't load new WASM.
- Tests prove the **governance flow works** (`execute()` calls
  `target.upgrade()` via cross-contract invocation, the proposal transitions
  to `Executed`), but they do **not** prove the target's WASM was actually
  replaced.

The test `test_governance_upgrade_executed_through_manager` explicitly
documents this limitation.

### 4.3 `mock_all_auths`

Most tests call `env.mock_all_auths()` so that any address can invoke any
function without signing. This means tests do **not** verify auth requirements.
To test auth rejection (e.g. a non-manager calling `target.upgrade()`), use a
fresh `Env::default()` without `mock_all_auths`:

```rust
let env2 = Env::default(); // no mock_all_auths
let mgr2 = env2.register(UpgradeManager, ());
let tgt2 = env2.register_at(&Address::generate(&env2), FixtureTarget, (mgr2.clone(),));
let v2h = env2.deployer().upload_contract_wasm(soroban_sdk::Bytes::new(&env2));
let tc2 = FixtureTargetClient::new(&env2, &tgt2);
assert!(tc2.try_upgrade(&v2h).is_err()); // non-manager auth fails
```

This pattern is used in `test_governance_upgrade_executed_through_manager`.


---

## 5. Referenced tests and helpers

All of the following exist in `contracts/upgrade-manager/src/test.rs`:

| Test / helper | Location | What it proves |
|---------------|----------|----------------|
| `MockTarget` | `mod mock_target_contract` | Minimal target with no auth check on `upgrade()`. |
| `FixtureTarget` | `mod fixture_target_contract` | Target whose `upgrade()` authenticates the manager. |
| `NoUpgradeFn` | `mod no_upgrade_contract` | Contract with no `upgrade()` (failure-path tests). |
| `setup()` | `fn setup()` | Standard manager + admin fixture (voting_period=100, timelock=17280, quorum=3, threshold=51). |
| `advance_past_voting()` | `fn advance_past_voting()` | Advances ledger 101 past voting_ends_at. |
| `advance_past_timelock()` | `fn advance_past_timelock()` | Advances ledger past voting period + timelock. |
| `test_full_governance_upgrade_flow` | `fn test_full_governance_upgrade_flow` | Full flow with `MockTarget`: propose → vote → finalize → execute → Executed. |
| `test_governance_upgrade_executed_through_manager` | `fn test_governance_upgrade_executed_through_manager` | Full flow with `FixtureTarget`: `execute()` calls `upgrade()` via cross-contract call, proposal → Executed; direct non-manager `upgrade()` fails. |
| `test_real_wasm_upgrade_v1_to_v2_through_manager` | `fn test_real_wasm_upgrade_v1_to_v2_through_manager` | Registers real `target_v1.wasm`, asserts `version()==1`, runs full flow, asserts `version()==2` and `Executed`. |

---

## 6. Checklist before writing your test

- [ ] Your target contract's `upgrade()` authenticates the manager
  (`manager.require_auth()` before `update_current_contract_wasm()`).
- [ ] Each helper contract is in its own `#[cfg(test)] mod`.
- [ ] You call `env.mock_all_auths()` for flow tests (or omit it for auth-rejection tests).
- [ ] You use empty bytes (`Bytes::new(&env)`) as the WASM hash placeholder.
- [ ] You bump TTL before advancing the ledger (`advance_past_voting` / `advance_past_timelock`).
- [ ] You assert the proposal status transitions to `Executed` after `execute()`.
- [ ] You assert direct `target.upgrade()` by a non-manager address fails.

---

## See also

- [contracts/example-target/src/lib.rs](../contracts/example-target/src/lib.rs) — reference target implementation with manager-auth `upgrade()`.
- [contracts/upgrade-manager/src/test.rs](../contracts/upgrade-manager/src/test.rs) — full test suite with helper contracts and all cited tests.
- [docs/wasm-compatibility.md](./wasm-compatibility.md) — how to verify storage-layout compatibility before proposing an upgrade.
- [README.md — Recommended Workflow](../README.md#recommended-workflow) — end-to-end upgrade workflow.

```
