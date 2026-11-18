//! # Token Target Contract
//!
//! A minimal upgradable contract that holds a token balance and survives a
//! WASM upgrade, used as an example target for the Soroban Upgrade Manager.
//!
//! The contract stores a `manager` address (set at deploy time) and a token
//! `balance` in persistent storage. Because Soroban persistent storage is
//! preserved by `env.deployer().update_current_contract_wasm`, the balance
//! survives upgrades.
//!
//! - `__constructor(manager)` — stores the manager address at deploy time.
//! - `get_balance()` — returns the tracked token balance.
//! - `receive(from, amount)` — requires `from`'s auth, then credits `amount`
//!   to the tracked balance.
//! - `upgrade(new_wasm_hash)` — calls `manager.require_auth()` then replaces
//!   the contract's WASM.
//!
//! With the `v2` feature, `transfer(to, amount)` is added so the contract can
//! send tokens from its tracked balance.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct TokenTarget;

#[contractimpl]
impl TokenTarget {
    /// Deploy-time initializer. Stores `manager` as the only address that may
    /// call `upgrade()`. Called automatically by the Soroban host during
    /// contract deployment; cannot be invoked again afterward.
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    /// Return the tracked token balance. Defaults to `0`.
    ///
    /// The balance is stored in persistent storage, so it survives WASM
    /// upgrades.
    pub fn get_balance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&soroban_sdk::symbol_short!("balance"))
            .unwrap_or(0)
    }

    /// Credit `amount` tokens to the contract's tracked balance.
    ///
    /// `from` must authenticate. In a production deployment this would also
    /// invoke the token contract's `transfer(from, this_contract, amount)`
    /// via `env.invoke_contract` so that real tokens move with the booking.
    pub fn receive(env: Env, from: Address, amount: i128) {
        from.require_auth();
        let balance = Self::get_balance(env);
        let new_balance = balance
            .checked_add(amount)
            .expect("balance overflow");
        env.storage()
            .persistent()
            .set(&soroban_sdk::symbol_short!("balance"), &new_balance);
    }

    /// Replace this contract's WASM with `new_wasm_hash`.
    ///
    /// Only the manager address set during construction may call this. The
    /// manager must authenticate (sign) the transaction. Persistent storage
    /// (including `balance`) is preserved by the upgrade.
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

#[cfg(feature = "v2")]
#[contractimpl]
impl TokenTarget {
    /// Debit `amount` tokens from the contract's tracked balance and send
    /// them to `to`.
    ///
    /// The contract itself must authorize the transfer. Persistent storage
    /// (including `balance`) is preserved across upgrades.
    pub fn transfer(env: Env, to: Address, amount: i128) {
        let contract = env.current_contract_address();
        contract.require_auth();
        let balance = Self::get_balance(env);
        let new_balance = balance
            .checked_sub(amount)
            .expect("insufficient balance");
        env.storage()
            .persistent()
            .set(&soroban_sdk::symbol_short!("balance"), &new_balance);
        // In a production deployment this would also invoke the token
        // contract's `transfer(this_contract, to, amount)` via
        // `env.invoke_contract` so that real tokens move with the booking.
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use crate::{TokenTarget, TokenTargetClient};
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

    /// Builds (or reuses) this crate's `v2` feature WASM and returns its
    /// bytes. See the same helper in `counter-target` for the rationale.
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
    fn test_token_balance_persists_across_upgrade() {
        let env = Env::default();
        env.mock_all_auths();

        let manager = Address::generate(&env);
        let from = Address::generate(&env);
        let token_id = env.register(TokenTarget, (manager.clone(),));
        let client = TokenTargetClient::new(&env, &token_id);

        // Credit tokens.
        client.receive(&from, &100);
        assert_eq!(client.get_balance(), 100);

        // Upgrade to the v2 WASM. The tracked balance lives in persistent
        // storage and must survive the WASM replacement.
        let wasm_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
        client.upgrade(&new_wasm_hash);

        assert_eq!(
            client.get_balance(),
            100,
            "token balance must persist across the upgrade"
        );
    }
}