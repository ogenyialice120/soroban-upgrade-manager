//! # Bad Target Contract — ANTI-EXAMPLE
//!
//! ⚠️ WARNING: This contract is an **anti-example**. Do NOT copy this pattern.
//!
//! It deliberately OMITS `manager.require_auth()` in `upgrade()`, so anyone who
//! can invoke the contract may replace its WASM without the manager's
//! authorization. The manager address is stored at deploy time but never
//! enforced, which is the entire vulnerability this contract documents.
//!
//! Use this contract only to understand *why* `upgrade()` MUST call
//! `manager.require_auth()` before `env.deployer().update_current_contract_wasm()`.
//! See `test_anyone_can_upgrade_without_auth` for a proof of the vulnerability.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct BadTarget;

#[contractimpl]
impl BadTarget {
    /// Deploy-time initializer. Stores `manager` at deploy time. `upgrade()`
    /// never authenticates the manager, so this storage is not enforced.
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    /// Return the contract version (always 1).
    pub fn version(_env: Env) -> u32 {
        1
    }

    /// Return the stored manager address.
    pub fn get_manager(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set")
    }

    /// Replace this contract's WASM with `new_wasm_hash`.
    ///
    /// # VULNERABILITY
    ///
    /// This function does NOT call `manager.require_auth()`. Any caller may
    /// replace the contract's WASM, completely bypassing the manager. This is
    /// the anti-pattern this contract exists to document.
    ///
    /// # Panics
    /// - If `new_wasm_hash` is not a registered WASM on the network.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        // Intentionally no `manager.require_auth()` — anyone can upgrade.
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

#[cfg(test)]
mod test {
    use crate::{BadTarget, BadTargetClient};
    use soroban_sdk::{testutils::Address as _, Address, Env};

    /// Anyone can call `upgrade()` without the manager's authorization because
    /// `upgrade()` never calls `manager.require_auth()`.
    ///
    /// This mirrors `test_real_example_target_upgrade_rejects_non_manager` in
    /// the upgrade-manager suite, which shows the *good* target rejecting the
    /// same call. Here the call succeeds, proving the risk.
    #[test]
    fn test_anyone_can_upgrade_without_auth() {
        let env = Env::default();
        // Intentionally NOT calling env.mock_all_auths().

        let manager = Address::generate(&env);
        let target_id = env.register(BadTarget, (manager.clone(),));
        let target_client = BadTargetClient::new(&env, &target_id);

        // Upload a valid (empty) WASM so `update_current_contract_wasm` has a
        // hash that exists in the ledger.
        let wasm_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

        // A caller that is not the manager and has not authorized anything
        // invokes upgrade(). Because upgrade() does not call
        // manager.require_auth(), this succeeds — demonstrating the
        // vulnerability.
        assert!(
            target_client.try_upgrade(&wasm_hash).is_ok(),
            "upgrade() must succeed without manager auth (anti-example: no require_auth)"
        );
    }
}
