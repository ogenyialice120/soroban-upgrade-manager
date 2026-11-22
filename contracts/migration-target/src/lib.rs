//! # Migration Target Contract
//!
//! A minimal upgradable contract that demonstrates **storage migration during
//! an upgrade**: the value stored under the v1 key `old_key` is moved to the v2
//! key `new_key` inside `upgrade()`, *before* the contract's WASM is replaced.
//!
//! This is the recommended Soroban pattern for changing a storage layout across
//! an upgrade. `env.deployer().update_current_contract_wasm` swaps the code in
//! place and preserves storage, but the **old** code is what runs during the
//! `upgrade()` call itself. The migration therefore must be performed by the
//! old contract's `upgrade()` — before it hands control to the new WASM —
//! because that is the only moment code that understands the old layout is
//! guaranteed to execute. After the swap, the new (v2) code reads from
//! `new_key`.
//!
//! ## Versions
//!
//! - **v1** (default, no features): `set_value(v)` stores a `u32` under
//!   `old_key`; `get_value()` reads `old_key`; `version()` returns `1`.
//! - **v2** (`--features v2`): `get_value()` reads `new_key`; `version()`
//!   returns `2`. v2 does not expose `set_value` — the value arrives via the
//!   migration performed by `upgrade()`.
//!
//! ## The `upgrade()` function (shared by both versions)
//!
//! `upgrade(new_wasm_hash)`:
//! 1. Loads the stored manager and calls `manager.require_auth()`.
//! 2. **Migrates**: if a value exists under `old_key`, copies it to `new_key`
//!    and removes `old_key`. This is idempotent — a no-op when `old_key` is
//!    already absent (e.g. a re-upgrade of an already-migrated contract).
//! 3. Calls `env.deployer().update_current_contract_wasm(new_wasm_hash)`.
//!
//! Because this `upgrade()` is the *old* contract's code (v1 in a v1→v2
//! upgrade), the migration runs before the WASM swap. This matches the
//! approach recommended in the hint on issue #131: run the migration in the
//! old contract's `upgrade()` rather than lazily on the new code's first call,
//! so the new code can assume `new_key` is authoritative from the start.
//!
//! ## Security model
//!
//! The manager address is stored atomically at deploy time via
//! `__constructor`, so there is no window for a front-running attack.
//! `upgrade()` requires the manager's auth before migrating or swapping the
//! WASM. Instance storage (`manager`, `old_key`, `new_key`) is preserved by
//! the upgrade.

#![no_std]

// Tests run as a native binary and need std, even though the library
// crate is #![no_std] for the WASM target.
#[cfg(test)]
extern crate std;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Symbol};

#[contract]
pub struct MigrationTarget;

/// The v1 storage key holding the value before migration.
fn old_key(env: &Env) -> Symbol {
    Symbol::new(env, "old_key")
}

/// The v2 storage key the value is migrated to by `upgrade()`.
fn new_key(env: &Env) -> Symbol {
    Symbol::new(env, "new_key")
}

#[contractimpl]
impl MigrationTarget {
    /// Deploy-time initializer. Stores `manager` as the only address that may
    /// call `upgrade()`. Called automatically by the Soroban host during
    /// contract deployment; cannot be invoked again afterward.
    pub fn __constructor(env: Env, manager: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
    }

    /// Replace this contract's WASM with `new_wasm_hash`.
    ///
    /// Before swapping the WASM, migrate the value from `old_key` to
    /// `new_key`. In a v1→v2 upgrade this runs as the **old** (v1) contract
    /// code, so the migration happens before the new (v2) code takes over.
    /// Only the stored manager may trigger an upgrade.
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

        // Migrate old_key -> new_key before the WASM swap. Idempotent: a
        // no-op when old_key is already absent.
        let value: Option<u32> = env.storage().instance().get(&old_key(&env));
        if let Some(v) = value {
            env.storage().instance().set(&new_key(&env), &v);
            env.storage().instance().remove(&old_key(&env));
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

// ---------------------------------------------------------------------------
// v1 (default): store and read the value under old_key.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "v2"))]
#[contractimpl]
impl MigrationTarget {
    /// Store `value` under `old_key`.
    pub fn set_value(env: Env, value: u32) {
        env.storage().instance().set(&old_key(&env), &value);
    }

    /// Return the value stored under `old_key` (`0` if unset).
    pub fn get_value(env: Env) -> u32 {
        env.storage().instance().get(&old_key(&env)).unwrap_or(0)
    }

    /// Return the contract version: `1` without the `v2` feature.
    pub fn version(_env: Env) -> u32 {
        1
    }
}

// ---------------------------------------------------------------------------
// v2 (`--features v2`): read the migrated value under new_key.
// ---------------------------------------------------------------------------

#[cfg(feature = "v2")]
#[contractimpl]
impl MigrationTarget {
    /// Return the value stored under `new_key` (`0` if unset). The value
    /// arrives here via the migration performed by `upgrade()`.
    pub fn get_value(env: Env) -> u32 {
        env.storage().instance().get(&new_key(&env)).unwrap_or(0)
    }

    /// Return the contract version: `2` with the `v2` feature.
    pub fn version(_env: Env) -> u32 {
        2
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// The migration test deploys v1 (it calls the v1-only `set_value`), so it is
// only compiled for the default (v1) build. The v2 build has no test.
#[cfg(all(test, not(feature = "v2")))]
mod test {
    use crate::{MigrationTarget, MigrationTargetClient};
    use soroban_sdk::{testutils::Address as _, Address, Env};
    use std::format;

    /// Builds (or reuses) this crate's `v2` feature WASM and returns its
    /// bytes. Building at runtime keeps the test self-contained (no committed
    /// binary fixture) and guarantees the upgraded WASM matches the current
    /// `v2` feature source. Requires the `wasm32v1-none` target, which the
    /// acceptance criteria assume is installed.
    ///
    /// As a workspace member, this crate's build artefacts land in the
    /// *workspace* `target/` directory (two levels above this crate), so the
    /// artefact is looked up there.
    fn v2_wasm_bytes(env: &Env) -> soroban_sdk::Bytes {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for host tests");
        let artifact = format!("{}.wasm", env!("CARGO_PKG_NAME").replace('-', "_"));
        // Workspace target is two levels up from the crate manifest dir.
        let wasm_path = std::path::PathBuf::from(&manifest_dir)
            .join("..")
            .join("..")
            .join("target")
            .join("wasm32v1-none")
            .join("release")
            .join(&artifact);

        if !wasm_path.exists() {
            let manifest_path = std::path::PathBuf::from(&manifest_dir).join("Cargo.toml");
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

    /// Deploy v1, store a value under `old_key`, upgrade to v2, and verify
    /// the value is readable under `new_key` after the upgrade. Proves the
    /// migration in the old contract's `upgrade()` moves the value before the
    /// WASM swap.
    #[test]
    fn test_storage_migrates_old_key_to_new_key_on_upgrade() {
        let env = Env::default();
        env.mock_all_auths();

        let manager = Address::generate(&env);
        let target_id = env.register(MigrationTarget, (manager.clone(),));
        let client = MigrationTargetClient::new(&env, &target_id);

        // v1: store a value under old_key.
        assert_eq!(client.version(), 1u32, "v1 reports version 1");
        client.set_value(&42);
        assert_eq!(
            client.get_value(),
            42,
            "v1 must read the value under old_key"
        );

        // Upgrade to the v2 WASM. The deployed v1's upgrade() migrates
        // old_key -> new_key before swapping the WASM.
        let wasm_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(wasm_bytes);
        client.upgrade(&new_wasm_hash);

        // v2: the value is now readable under new_key.
        assert_eq!(client.version(), 2u32, "v2 reports version 2");
        assert_eq!(
            client.get_value(),
            42,
            "v2 must read the migrated value under new_key"
        );
    }
}
