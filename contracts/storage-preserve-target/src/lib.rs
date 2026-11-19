//! # Storage Preserve Target Contract
//!
//! A minimal upgradable contract that proves storage layout is preserved
//! across a v1→v2 WASM upgrade.
//!
//! Both v1 and v2 store a counter under `symbol_short!("count")` in instance
//! storage.
//!
//! - v1 (default): `set_count(count)` stores a value, `get_count()` reads it.
//! - v2 (`--features v2`): `get_count()` reads it, `increment()` reads and
//!   increments the same key, proving the key still works after upgrade.
//!
//! ## Security model
//!
//! The manager address is stored atomically during deployment via
//! `__constructor`, so there is no window for a front-running attack.
//! `upgrade()` requires the manager's auth before replacing the contract's
//! WASM. Instance storage (including `count`) is preserved by the upgrade.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct StoragePreserveTarget;

#[contractimpl]
impl StoragePreserveTarget {
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    pub fn get_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("count"))
            .unwrap_or(0)
    }

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

#[cfg(not(feature = "v2"))]
#[contractimpl]
impl StoragePreserveTarget {
    pub fn set_count(env: Env, count: u32) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("count"), &count);
    }
}

#[cfg(feature = "v2")]
#[contractimpl]
impl StoragePreserveTarget {
    pub fn increment(env: Env) -> u32 {
        let count = Self::get_count(env);
        let new_count = count.saturating_add(1);
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("count"), &new_count);
        new_count
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use crate::{StoragePreserveTarget, StoragePreserveTargetClient};
    use soroban_sdk::{testutils::Address as _, Address, Env};

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
    fn test_count_survives_upgrade() {
        let env = Env::default();
        env.mock_all_auths();

        let manager = Address::generate(&env);
        let target_id = env.register(StoragePreserveTarget, (manager.clone(),));
        let client = StoragePreserveTargetClient::new(&env, &target_id);

        client.set_count(&42);
        assert_eq!(client.get_count(), 42, "count must be 42 before upgrade");

        let wasm_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
        client.upgrade(&new_wasm_hash);

        assert_eq!(
            client.get_count(),
            42,
            "count must persist across the upgrade"
        );

        let increment_sym = soroban_sdk::Symbol::new(&env, "increment");
        let new_count: u32 =
            env.invoke_contract(&target_id, &increment_sym, soroban_sdk::vec![&env]);
        assert_eq!(
            new_count,
            43,
            "v2 increment must read the same count key (42 + 1 = 43)"
        );
        assert_eq!(client.get_count(), 43);
    }
}