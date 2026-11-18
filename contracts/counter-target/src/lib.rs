//! # Counter Target Contract
//!
//! A minimal upgradable counter contract that serves as a second example
//! target for the Soroban Upgrade Manager upgrade pattern.
//!
//! The contract stores a `count` in instance storage and a `manager`
//! address that is set at deploy time via `__constructor`.
//!
//! - `__constructor(manager)` — stores the manager address at deploy time.
//! - `get_count()` — returns the current count.
//! - `increment()` — increases the count by 1 (by 2 when built with the
//!   `v2` feature flag) and returns the new count.
//! - `upgrade(new_wasm_hash)` — calls `manager.require_auth()` then
//!   `env.deployer().update_current_contract_wasm(new_wasm_hash)`. Only the
//!   stored manager may call this.
//!
//! ## Security model
//!
//! The manager address is stored atomically during deployment, so there is
//! no window for a front-running attack. `upgrade()` requires the manager's
//! auth before replacing the contract's WASM.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct CounterTarget;

#[contractimpl]
impl CounterTarget {
    /// Deploy-time initializer. Stores `manager` as the only address that may
    /// call `upgrade()`. Called automatically by the Soroban host during
    /// contract deployment; cannot be invoked again afterward.
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    /// Return the current count. Defaults to `0` before the first increment.
    pub fn get_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("count"))
            .unwrap_or(0)
    }

    /// Increase the count by 1 (by 2 with the `v2` feature flag) and return
    /// the new count.
    pub fn increment(env: Env) -> u32 {
        let count = Self::get_count(env);
        let new_count = count.saturating_add(increment_delta());
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("count"), &new_count);
        new_count
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

/// Returns the increment delta: `2` with the `v2` feature flag, `1`
/// otherwise. Exactly one of these two functions compiles for any feature
/// combination, so there is no dead code.

#[cfg(feature = "v2")]
fn increment_delta() -> u32 {
    2
}

#[cfg(not(feature = "v2"))]
fn increment_delta() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use crate::{CounterTarget, CounterTargetClient};
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

    /// Builds (or reuses) this crate's `v2` feature WASM and returns its
    /// bytes. Building at runtime keeps the test self-contained (no committed
    /// binary fixture) and guarantees the upgraded WASM matches the current
    /// `v2` feature source. Requires the `wasm32v1-none` target, which the
    /// acceptance criteria assume is installed.
    fn v2_wasm_bytes(env: &Env) -> soroban_sdk::Bytes {
        use std::path::PathBuf;

        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for host tests");
        let artifact = format!("{}.wasm", env!("CARGO_PKG_NAME").replace('-', "_"));
        let wasm_path = PathBuf::from(&manifest_dir)
            .join("target")
            .join("wasm32v1-none")
            .join("release")
            .join(&artifact);

        if !wasm_path.exists() {
            let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");
            let status = std::process::Command::new("cargo")
                .args([
                    "build",
                    "--manifest-path",
                    manifest_path
                        .to_str()
                        .expect("manifest path is valid unicode"),
                    "--features",
                    "v2",
                    "--target",
                    "wasm32v1-none",
                    "--release",
                ])
                .status()
                .expect("failed to execute cargo to build the v2 wasm");
            assert!(
                status.success(),
                "`cargo build --features v2 --target wasm32v1-none --release` failed"
            );
        }

        let bytes = std::fs::read(&wasm_path).expect("failed to read the v2 wasm file");
        soroban_sdk::Bytes::from_slice(env, &bytes)
    }

    #[test]
    fn test_counter_increment_then_upgrade_to_v2() {
        let env = Env::default();
        env.mock_all_auths();

        let manager = Address::generate(&env);
        let counter_id = env.register(CounterTarget, (manager.clone(),));
        let client = CounterTargetClient::new(&env, &counter_id);

        // v1: increment adds 1.
        client.increment();
        assert_eq!(client.get_count(), 1);

        // Upgrade to the v2 WASM (increment now adds 2). Storage persists.
        let wasm_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
        client.upgrade(&new_wasm_hash);

        assert_eq!(
            client.get_count(),
            1,
            "count must persist across the upgrade"
        );
        assert_eq!(
            client.increment(),
            3,
            "v2 increment must add 2 (1 + 2 = 3)"
        );
        assert_eq!(client.get_count(), 3);
    }
}