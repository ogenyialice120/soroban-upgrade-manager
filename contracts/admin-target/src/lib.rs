//! # Admin Target Contract
//!
//! An upgradable example target that demonstrates a realistic multi-role
//! security model for the Soroban Upgrade Manager.
//!
//! The contract keeps two distinct roles, stored as separate instance
//! storage keys at deploy time via `__constructor`:
//!
//! - `manager` — the Soroban Upgrade Manager contract. The only address
//!   allowed to call `upgrade()`, i.e. upgrade authority is fully
//!   delegated to the governance + timelock process.
//! - `admin` — the target's own administrator. The only address allowed
//!   to call `admin_action()`, an example of a regular (non-upgrade)
//!   administrative function that stays under direct admin control.
//!
//! ## Functions
//!
//! - `__constructor(env, manager, admin)` — stores both roles atomically
//!   at deploy time; cannot be called again afterwards.
//! - `version()` — returns 1 (2 when compiled with the `v2` feature flag).
//! - `admin_action(env)` — requires `admin.require_auth()`; increments and
//!   returns the admin-call counter. Demonstrates an admin-only function.
//! - `upgrade(env, new_wasm_hash)` — requires `manager.require_auth()`
//!   (NOT the admin), then replaces the contract WASM.
//! - `get_manager()` / `get_admin()` — read-only role lookups.
//!
//! ## Role separation
//!
//! The admin can run day-to-day operations but cannot upgrade the contract.
//! The manager can upgrade the contract but cannot run admin operations.
//! Tests below prove both directions of this separation.

#![no_std]

// Tests run as a native binary and need std, even though the library
// crate is #![no_std] for the WASM target.
#[cfg(test)]
extern crate std;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct AdminTarget;

#[contractimpl]
impl AdminTarget {
    // -----------------------------------------------------------------------
    // Constructor — called ONCE at deploy time; cannot be re-called.
    // -----------------------------------------------------------------------

    /// Deploy-time initializer. Stores `manager` (the upgrade authority)
    /// and `admin` (the operational administrator) as separate storage keys.
    ///
    /// The manager is a contract address and cannot sign, so we do NOT call
    /// `require_auth()` here. Deploying is atomic, so there is no window
    /// for a front-running attack on either role.
    pub fn __constructor(env: Env, manager: Address, admin: Address) {
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("manager"), &manager);
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("admin"), &admin);
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("acalls"), &0u32);
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

    /// Return the stored upgrade-manager address (upgrade authority).
    pub fn get_manager(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set")
    }

    /// Return the stored operational admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("admin"))
            .expect("admin not set")
    }

    /// Return the number of successful `admin_action` calls (read-only).
    pub fn get_admin_calls(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("acalls"))
            .unwrap_or(0u32)
    }

    // -----------------------------------------------------------------------
    // Admin-only operation — the target's own admin, not the manager
    // -----------------------------------------------------------------------

    /// Example admin-only function.
    ///
    /// Requires the stored `admin` address to authenticate. Increments the
    /// admin-call counter and returns the new value.
    ///
    /// # Panics
    /// - If `admin.require_auth()` fails (not called by the admin, e.g. by
    ///   the upgrade manager).
    pub fn admin_action(env: Env) -> u32 {
        let admin: Address = env
            .storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("admin"))
            .expect("admin not set");
        // Only the target's own admin may run regular operations.
        admin.require_auth();

        let calls: u32 = env
            .storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("acalls"))
            .unwrap_or(0u32);
        let calls = calls + 1;
        env.storage()
            .instance()
            .set(&soroban_sdk::symbol_short!("acalls"), &calls);
        calls
    }

    // -----------------------------------------------------------------------
    // Upgrade — only callable by the stored manager (delegated authority)
    // -----------------------------------------------------------------------

    /// Replace this contract's WASM with `new_wasm_hash`.
    ///
    /// Requires the stored `manager` address to authenticate — the target's
    /// own admin explicitly CANNOT trigger an upgrade directly; upgrades go
    /// through the manager's governance + timelock flow.
    ///
    /// # Panics
    /// - If `manager.require_auth()` fails (not called by the manager, e.g.
    ///   by the target's own admin).
    /// - If `new_wasm_hash` is not a registered WASM on the network.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let manager: Address = env
            .storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("manager"))
            .expect("manager not set");
        // Only the upgrade manager may trigger an upgrade — not the admin.
        manager.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Tests for the two-role security model. Auth is mocked selectively with
// `env.mock_auths` (NOT `mock_all_auths`) so that only the intended role
// is authenticated; the other role's `require_auth()` then fails, proving
// the separation.
#[cfg(test)]
mod test {
    use crate::{AdminTarget, AdminTargetClient};
    use soroban_sdk::testutils::{Address as _, MockAuth, MockAuthInvoke};
    use soroban_sdk::{vec, Address, Env, IntoVal};

    /// Register the admin-target and return (env, client, target, manager, admin).
    fn setup() -> (Env, AdminTargetClient<'static>, Address, Address, Address) {
        let env = Env::default();
        let manager = Address::generate(&env);
        let admin = Address::generate(&env);
        let target_id = env.register(AdminTarget, (manager.clone(), admin.clone()));
        let client = AdminTargetClient::new(&env, &target_id);
        (env, client, target_id, manager, admin)
    }

    /// The constructor stores both roles.
    #[test]
    fn test_constructor_stores_both_roles() {
        let (_env, client, _target, manager, admin) = setup();
        assert_eq!(client.get_manager(), manager);
        assert_eq!(client.get_admin(), admin);
        assert_eq!(client.version(), 1);
        assert_eq!(client.get_admin_calls(), 0);
    }

    /// admin_action succeeds when the admin authenticates.
    #[test]
    fn test_admin_action_with_admin_auth_succeeds() {
        let (env, client, target, _manager, admin) = setup();
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &target,
                fn_name: "admin_action",
                args: vec![&env],
                sub_invokes: &[],
            },
        }]);
        assert_eq!(client.admin_action(), 1);
        // A second authenticated call needs a fresh nonce, so re-mock.
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &target,
                fn_name: "admin_action",
                args: vec![&env],
                sub_invokes: &[],
            },
        }]);
        assert_eq!(client.admin_action(), 2);
        assert_eq!(client.get_admin_calls(), 2);
    }

    /// admin_action fails when only the MANAGER is authenticated — the
    /// manager cannot run the target's admin operations.
    #[test]
    fn test_admin_action_with_manager_auth_fails() {
        let (env, client, target, manager, admin) = setup();

        // Sanity: with admin auth the call succeeds.
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &target,
                fn_name: "admin_action",
                args: vec![&env],
                sub_invokes: &[],
            },
        }]);
        assert_eq!(client.admin_action(), 1);

        // Now authenticate the manager instead — admin.require_auth() fails
        // with a host-level auth error (not one of the target's errors).
        env.mock_auths(&[MockAuth {
            address: &manager,
            invoke: &MockAuthInvoke {
                contract: &target,
                fn_name: "admin_action",
                args: vec![&env],
                sub_invokes: &[],
            },
        }]);
        assert!(
            client.try_admin_action().is_err(),
            "admin_action authenticated as the manager must fail"
        );
        // State unchanged by the failed call.
        assert_eq!(client.get_admin_calls(), 1);
    }

    /// admin_action fails without any auth at all.
    #[test]
    fn test_admin_action_without_admin_auth_fails() {
        let (_env, client, _target, _manager, _admin) = setup();
        assert!(
            client.try_admin_action().is_err(),
            "admin_action without admin auth must fail"
        );
    }

    /// upgrade succeeds when the MANAGER authenticates (v2 WASM), proving
    /// the upgrade path is live while the admin has no say in it.
    #[test]
    fn test_upgrade_with_manager_auth_succeeds() {
        let (env, client, target, manager, _admin) = setup();

        let v2_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(v2_bytes);

        env.mock_auths(&[MockAuth {
            address: &manager,
            invoke: &MockAuthInvoke {
                contract: &target,
                fn_name: "upgrade",
                args: (new_wasm_hash.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        client.upgrade(&new_wasm_hash);

        // The contract now runs the v2 WASM.
        assert_eq!(client.version(), 2);
    }

    /// upgrade fails when only the target's own ADMIN authenticates —
    /// the admin cannot bypass the governance + timelock flow.
    #[test]
    fn test_upgrade_with_admin_auth_fails() {
        let (env, client, target, _manager, admin) = setup();

        // Register a real WASM so the failure is attributable to auth, not
        // to an unregistered hash.
        let v2_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(v2_bytes);

        // Authenticate as the target's own admin.
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &target,
                fn_name: "upgrade",
                args: (new_wasm_hash.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        let result = client.try_upgrade(&new_wasm_hash);
        assert!(
            result.is_err(),
            "upgrade authenticated as the target admin must fail"
        );
        // The contract was not upgraded.
        assert_eq!(client.version(), 1);
    }

    /// upgrade fails without any auth at all.
    #[test]
    fn test_upgrade_without_manager_auth_fails() {
        let (env, client, _target, _manager, _admin) = setup();
        let v2_bytes = v2_wasm_bytes(&env);
        let new_wasm_hash = env.deployer().upload_contract_wasm(v2_bytes);
        assert!(
            client.try_upgrade(&new_wasm_hash).is_err(),
            "upgrade without manager auth must fail"
        );
        assert_eq!(client.version(), 1);
    }

    // -------------------------------------------------------------------
    // v2 WASM helper — same approach as counter-target.
    // -------------------------------------------------------------------

    fn v2_wasm_bytes(env: &Env) -> soroban_sdk::Bytes {
        use std::format;
        use std::string::ToString;

        let artifact =
            std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "admin_target.wasm".to_string());
        let artifact = format!("{}.wasm", artifact.replace('-', "_"));

        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let wasm_path = std::path::Path::new(&manifest_dir)
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
}
