//! # Feature-Add Target Contract
//!
//! A target contract that demonstrates an upgrade adding an entirely new
//! function. v1 provides `version()`, `upgrade()` and `get_manager()`. v2
//! (built with `--features v2`) additionally exposes `get_description()`
//! while `version()` returns 2.
//!
//! - `__constructor(manager)` — stores the manager address at deploy time.
//! - `version()` — returns `1` (v1) or `2` (v2).
//! - `get_description()` — v2 only: returns a description string.
//! - `get_manager()` — returns the stored manager address.
//! - `upgrade(new_wasm_hash)` — calls `manager.require_auth()` then
//!   `env.deployer().update_current_contract_wasm(new_wasm_hash)`.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct FeatureAddTarget;

#[contractimpl]
impl FeatureAddTarget {
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    /// Return the contract version: `1` without `v2`, `2` with `v2`.
    pub fn version(_env: Env) -> u32 {
        #[cfg(feature = "v2")]
        return 2;
        #[cfg(not(feature = "v2"))]
        return 1;
    }

    /// v2 only: return a human-readable description of the contract.
    #[cfg(feature = "v2")]
    pub fn get_description(_env: Env) -> soroban_sdk::String {
        soroban_sdk::String::from_str(
            &_env,
            "feature-add-target v2: adds get_description()",
        )
    }

    /// Return the stored manager address.
    pub fn get_manager(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set")
    }

    /// Replace this contract's WASM with `new_wasm_hash`.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let manager: Address = env
            .storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set");
        manager.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

#[cfg(test)]
mod test {
    use crate::{FeatureAddTarget, FeatureAddTargetClient};
    use soroban_sdk::{
        testutils::Address as _, Address, BytesN, Env, Symbol,
    };

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

    /// `get_description` is not present on v1 (function not found), but exists
    /// and returns a description after upgrading to v2.
    #[test]
    fn test_get_description_only_after_upgrade() {
        let env = Env::default();
        env.mock_all_auths();

        let manager = Address::generate(&env);
        let target_id = env.register(FeatureAddTarget, (manager.clone(),));
        let client = FeatureAddTargetClient::new(&env, &target_id);

        assert_eq!(client.version(), 1u32);

        // v1 does not expose get_description: invoking it by name must fail
        // (function not found), which surfaces as a panic from the host.
        let desc_sym = Symbol::new(&env, "get_description");
        let panicked = std::panic::catch_unwind(|| {
            let _: soroban_sdk::String =
                env.invoke_contract(&target_id, &desc_sym, soroban_sdk::vec![&env]);
        });
        assert!(
            panicked.is_err(),
            "calling get_description on v1 must fail (function not found)"
        );

        // Upgrade to v2.
        let wasm_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
        client.upgrade(&new_wasm_hash);

        // v2 exposes get_description and returns the description string.
        let description: soroban_sdk::String = env.invoke_contract(
            &target_id,
            &desc_sym,
            soroban_sdk::vec![&env],
        );
        assert_eq!(
            description,
            soroban_sdk::String::from_str(
                &env,
                "feature-add-target v2: adds get_description()"
            ),
            "get_description must exist and return the description after upgrade"
        );
        assert_eq!(client.version(), 2u32);
    }
}
