//! # Triple Target Contract
//!
//! A target contract that can be upgraded multiple times in sequence. It has
//! three feature flags:
//! - v1 (default): `version()` returns `1`.
//! - v2 (`--features v2`): `version()` returns `2`.
//! - v3 (`--features v3`): `version()` returns `3`.
//!
//! This contract is used to prove the upgrade mechanism works repeatedly and
//! that each upgrade preserves the manager relationship.
//!
//! - `__constructor(manager)` — stores the manager address at deploy time.
//! - `version()` — returns `1`, `2`, or `3` depending on the active feature flag.
//! - `get_manager()` — returns the stored manager address.
//! - `upgrade(new_wasm_hash)` — calls `manager.require_auth()` then
//!   `env.deployer().update_current_contract_wasm(new_wasm_hash)`. Only the
//!   stored manager may call this.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct TripleTarget;

#[contractimpl]
impl TripleTarget {
    /// Deploy-time initializer. Stores `manager` at deploy time. The manager
    /// is set atomically during deployment, so there is no window for a
    /// front-running attack.
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    /// Return the contract version: `1` (v1), `2` (v2), or `3` (v3).
    pub fn version(_env: Env) -> u32 {
        #[cfg(feature = "v3")]
        return 3;
        #[cfg(feature = "v2")]
        return 2;
        #[cfg(not(any(feature = "v2", feature = "v3")))]
        return 1;
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
    /// Only the manager address set during construction may call this. The
    /// manager must authenticate (sign) the transaction.
    ///
    /// # Panics
    /// - If `manager.require_auth()` fails (not called by the manager).
    /// - If `new_wasm_hash` is not a registered WASM on the network.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let manager: Address = env
            .storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set");
        // Only the stored manager may trigger an upgrade.
        manager.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}
