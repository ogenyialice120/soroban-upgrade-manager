//! # Example Target Contract
//!
//! A minimal upgradable contract used to demonstrate and test the Soroban Upgrade Manager.
//!
//! ## Security model
//!
//! The address of the upgrade-manager that is allowed to call `upgrade()` is
//! stored at deploy time via `__constructor`. The constructor is called
//! **atomically** during deployment and cannot be called again, so there is
//! no window for a front-running attack.
//!
//! - `version()` — returns 1 (or 2 when compiled with the `v2` feature flag).
//! - `upgrade(new_wasm_hash)` — calls `manager.require_auth()` then
//!   `env.deployer().update_current_contract_wasm(new_wasm_hash)`.
//!   Only the stored manager address may call this function.
//! - `get_manager()` — returns the stored manager address (read-only).
//!
//! ## Preventing front-running
//!
//! Using `__constructor` means the manager address is set in the same
//! transaction that deploys the contract.  There is no separate initializer
//! function that could be called by an observer before the deployer.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ExampleTarget;

#[contractimpl]
impl ExampleTarget {
    // -----------------------------------------------------------------------
    // Constructor — called ONCE at deploy time; cannot be re-called.
    // -----------------------------------------------------------------------

    /// Deploy-time initializer.  Stores `manager` as the only address that
    /// may call `upgrade()`.  Called automatically by the Soroban host during
    /// contract deployment; cannot be invoked again afterward.
    pub fn __constructor(env: Env, manager: Address) {
        // Require auth from the manager so the deployer must control it.
        manager.require_auth();
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    // -----------------------------------------------------------------------
    // Read-only queries
    // -----------------------------------------------------------------------

    /// Return the contract version.
    ///
    /// Returns `1` when built without the `v2` feature flag.
    /// Returns `2` when built with `--features v2`.
    pub fn version(_env: Env) -> u32 {
        #[cfg(feature = "v2")]
        return 2;
        #[cfg(not(feature = "v2"))]
        return 1;
    }

    /// Return the stored manager address.
    pub fn get_manager(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set")
    }

    // -----------------------------------------------------------------------
    // Upgrade — only callable by the stored manager
    // -----------------------------------------------------------------------

    /// Replace this contract's WASM with `new_wasm_hash`.
    ///
    /// Only the manager address set during construction may call this.
    /// The manager must authenticate (sign) the transaction.
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
