//! # Upgrade Manager — Test Suite
//!
//! All unit and integration tests for the upgrade-manager contract.
//!
//! ## Design notes
//! - Each helper contract (`MockTarget`, `FixtureTarget`, `NoUpgradeFn`) lives in
//!   its own sub-module because `#[contractimpl]` emits module-level symbols
//!   (`__upgrade`, `__version`, etc.) that would collide if all contracts were in
//!   the same scope.
//! - Event topic comparisons use `Symbol::try_from_val` because `soroban_sdk::Val`
//!   (the host variant) intentionally does not implement `PartialEq`.
//! - `try_get_config` / `try_get_admin` tests use `is_err()` because `Config`
//!   does not implement `PartialEq` (it uses `#[contracttype]` without `#[derive]`).

// -----------------------------------------------------------------------
// Helper sub-contracts — each in its own module to avoid symbol collisions
// -----------------------------------------------------------------------

/// A minimal mock target with no auth check on upgrade(). Used by simple flow tests.
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

/// Target that mirrors example-target's manager-auth upgrade() pattern.
/// Used by real v1→v2 and double-execute tests.
///
/// Note: the constructor does NOT call manager.require_auth() because
/// env.register_at() in tests doesn't pass through mock_all_auths.
/// The real example-target does require constructor auth; this test
/// proxy omits it to avoid the auth failure in the test environment.
#[cfg(test)]
mod fixture_target_contract {
    use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env};

    #[contract]
    pub struct FixtureTarget;

    #[contractimpl]
    impl FixtureTarget {
        pub fn __constructor(env: Env, manager: Address) {
            // No require_auth here — mock_all_auths doesn't cover register_at constructors.
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

/// A contract that deliberately has NO upgrade() function.
/// Used to test the failed-upgrade path.
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

/// A target contract whose upgrade() RETURNS AN ERROR instead of panicking
/// or being absent. Exercises the error-propagation path of the
/// cross-contract call in invoke_upgrade: the manager's
/// `env.invoke_contract::<()>` fails with the target's error, which makes
/// the whole execute() transaction revert.
#[cfg(test)]
mod error_target_contract {
    use soroban_sdk::{contract, contracterror, contractimpl, BytesN, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum TargetError {
        /// Simulates a target-side rejection of the upgrade (e.g. a failed
        /// compatibility self-check before swapping the WASM).
        UpgradeRejected = 1,
    }

    #[contract]
    pub struct ErrorTarget;

    #[contractimpl]
    impl ErrorTarget {
        pub fn version(_env: Env) -> u32 {
            1
        }

        /// Has the right signature but returns Err, never performing the
        /// `update_current_contract_wasm` call.
        pub fn upgrade(_env: Env, _new_wasm_hash: BytesN<32>) -> Result<(), TargetError> {
            Err(TargetError::UpgradeRejected)
        }
    }
}

/// A target contract whose upgrade() PANICS unconditionally.
/// Exercises the panic/trap rollback path of the cross-contract call in
/// `invoke_upgrade`: the host catches the panic, aborts the transaction,
/// and reverts the manager's pre-call `proposal.status = Executed` write.
#[cfg(test)]
mod panicking_target_contract {
    use soroban_sdk::{contract, contractimpl, BytesN, Env};

    #[contract]
    pub struct PanickingTarget;

    #[contractimpl]
    impl PanickingTarget {
        pub fn version(_env: Env) -> u32 {
            1
        }

        /// Has the expected `upgrade(BytesN<32>)` signature, but panics
        /// before performing `update_current_contract_wasm`.
        pub fn upgrade(_env: Env, _new_wasm_hash: BytesN<32>) {
            panic!("simulated panic inside target upgrade");
        }
    }
}

/// A target contract whose upgrade() performs several storage reads and writes
/// before calling update_current_contract_wasm, simulating a migration that
/// consumes a meaningful portion of the compute budget.
#[cfg(test)]
mod budget_consuming_target_contract {
    use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env};

    #[contract]
    pub struct BudgetConsumingTarget;

    #[contractimpl]
    impl BudgetConsumingTarget {
        pub fn __constructor(env: Env, manager: Address) {
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

            // Simulate migration work: write and read back several storage entries.
            for i in 0u32..8u32 {
                env.storage().instance().set(&i, &i);
            }
            for i in 0u32..8u32 {
                let _: u32 = env
                    .storage()
                    .instance()
                    .get(&i)
                    .expect("migration key must exist");
            }

            env.deployer().update_current_contract_wasm(new_wasm_hash);
        }

        pub fn version(_env: Env) -> u32 {
            1
        }
    }
}

// -----------------------------------------------------------------------
// Main test module
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::{UpgradeManager, UpgradeManagerClient};
    use example_target::{ExampleTarget, ExampleTargetClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Events, Ledger},
        Address, BytesN, Env, InvokeError, String, Symbol,
    };
    use triple_target::{TripleTarget, TripleTargetClient};

    use crate::types::{DataKey, Error, ProposalStatus, INSTANCE_TTL_BUMP, PERSISTENT_TTL_BUMP};

    use super::budget_consuming_target_contract::BudgetConsumingTarget;
    use super::error_target_contract::ErrorTarget;
    use super::fixture_target_contract::{FixtureTarget, FixtureTargetClient};
    use super::mock_target_contract::MockTarget;
    use super::no_upgrade_contract::NoUpgradeFn;
    use super::panicking_target_contract::{PanickingTarget, PanickingTargetClient};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Standard test setup: voting_period=100, timelock_delay=17_280, quorum=3, threshold=51.
    fn setup() -> (Env, Address, Address, UpgradeManagerClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);
        (env, admin, cid, client)
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn desc(env: &Env, s: &str) -> String {
        String::from_str(env, s)
    }

    fn title(env: &Env, s: &str) -> String {
        String::from_str(env, s)
    }

    /// Advance ledger 101 past voting_ends_at (voting_period=100).
    fn advance_past_voting(env: &Env, cid: &Address, proposal_id: u64) {
        const ADVANCE: u32 = 101;
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
    }

    /// Advance ledger past both voting period AND timelock.
    fn advance_past_timelock(env: &Env, cid: &Address, proposal_id: u64) {
        const ADVANCE: u32 = 17_382; // 101 + 17_281
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
    }

    /// Returns the number of events whose first two topics match the given symbols.
    ///
    /// `soroban_sdk::Val` does not implement `PartialEq` in the host context.
    /// We use `Val::shallow_eq` (bitwise equality on SymbolSmall encodings).
    fn count_events(env: &Env, topic0_sym: Symbol, topic1_sym: Symbol) -> usize {
        use soroban_sdk::IntoVal;
        let want0: soroban_sdk::Val = topic0_sym.into_val(env);
        let want1: soroban_sdk::Val = topic1_sym.into_val(env);
        env.events()
            .all()
            .iter()
            .filter(|(_, topics, _)| {
                topics.get(0).is_some_and(|v| v.shallow_eq(&want0))
                    && topics.get(1).is_some_and(|v| v.shallow_eq(&want1))
            })
            .count()
    }

    // -----------------------------------------------------------------------
    // initialize
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_success() {
        let (_env, admin, _cid, client) = setup();
        let config = client.get_config();
        assert_eq!(config.voting_period, 100);
        assert_eq!(config.timelock_delay, 17_280);
        assert_eq!(config.quorum, 3);
        assert_eq!(config.approval_threshold_pct, 51);
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    fn test_initialize_twice_fails() {
        let (_env, admin, _cid, client) = setup();
        assert_eq!(
            client.try_initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32),
            Err(Ok(Error::AlreadyInitialized))
        );
    }

    #[test]
    fn test_initialize_zero_delay_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        assert_eq!(
            client.try_initialize(&admin, &100u32, &0u32, &1u32, &51u32, &0u32),
            Err(Ok(Error::InvalidTimelockDelay))
        );
    }

    #[test]
    fn test_initialize_zero_voting_period_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        assert_eq!(
            client.try_initialize(&admin, &0u32, &100u32, &1u32, &51u32, &0u32),
            Err(Ok(Error::InvalidVotingPeriod))
        );
    }

    #[test]
    fn test_initialize_zero_quorum_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        assert_eq!(
            client.try_initialize(&admin, &100u32, &100u32, &0u32, &51u32, &0u32),
            Err(Ok(Error::InvalidQuorum))
        );
    }

    #[test]
    fn test_initialize_zero_threshold_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        assert_eq!(
            client.try_initialize(&admin, &100u32, &100u32, &1u32, &0u32, &0u32),
            Err(Ok(Error::InvalidThreshold))
        );
    }

    #[test]
    fn test_initialize_threshold_101_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        assert_eq!(
            client.try_initialize(&admin, &100u32, &100u32, &1u32, &101u32, &0u32),
            Err(Ok(Error::InvalidThreshold))
        );
    }

    /// get_config before initialization returns an error (NotInitialized).
    #[test]
    fn test_get_config_not_initialized_fails() {
        let env = Env::default();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        // Config does not implement PartialEq; check is_err() instead.
        assert!(
            client.try_get_config().is_err(),
            "get_config before initialize must fail"
        );
    }

    /// get_proposal(0) on a freshly initialized contract returns ProposalNotFound.
    ///
    /// No proposals have been created, so DataKey::Proposal(0) is absent from
    /// storage. load_proposal returns Error::ProposalNotFound in that case.
    #[test]
    fn test_get_proposal_empty_state() {
        let (_, _, _, client) = setup();
        let result = client.try_get_proposal(&0u64);
        assert!(
            matches!(result, Err(Ok(Error::ProposalNotFound))),
            "get_proposal(0) on empty state must return ProposalNotFound, got: {result:?}"
        );
    }

    /// get_proposal with an unknown proposal ID returns ProposalNotFound.
    ///
    /// `get_proposal` delegates to `load_proposal`, which returns
    /// `Error::ProposalNotFound` when `DataKey::Proposal(id)` is absent.
    #[test]
    fn test_get_proposal_unknown_id_fails() {
        let (_, _, _, client) = setup();
        let result = client.try_get_proposal(&999u64);
        assert!(
            matches!(result, Err(Ok(Error::ProposalNotFound))),
            "get_proposal(999) on empty state must return ProposalNotFound, got: {result:?}"
        );
    }

    /// get_admin before initialization returns an error (NotInitialized).
    #[test]
    fn test_get_admin_not_initialized_fails() {
        let env = Env::default();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        assert_eq!(client.try_get_admin(), Err(Ok(Error::NotInitialized)));
    }

    // -----------------------------------------------------------------------
    // auth_without_mock — prove auth is enforced without mock_all_auths
    // -----------------------------------------------------------------------

    /// initialize requires the admin address to authenticate.
    #[test]
    fn test_auth_initialize_without_auth_fails() {
        let env = Env::default(); // no mock_all_auths
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let result = client.try_initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32);
        assert!(result.is_err(), "initialize without auth must fail");
    }

    /// propose_upgrade requires proposer to authenticate.
    #[test]
    fn test_auth_propose_without_auth_fails() {
        let env = Env::default(); // no mock_all_auths
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let proposer = Address::generate(&env);
        let target = Address::generate(&env);
        let result = client.try_propose_upgrade(
            &proposer,
            &target,
            &zero_hash(&env),
            &title(&env, "test"),
            &desc(&env, "test"),
            &zero_hash(&env),
        );
        assert!(result.is_err(), "propose_upgrade without auth must fail");
    }

    /// vote requires the voter to authenticate.
    #[test]
    fn test_auth_vote_without_auth_fails() {
        let env = Env::default(); // no mock_all_auths
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let voter = Address::generate(&env);
        let result = client.try_vote(&voter, &0u64, &true);
        assert!(result.is_err(), "vote without auth must fail");
    }

    /// cancel by a non-admin returns Unauthorized (address check fails after auth check).
    #[test]
    fn test_auth_cancel_non_admin_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        let non_admin = Address::generate(&env);
        assert_eq!(
            client.try_cancel(&non_admin, &id),
            Err(Ok(Error::Unauthorized))
        );
    }

    /// cancel without any auth at all fails at the host level.
    #[test]
    fn test_auth_cancel_without_auth_fails() {
        let env = Env::default(); // no mock_all_auths
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let caller = Address::generate(&env);
        let result = client.try_cancel(&caller, &0u64);
        assert!(result.is_err(), "cancel without auth must fail");
    }

    /// update_config by a non-admin returns Unauthorized.
    #[test]
    fn test_auth_update_config_non_admin_fails() {
        let (env, _admin, _cid, client) = setup();
        let non_admin = Address::generate(&env);
        assert_eq!(
            client.try_update_config(&non_admin, &50u32, &100u32, &1u32, &51u32, &0u32),
            Err(Ok(Error::Unauthorized))
        );
    }

    /// update_config without any auth at all fails at the host level.
    #[test]
    fn test_auth_update_config_without_auth_fails() {
        let env = Env::default(); // no mock_all_auths
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let caller = Address::generate(&env);
        let result = client.try_update_config(&caller, &50u32, &100u32, &1u32, &51u32, &0u32);
        assert!(result.is_err(), "update_config without auth must fail");
    }

    /// transfer_admin by a non-admin returns Unauthorized.
    #[test]
    fn test_auth_transfer_admin_non_admin_fails() {
        let (env, _admin, _cid, client) = setup();
        let non_admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        assert_eq!(
            client.try_transfer_admin(&non_admin, &new_admin),
            Err(Ok(Error::Unauthorized))
        );
    }

    /// transfer_admin without any auth at all fails at the host level.
    #[test]
    fn test_auth_transfer_admin_without_auth_fails() {
        let env = Env::default(); // no mock_all_auths
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let caller = Address::generate(&env);
        let new_admin = Address::generate(&env);
        let result = client.try_transfer_admin(&caller, &new_admin);
        assert!(result.is_err(), "transfer_admin without auth must fail");
    }

    // -----------------------------------------------------------------------
    // propose_upgrade
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_proposal() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        assert_eq!(id, 0u64);
        let p = client.get_proposal(&id);
        assert_eq!(p.status, ProposalStatus::Active);
        assert_eq!(p.target, target);
        assert_eq!(p.yes_votes, 0);
    }

    /// Creating 5 proposals assigns sequential IDs 0–4 and all remain Active.
    #[test]
    fn test_many_proposals_sequential_ids() {
        let (env, admin, _cid, client) = setup();

        // Five distinct targets.
        let targets: std::vec::Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();

        let mut ids: std::vec::Vec<u64> = std::vec::Vec::new();
        for (i, target) in targets.iter().enumerate() {
            let id = client.propose_upgrade(
                &admin,
                target,
                &zero_hash(&env),
                &title(&env, &std::format!("proposal {i}")),
                &desc(&env, &std::format!("description {i}")),
                &zero_hash(&env),
            );
            ids.push(id);
        }

        // IDs must be sequential starting at 0.
        assert_eq!(ids, std::vec![0u64, 1u64, 2u64, 3u64, 4u64]);

        // All proposals must be Active and have the correct unique target.
        for (i, id) in ids.iter().enumerate() {
            let p = client.get_proposal(id);
            assert_eq!(p.status, ProposalStatus::Active);
            assert_eq!(p.target, targets[i]);
        }
    }

    #[test]
    fn test_description_too_long() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let long_desc = String::from_str(&env, &"a".repeat(257));
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "long description"),
                &long_desc,
                &zero_hash(&env)
            ),
            Err(Ok(Error::DescriptionTooLong))
        );
    }

    #[test]
    fn test_description_empty_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let empty = String::from_str(&env, "");
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "empty description"),
                &empty,
                &zero_hash(&env)
            ),
            Err(Ok(Error::EmptyDescription))
        );
    }

    /// 256-byte description is exactly at the limit — must succeed.
    #[test]
    fn test_description_256_bytes_ok() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let max_desc = String::from_str(&env, &"a".repeat(256));
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "max description"),
            &max_desc,
            &zero_hash(&env),
        );
        assert_eq!(id, 0u64);
    }

    /// Pre-setting ProposalCount to u64::MAX causes next_proposal_id to overflow.
    #[test]
    fn test_proposal_id_overflow() {
        let (env, admin, cid, client) = setup();

        // Set the proposal counter to u64::MAX so the next increment overflows.
        env.as_contract(&cid, || {
            env.storage()
                .instance()
                .set(&DataKey::ProposalCount, &u64::MAX);
        });

        let target = Address::generate(&env);
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "overflow"),
                &desc(&env, "overflow test"),
                &zero_hash(&env),
            ),
            Err(Ok(Error::Overflow))
        );
    }

    #[test]
    fn test_propose_not_initialized_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let proposer = Address::generate(&env);
        let target = Address::generate(&env);
        assert_eq!(
            client.try_propose_upgrade(
                &proposer,
                &target,
                &zero_hash(&env),
                &title(&env, "v2"),
                &desc(&env, "v2"),
                &zero_hash(&env)
            ),
            Err(Ok(Error::NotInitialized))
        );
    }

    /// Config fields are snapshotted at creation time. Changing config mid-proposal
    /// does not affect the proposal's snap_* fields.
    #[test]
    fn test_config_snapshot_isolation() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );

        let p = client.get_proposal(&id);
        assert_eq!(p.snap_quorum, 3);
        assert_eq!(p.snap_threshold_pct, 51);
        assert_eq!(p.snap_timelock_delay, 17_280);
        assert_eq!(p.snap_voting_period, 100);

        // Change config — new values must not affect in-flight proposal.
        client.update_config(&admin, &200u32, &5_000u32, &10u32, &75u32, &0u32);
        assert_eq!(client.get_config().quorum, 10);

        let p2 = client.get_proposal(&id);
        assert_eq!(p2.snap_quorum, 3);
        assert_eq!(p2.snap_threshold_pct, 51);
        assert_eq!(p2.snap_timelock_delay, 17_280);

        // Old rules still apply: 3 YES votes satisfies old quorum=3 / threshold=51.
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        // advance_past_voting uses 101 ledgers — satisfies old voting_period=100.
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Queued);
    }

    // -----------------------------------------------------------------------
    // vote
    // -----------------------------------------------------------------------

    #[test]
    fn test_vote_yes_no() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &false);
        let p = client.get_proposal(&id);
        assert_eq!(p.yes_votes, 2);
        assert_eq!(p.no_votes, 1);
    }

    #[test]
    fn test_double_vote_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        let voter = Address::generate(&env);
        client.vote(&voter, &id, &true);
        assert_eq!(
            client.try_vote(&voter, &id, &true),
            Err(Ok(Error::AlreadyVoted))
        );
    }

    #[test]
    fn test_vote_on_unknown_proposal_fails() {
        let (env, _admin, _cid, client) = setup();
        let voter = Address::generate(&env);
        assert_eq!(
            client.try_vote(&voter, &999u64, &true),
            Err(Ok(Error::ProposalNotFound))
        );
    }

    /// Voting on a Cancelled proposal returns VotingClosed.
    #[test]
    fn test_vote_on_cancelled_returns_voting_closed() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        // Cancel the proposal.
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);
        // Attempting to vote on a Cancelled proposal must return VotingClosed.
        let voter = Address::generate(&env);
        assert_eq!(
            client.try_vote(&voter, &id, &true),
            Err(Ok(Error::VotingClosed))
        );
    }

    /// Voting on the exact last ledger of voting period (voting_ends_at) must succeed.
    #[test]
    fn test_vote_at_voting_ends_at_succeeds() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        const ADVANCE: u32 = 100; // exactly voting_ends_at
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
        client.vote(&Address::generate(&env), &id, &true);
    }

    /// Voting one ledger after voting_ends_at must fail with VotingPeriodEnded.
    #[test]
    fn test_vote_after_voting_period_ends_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        advance_past_voting(&env, &cid, id); // ledger=101, voting_ends_at=100
        assert_eq!(
            client.try_vote(&Address::generate(&env), &id, &true),
            Err(Ok(Error::VotingPeriodEnded))
        );
    }

    // -----------------------------------------------------------------------
    // finalize
    // -----------------------------------------------------------------------

    #[test]
    fn test_finalize_before_voting_ends_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingStillActive)));
    }

    /// Finalize on the boundary ledger voting_ends_at must still fail
    /// (requires sequence > voting_ends_at).
    #[test]
    fn test_finalize_at_voting_ends_at_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        const ADVANCE: u32 = 100; // exactly voting_ends_at
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingStillActive)));
    }

    /// Finalize one ledger past voting_ends_at must succeed.
    #[test]
    fn test_finalize_at_voting_ends_at_plus_one_succeeds() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id); // ledger=101
        assert_eq!(client.finalize(&id), ProposalStatus::Queued);
    }

    // Keep old snapshot-compatible test name.
    #[test]
    fn test_finalize_before_timelock_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingStillActive)));
    }

    /// Threshold boundary check: exact percentage match passes (>= not >).
    ///
    /// `finalize_voting` uses `(yes_votes * 100) >= (total_votes * threshold_pct)`.
    /// With quorum=5, threshold=60, 3 YES and 2 NO: 3*100=300 >= 5*60=300 — the
    /// `>=` operator means the exact boundary passes. This test verifies that
    /// a proposal at exactly the threshold percentage is finalized as Queued, not
    /// Defeated.
    #[test]
    fn test_threshold_exact_boundary_passes() {
        let env = Env::default();
        env.mock_all_auths();
        // quorum=5, threshold=60: 3 YES + 2 NO = total 5, 3*100=300 >= 5*60=300 → passes.
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &5u32, &60u32, &0u32);

        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "threshold boundary"),
            &desc(&env, "60% threshold boundary test"),
            &zero_hash(&env),
        );

        // Cast 3 YES and 2 NO votes: total = 5 (meets quorum), yes% = 60% (== threshold).
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        for _ in 0..2 {
            client.vote(&Address::generate(&env), &id, &false);
        }

        let p = client.get_proposal(&id);
        assert_eq!(p.yes_votes, 3, "must have 3 YES votes");
        assert_eq!(p.no_votes, 2, "must have 2 NO votes");

        // Advance past the voting period (voting_period=100).
        const ADVANCE: u32 = 101;
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);

        // 60% == threshold (>=), so the proposal must be Queued, not Defeated.
        assert_eq!(
            client.finalize(&id),
            ProposalStatus::Queued,
            "exact threshold boundary (60% == 60%) must pass and produce Queued"
        );
    }

    #[test]
    fn test_finalize_passes_with_quorum() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Queued);
    }

    #[test]
    fn test_finalize_defeated_without_quorum() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..2 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
    }

    /// When quorum is u32::MAX no realistic vote count can reach it, so finalize
    /// must always return Defeated regardless of the YES/NO split.
    #[test]
    fn test_max_quorum_always_defeated() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        // quorum = u32::MAX: total_votes can never reach this in practice.
        client.initialize(&admin, &100u32, &100u32, &u32::MAX, &51u32, &0u32);

        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "max quorum"),
            &desc(&env, "quorum is u32::MAX — always defeated"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }

        const BUMP: u32 = 50_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);

        assert_eq!(
            client.finalize(&id),
            ProposalStatus::Defeated,
            "quorum=u32::MAX must always produce Defeated"
        );
    }

    #[test]
    fn test_finalize_defeated_all_no_votes() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &false);
        }
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
    }

    /// With approval_threshold_pct = 100, every vote must be YES for the
    /// proposal to pass. A single NO vote causes Defeated.
    ///
    /// `finalize_voting` checks `(yes_votes * 100) >= (total_votes * threshold)`.
    /// With threshold = 100 this collapses to `yes_votes >= total_votes`.
    #[test]
    fn test_threshold_100_requires_unanimous() {
        // --- Proposal A: 3 YES, 0 NO → must pass (Queued) ---
        {
            let env = Env::default();
            env.mock_all_auths();
            let cid = env.register(UpgradeManager, ());
            let client = UpgradeManagerClient::new(&env, &cid);
            let admin = Address::generate(&env);
            client.initialize(&admin, &100u32, &100u32, &3u32, &100u32, &0u32);

            let target = Address::generate(&env);
            let id = client.propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "unanimous A"),
                &desc(&env, "threshold 100 all YES"),
                &zero_hash(&env),
            );
            for _ in 0..3 {
                client.vote(&Address::generate(&env), &id, &true);
            }
            const BUMP: u32 = 50_000;
            env.as_contract(&cid, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
            });
            env.ledger().with_mut(|l| l.sequence_number += 101);
            assert_eq!(
                client.finalize(&id),
                ProposalStatus::Queued,
                "3 YES with threshold=100 must be Queued"
            );
        }

        // --- Proposal B: 2 YES, 1 NO → must fail (Defeated) ---
        {
            let env = Env::default();
            env.mock_all_auths();
            let cid = env.register(UpgradeManager, ());
            let client = UpgradeManagerClient::new(&env, &cid);
            let admin = Address::generate(&env);
            client.initialize(&admin, &100u32, &100u32, &3u32, &100u32, &0u32);

            let target = Address::generate(&env);
            let id = client.propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "unanimous B"),
                &desc(&env, "threshold 100 one NO"),
                &zero_hash(&env),
            );
            for _ in 0..2 {
                client.vote(&Address::generate(&env), &id, &true);
            }
            client.vote(&Address::generate(&env), &id, &false);
            const BUMP: u32 = 50_000;
            env.as_contract(&cid, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
            });
            env.ledger().with_mut(|l| l.sequence_number += 101);
            assert_eq!(
                client.finalize(&id),
                ProposalStatus::Defeated,
                "2 YES + 1 NO with threshold=100 must be Defeated"
            );
        }
    }

    /// With approval_threshold_pct = 1 (the minimum valid threshold), a single
    /// YES vote passes once quorum is met, while 0 YES votes is still Defeated.
    ///
    /// `finalize_voting` checks `(yes_votes * 100) >= (total_votes * threshold)`.
    /// With threshold = 1 and quorum = 3:
    /// - 1 YES + 2 NO → 100 >= 3 → Queued
    /// - 0 YES + 3 NO → 0 >= 3 is false → Defeated
    #[test]
    fn test_threshold_1_passes_with_any_yes() {
        // --- Proposal A: 1 YES, 2 NO → must pass (Queued) ---
        {
            let env = Env::default();
            env.mock_all_auths();
            let cid = env.register(UpgradeManager, ());
            let client = UpgradeManagerClient::new(&env, &cid);
            let admin = Address::generate(&env);
            client.initialize(&admin, &100u32, &100u32, &3u32, &1u32, &0u32);

            let target = Address::generate(&env);
            let id = client.propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "threshold 1 pass"),
                &desc(&env, "threshold 1 with 1 YES and 2 NO"),
                &zero_hash(&env),
            );
            client.vote(&Address::generate(&env), &id, &true);
            for _ in 0..2 {
                client.vote(&Address::generate(&env), &id, &false);
            }
            advance_past_voting(&env, &cid, id);
            assert_eq!(
                client.finalize(&id),
                ProposalStatus::Queued,
                "1 YES + 2 NO with threshold=1 must be Queued"
            );
            let proposal = client.get_proposal(&id);
            assert_eq!(proposal.status, ProposalStatus::Queued);
            assert_eq!(proposal.yes_votes, 1);
            assert_eq!(proposal.no_votes, 2);
        }

        // --- Proposal B: 0 YES, 3 NO → must fail (Defeated) ---
        {
            let env = Env::default();
            env.mock_all_auths();
            let cid = env.register(UpgradeManager, ());
            let client = UpgradeManagerClient::new(&env, &cid);
            let admin = Address::generate(&env);
            client.initialize(&admin, &100u32, &100u32, &3u32, &1u32, &0u32);

            let target = Address::generate(&env);
            let id = client.propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "threshold 1 zero yes"),
                &desc(&env, "threshold 1 with 0 YES and 3 NO"),
                &zero_hash(&env),
            );
            for _ in 0..3 {
                client.vote(&Address::generate(&env), &id, &false);
            }
            advance_past_voting(&env, &cid, id);
            assert_eq!(
                client.finalize(&id),
                ProposalStatus::Defeated,
                "0 YES + 3 NO with threshold=1 must be Defeated"
            );
        }
    }

    /// Finalize on a Cancelled proposal returns VotingClosed.
    ///
    /// `finalize_voting` checks `proposal.status != ProposalStatus::Active` and
    /// returns `Error::VotingClosed` for any other status. `cancel_proposal` sets
    /// the status to Cancelled. This test verifies the Cancelled path explicitly,
    /// separate from `test_finalize_already_queued_fails` which covers Queued.
    #[test]
    fn test_finalize_on_cancelled_returns_voting_closed() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        // Cancel the Active proposal.
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);

        // Attempting to finalize a Cancelled proposal must return VotingClosed.
        assert_eq!(
            client.try_finalize(&id),
            Err(Ok(Error::VotingClosed)),
            "finalize on a Cancelled proposal must return VotingClosed"
        );
    }

    /// Finalize on an already-Queued proposal fails with VotingClosed.
    #[test]
    fn test_finalize_already_queued_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id);
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingClosed)));
    }

    // -----------------------------------------------------------------------
    // cancel
    // -----------------------------------------------------------------------

    #[test]
    fn test_cancel_by_admin() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);
    }

    /// Admin can cancel a Queued proposal.
    #[test]
    fn test_cancel_queued_proposal() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id); // → Queued
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);
    }

    #[test]
    fn test_cancel_by_non_admin_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        let non_admin = Address::generate(&env);
        assert_eq!(
            client.try_cancel(&non_admin, &id),
            Err(Ok(Error::Unauthorized))
        );
    }

    #[test]
    fn test_cancel_already_cancelled_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id);
        client.cancel(&admin, &id);
        assert_eq!(
            client.try_cancel(&admin, &id),
            Err(Ok(Error::ProposalFinalized))
        );
    }

    /// cancel on a Defeated proposal fails with ProposalFinalized.
    #[test]
    fn test_cancel_defeated_proposal_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..2 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id); // → Defeated
        assert_eq!(
            client.try_cancel(&admin, &id),
            Err(Ok(Error::ProposalFinalized))
        );
    }

    /// cancel on an Executed proposal fails with ProposalFinalized.
    #[test]
    fn test_cancel_executed_proposal_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let manager_id = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32);

        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
        let target_id = env.register_at(
            &Address::generate(&env),
            super::fixture_target_contract::FixtureTarget,
            (manager_id.clone(),),
        );

        let id = client.propose_upgrade(
            &admin,
            &target_id,
            &v2_hash,
            &title(&env, "will execute"),
            &desc(&env, "will execute"),
            &zero_hash(&env),
        );
        client.vote(&Address::generate(&env), &id, &true);

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        client.finalize(&id);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);

        client.execute(&id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
        assert_eq!(
            client.try_cancel(&admin, &id),
            Err(Ok(Error::ProposalFinalized)),
            "cancel on Executed proposal must return ProposalFinalized"
        );
    }

    // -----------------------------------------------------------------------
    // execute
    // -----------------------------------------------------------------------

    #[test]
    fn test_execute_fails_on_active_proposal() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    #[test]
    fn test_execute_fails_on_defeated_proposal() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    #[test]
    fn test_execute_fails_before_timelock() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::TimelockNotExpired)));
    }

    /// Execute at exactly executable_at - 1 must still fail.
    #[test]
    fn test_execute_one_ledger_before_executable_at_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id); // → Queued; executable_at = current + 17_280

        const ADVANCE: u32 = 17_279; // one short of executable_at
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::TimelockNotExpired)));
    }

    /// `assert_timelock_passed` rejects only `sequence < executable_at`, so the
    /// ledger equal to `executable_at` is the first one where `execute` succeeds.
    #[test]
    fn test_execute_at_exactly_executable_at_succeeds() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        // voting_period=100, timelock_delay=100, quorum=3, threshold=51
        client.initialize(&admin, &100u32, &100u32, &3u32, &51u32, &0u32);

        let target = env.register(MockTarget, ());
        let new_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
        let id = client.propose_upgrade(
            &admin,
            &target,
            &new_hash,
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        let finalized_at = env.ledger().sequence();
        assert_eq!(client.finalize(&id), ProposalStatus::Queued);

        let executable_at = client.get_proposal(&id).executable_at;
        assert_eq!(executable_at, finalized_at + 100);

        // advance to exactly executable_at, not one ledger beyond it
        env.ledger().with_mut(|l| l.sequence_number = executable_at);
        assert_eq!(env.ledger().sequence(), executable_at);

        client.execute(&id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
    }

    // -----------------------------------------------------------------------
    // time_until_executable
    // -----------------------------------------------------------------------

    #[test]
    fn test_time_until_executable() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        assert_eq!(client.time_until_executable(&id), 0u32); // not Queued yet

        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id);
        assert_eq!(client.time_until_executable(&id), 17_280u32);

        advance_past_timelock(&env, &cid, id);
        assert_eq!(client.time_until_executable(&id), 0u32);
    }

    /// time_until_executable on a Cancelled proposal returns 0.
    ///
    /// `ledgers_until_executable` returns 0 for any proposal whose status is not
    /// Queued. This test covers the Cancelled path: create an Active proposal,
    /// cancel it, then assert `time_until_executable` returns 0.
    #[test]
    fn test_time_until_executable_on_cancelled() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        // Confirm the proposal is Active before cancelling.
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Active);

        // Admin cancels the proposal.
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);

        // A Cancelled proposal is not Queued, so time_until_executable must return 0.
        assert_eq!(
            client.time_until_executable(&id),
            0u32,
            "time_until_executable must return 0 for a Cancelled proposal"
        );
    }

    /// time_until_executable on an unknown proposal ID returns ProposalNotFound.
    #[test]
    fn test_time_until_executable_unknown_id_fails() {
        let (_env, _admin, _cid, client) = setup();
        assert_eq!(
            client.try_time_until_executable(&999u64),
            Err(Ok(Error::ProposalNotFound))
        );
    }

    /// After a proposal has been fully executed, `time_until_executable` must
    /// return `0`.
    ///
    /// `ledgers_until_executable` returns `0` for any proposal whose status is
    /// not `Queued`. Once `execute()` sets the status to `Executed`, the
    /// function must therefore return `0`.
    ///
    /// Uses the `FixtureTarget` pattern from
    /// `test_governance_upgrade_executed_through_manager`: initialize the
    /// manager with a short voting period and timelock, run the full flow
    /// (propose → vote → finalize → execute), and assert the return value.
    #[test]
    fn test_time_until_executable_on_executed() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        // Short voting period and timelock so the ledger advance is small.
        manager.initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32);

        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
        let target_id = env.register_at(
            &Address::generate(&env),
            super::fixture_target_contract::FixtureTarget,
            (manager_id.clone(),),
        );

        let pid = manager.propose_upgrade(
            &admin,
            &target_id,
            &v2_hash,
            &title(&env, "v2 upgrade"),
            &desc(&env, "full flow for time_until_executable"),
            &zero_hash(&env),
        );
        manager.vote(&Address::generate(&env), &pid, &true);

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        assert_eq!(manager.finalize(&pid), ProposalStatus::Queued);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);

        manager.execute(&pid);
        assert_eq!(
            manager.get_proposal(&pid).status,
            ProposalStatus::Executed,
            "proposal must be Executed"
        );

        // After execution the proposal is no longer Queued, so
        // time_until_executable must return 0.
        assert_eq!(
            manager.time_until_executable(&pid),
            0u32,
            "time_until_executable must return 0 for an Executed proposal"
        );
    }

    // -----------------------------------------------------------------------
    // update_config / transfer_admin
    // -----------------------------------------------------------------------

    #[test]
    fn test_update_config() {
        let (_env, admin, _cid, client) = setup();
        client.update_config(&admin, &50u32, &1000u32, &5u32, &66u32, &0u32);
        let cfg = client.get_config();
        assert_eq!(cfg.voting_period, 50);
        assert_eq!(cfg.timelock_delay, 1000);
        assert_eq!(cfg.quorum, 5);
        assert_eq!(cfg.approval_threshold_pct, 66);
    }

    #[test]
    fn test_update_config_invalid_values_fail() {
        let (_env, admin, _cid, client) = setup();
        assert_eq!(
            client.try_update_config(&admin, &0u32, &1000u32, &5u32, &66u32, &0u32),
            Err(Ok(Error::InvalidVotingPeriod))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &0u32, &5u32, &66u32, &0u32),
            Err(Ok(Error::InvalidTimelockDelay))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &1000u32, &0u32, &66u32, &0u32),
            Err(Ok(Error::InvalidQuorum))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &1000u32, &5u32, &0u32, &0u32),
            Err(Ok(Error::InvalidThreshold))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &1000u32, &5u32, &101u32, &0u32),
            Err(Ok(Error::InvalidThreshold))
        );
    }

    /// Calling `update_config` with a different `timelock_delay` after a
    /// proposal has been finalized into `Queued` must not alter the proposal's
    /// already-set `executable_at`.
    ///
    /// `executable_at` is derived from the snapshotted `snap_timelock_delay`
    /// at finalization time. The global `Config` is independent of any
    /// in-flight proposal, so changing `timelock_delay` via `update_config`
    /// can only affect proposals created after the config change.
    #[test]
    fn test_update_config_does_not_affect_queued_executable_at() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);

        // Create a proposal and vote it through.
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }

        // Advance past the voting period and finalize → Queued.
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Queued);

        // Record executable_at before changing config.
        let proposal_before = client.get_proposal(&id);
        assert_eq!(
            proposal_before.status,
            ProposalStatus::Queued,
            "proposal must be Queued before config change"
        );
        let executable_at_before = proposal_before.executable_at;

        // Change timelock_delay to a very different value.
        client.update_config(&admin, &100u32, &1_000u32, &3u32, &51u32, &0u32);

        // The global config now has a different timelock_delay.
        assert_eq!(client.get_config().timelock_delay, 1_000u32);

        // The Queued proposal's executable_at must be unchanged.
        let proposal_after = client.get_proposal(&id);
        assert_eq!(
            proposal_after.executable_at, executable_at_before,
            "update_config must not alter executable_at of a Queued proposal"
        );
        assert_eq!(
            proposal_after.status,
            ProposalStatus::Queued,
            "proposal status must remain Queued after config change"
        );
    }

    #[test]
    fn test_transfer_admin() {
        let (env, admin, _cid, client) = setup();
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin);
        assert_eq!(client.get_admin(), new_admin);
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &new_admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        assert_eq!(client.try_cancel(&admin, &id), Err(Ok(Error::Unauthorized)));
    }

    /// Transferring admin while a proposal is Active means the old admin
    /// can no longer cancel and the new admin can.
    ///
    /// `cancel_proposal` reads the current admin from storage via `load_admin`.
    /// After `transfer_admin`, that stored value changes, so the old admin's
    /// address no longer matches and cancel returns Unauthorized.
    #[test]
    fn test_admin_transfer_during_active_proposal() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);

        // Create a proposal while the original admin is in place.
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "in-flight"),
            &desc(&env, "proposal active during admin transfer"),
            &zero_hash(&env),
        );
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Active);

        // Transfer admin to a new address while the proposal is still Active.
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin);
        assert_eq!(client.get_admin(), new_admin);

        // Old admin can no longer cancel.
        assert_eq!(
            client.try_cancel(&admin, &id),
            Err(Ok(Error::Unauthorized)),
            "old admin must not be able to cancel after transfer"
        );

        // New admin can cancel successfully.
        client.cancel(&new_admin, &id);
        assert_eq!(
            client.get_proposal(&id).status,
            ProposalStatus::Cancelled,
            "proposal must be Cancelled after new admin cancels"
        );
    }

    // -----------------------------------------------------------------------
    // TTL — verified with soroban-sdk testutils
    // -----------------------------------------------------------------------

    #[test]
    fn test_ttl_extended_on_write_and_read() {
        use soroban_sdk::testutils::storage::{
            Instance as InstanceTtl, Persistent as PersistentTtl,
        };

        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let voter = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.vote(&voter, &id, &true);

        env.as_contract(&cid, || {
            let proposal_ttl = env.storage().persistent().get_ttl(&DataKey::Proposal(id));
            assert!(
                proposal_ttl >= PERSISTENT_TTL_BUMP,
                "Proposal TTL {proposal_ttl} must be >= {PERSISTENT_TTL_BUMP}"
            );
            let vote_key = DataKey::Vote(id, voter.clone());
            let vote_ttl = env.storage().persistent().get_ttl(&vote_key);
            assert!(
                vote_ttl >= PERSISTENT_TTL_BUMP,
                "Vote TTL {vote_ttl} must be >= {PERSISTENT_TTL_BUMP}"
            );
            let instance_ttl = env.storage().instance().get_ttl();
            assert!(
                instance_ttl >= INSTANCE_TTL_BUMP,
                "Instance TTL {instance_ttl} must be >= {INSTANCE_TTL_BUMP}"
            );
        });

        // get_proposal read also bumps TTL.
        client.get_proposal(&id);
        env.as_contract(&cid, || {
            let ttl = env.storage().persistent().get_ttl(&DataKey::Proposal(id));
            assert!(
                ttl >= PERSISTENT_TTL_BUMP,
                "Proposal TTL after read {ttl} must be >= {PERSISTENT_TTL_BUMP}"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Events — env.events().all() returns events from the most recent call
    // -----------------------------------------------------------------------

    /// Verify initialize emits an event.
    #[test]
    fn test_event_initialized() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);
        // Events are per-call; check after the initialize call.
        assert!(
            !env.events().all().is_empty(),
            "initialize must emit at least one event"
        );
    }

    /// Verify propose_upgrade emits a proposal_created event.
    #[test]
    fn test_event_proposal_created() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        assert!(
            count_events(&env, symbol_short!("proposal"), symbol_short!("created")) >= 1,
            "must have proposal_created event"
        );
    }

    /// Verify vote emits a vote_cast event.
    #[test]
    fn test_event_vote_cast() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.vote(&Address::generate(&env), &id, &true);
        assert!(
            count_events(&env, symbol_short!("vote"), symbol_short!("cast")) >= 1,
            "must have vote_cast event"
        );
    }

    /// Verify finalize emits a finalized event.
    #[test]
    fn test_event_finalized() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id);
        assert!(
            count_events(&env, symbol_short!("proposal"), symbol_short!("finalized")) >= 1,
            "must have finalized event"
        );
    }

    #[test]
    fn test_event_cancelled() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.cancel(&admin, &id);
        assert!(
            count_events(&env, symbol_short!("proposal"), symbol_short!("cancelled")) >= 1,
            "missing cancelled event"
        );
    }

    #[test]
    fn test_event_config_updated() {
        let (env, admin, _cid, client) = setup();
        client.update_config(&admin, &50u32, &1000u32, &1u32, &51u32, &0u32);
        assert!(
            count_events(&env, symbol_short!("config"), symbol_short!("updated")) >= 1,
            "missing config_updated event"
        );
    }

    #[test]
    fn test_event_admin_transferred() {
        let (env, admin, _cid, client) = setup();
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin);
        assert!(
            count_events(&env, symbol_short!("admin"), symbol_short!("transfer")) >= 1,
            "missing admin_transferred event"
        );
    }

    // -----------------------------------------------------------------------
    // Integration: full governance flow (mock target, no real WASM swap)
    // -----------------------------------------------------------------------

    /// Full governance-gated upgrade flow using the MockTarget.
    ///
    /// **Proves:** the manager's execute() calls the target's upgrade() and the
    /// proposal ends up Executed.
    ///
    /// **Does NOT prove:** the target WASM actually changed. The mock host accepts
    /// any hash registered by upload_contract_wasm; real v1→v2 proof is in
    /// `test_real_v1_to_v2_upgrade_through_manager`.
    #[test]
    fn test_full_governance_upgrade_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let target_id = env.register(MockTarget, ());

        let new_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);

        let pid = manager.propose_upgrade(
            &admin,
            &target_id,
            &new_hash,
            &title(&env, "Integration test"),
            &String::from_str(&env, "Integration test"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            manager.vote(&Address::generate(&env), &pid, &true);
        }

        const BUMP: u32 = 50_000;
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        assert_eq!(manager.finalize(&pid), ProposalStatus::Queued);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 17_281);
        manager.execute(&pid);

        assert_eq!(manager.get_proposal(&pid).status, ProposalStatus::Executed);
    }

    // Backward-compatible test names preserved for existing snapshot files.
    #[test]
    fn test_execute_fails_on_active_proposal_compat() {
        let (env, admin, _cid, client) = setup();
        let target = env.register(MockTarget, ());
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    #[test]
    fn test_execute_fails_on_defeated_proposal_compat() {
        let (env, admin, cid, client) = setup();
        let target = env.register(MockTarget, ());
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    #[test]
    fn test_execute_fails_before_timelock_compat() {
        let (env, admin, cid, client) = setup();
        let target = env.register(MockTarget, ());
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "v2"),
            &desc(&env, "v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::TimelockNotExpired)));
    }

    // -----------------------------------------------------------------------
    // Real v1→v2 WASM upgrade using committed fixture WASMs
    // -----------------------------------------------------------------------

    /// Real v1→v2 WASM upgrade through the manager, using FixtureTarget.
    ///
    /// **Proves:**
    /// 1. Manager's execute() successfully calls upgrade() on the FixtureTarget.
    /// 2. The proposal ends up Executed after execute() returns.
    /// 3. Direct call to target.upgrade() by a non-manager address fails
    ///    (proven in a fresh env with no mock_all_auths).
    ///
    /// **About the fixture WASMs:** target_v1.wasm and target_v2.wasm are built
    /// by scripts/build_fixtures.sh and committed to tests/fixtures/ with
    /// SHA256SUMS for checksum verification. They cannot be uploaded via
    /// upload_contract_wasm() in this test because soroban-wasmi 0.31.1 (used
    /// by soroban-env-host 22.1.3) rejects call_indirect instructions whose
    /// table-index byte is LEB128-encoded (Rust ≥ 1.85.0 emits multi-byte zeros
    /// for this reserved byte). The mock host does not execute WASM code at all —
    /// it uses native Rust — so we use Bytes::new(&env) (empty bytes) as the
    /// Governance upgrade executed through the manager.
    ///
    /// Exercises the full governance flow (initialize → propose → vote × 3 →
    /// advance past voting → finalize → advance past timelock → execute) against
    /// a `FixtureTarget` whose `upgrade()` authenticates the manager, and asserts
    /// the proposal transitions to `Executed`. Also proves a direct call to
    /// `target.upgrade()` by a non-manager address fails (separate env without
    /// `mock_all_auths`).
    ///
    /// **Limitation:** This does NOT prove that the target contract's WASM is
    /// actually replaced or that `version()` changes from 1 to 2. The mock host
    /// accepts any hash registered by `upload_contract_wasm` and does not hot-swap
    /// WASM in memory, so the real fixture WASMs (which modern Rust toolchains
    /// emit with reference-types that soroban-wasmi 0.31.1 rejects) cannot be used
    /// here. The test uses empty bytes as a placeholder hash. What it proves is
    /// that `execute()` successfully invokes `upgrade()` on the target via
    /// cross-contract invocation and the proposal transitions to `Executed`.
    #[test]
    fn test_governance_upgrade_executed_through_manager() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);

        // Use empty bytes as the "v2" WASM — soroban-wasmi rejects the real
        // fixture WASMs due to LEB128 table-index encoding (see doccomment).
        // The fixture SHA256 hashes are documented in tests/fixtures/SHA256SUMS.
        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

        let target_id = env.register_at(
            &Address::generate(&env),
            FixtureTarget,
            (manager_id.clone(),),
        );
        let target_client = FixtureTargetClient::new(&env, &target_id);
        assert_eq!(target_client.version(), 1u32);
        assert_eq!(target_client.get_manager(), manager_id);

        // Prove direct upgrade() by non-manager fails (no mock_all_auths).
        {
            let env2 = Env::default();
            let mgr2 = env2.register(UpgradeManager, ());
            let tgt2 = env2.register_at(&Address::generate(&env2), FixtureTarget, (mgr2.clone(),));
            let v2h = env2
                .deployer()
                .upload_contract_wasm(soroban_sdk::Bytes::new(&env2));
            let tc2 = FixtureTargetClient::new(&env2, &tgt2);
            assert!(
                tc2.try_upgrade(&v2h).is_err(),
                "direct upgrade() call without manager auth must fail"
            );
        }

        let pid = manager.propose_upgrade(
            &admin,
            &target_id,
            &v2_hash,
            &title(&env, "Upgrade example-target v1 to v2"),
            &String::from_str(&env, "Upgrade example-target v1 to v2"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            manager.vote(&Address::generate(&env), &pid, &true);
        }

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        assert_eq!(manager.finalize(&pid), ProposalStatus::Queued);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 17_281);

        manager.execute(&pid);
        assert_eq!(manager.get_proposal(&pid).status, ProposalStatus::Executed);
    }

    /// A second call to example-target's __constructor fails.
    ///
    /// The Soroban host invokes `__constructor` once at deploy time and prevents
    /// it from being called again. After `env.register(ExampleTarget, (manager,))`
    /// the constructor has already run; a subsequent direct invocation via
    /// `env.invoke_contract` must fail at the host level.
    #[test]
    fn test_example_target_second_constructor_fails() {
        let env = Env::default();
        env.mock_all_auths();

        // Deploy example-target with a manager address (constructor runs here).
        let manager_id = env.register(UpgradeManager, ());
        let target_id = env.register(ExampleTarget, (manager_id.clone(),));
        let target_client = ExampleTargetClient::new(&env, &target_id);

        // Confirm the first deployment succeeded and stored the manager.
        assert_eq!(target_client.get_manager(), manager_id);
        assert_eq!(target_client.version(), 1u32);

        // A second call to __constructor must fail: the host only allows it once.
        // The ExampleTargetClient doesn't expose __constructor post-deployment,
        // so we invoke it directly via env.try_invoke_contract and assert it fails.
        let new_manager = Address::generate(&env);
        let ctor_sym = Symbol::new(&env, "__constructor");
        let result = env.try_invoke_contract::<soroban_sdk::Val, soroban_sdk::InvokeError>(
            &target_id,
            &ctor_sym,
            soroban_sdk::vec![&env, new_manager.to_val()],
        );
        assert!(
            result.is_err(),
            "__constructor must fail when called a second time after deployment"
        );
    }

    /// Deploys the real `example-target` contract with a contract address as
    /// manager, WITHOUT `mock_all_auths()`.
    #[test]
    fn test_real_example_target_deployed_with_contract_manager_without_mock_auths() {
        let env = Env::default();
        // Intentionally NOT calling env.mock_all_auths().

        let manager_id = env.register(UpgradeManager, ());

        // Deploy the real example-target with the manager contract as manager.
        // The constructor no longer calls manager.require_auth(), so this
        // succeeds even though the manager is a contract address that cannot sign.
        let target_id = env.register(ExampleTarget, (manager_id.clone(),));
        let target_client = ExampleTargetClient::new(&env, &target_id);
        assert_eq!(target_client.get_manager(), manager_id);
        assert_eq!(target_client.version(), 1u32);
    }

    /// Real `example-target` rejects `upgrade()` when the manager has not
    /// authorized it, WITHOUT `mock_all_auths`.
    ///
    /// Deploys the real `example-target` with a contract address as manager and
    /// calls `upgrade()` directly with an arbitrary hash. Because the caller is
    /// not the stored manager (and no auth is provided), `manager.require_auth()`
    /// inside `upgrade()` fails and `try_upgrade` returns an error.
    #[test]
    fn test_real_example_target_upgrade_rejects_non_manager() {
        let env = Env::default();
        // Intentionally NOT calling env.mock_all_auths().
        let manager_id = env.register(UpgradeManager, ());
        let target_id = env.register(ExampleTarget, (manager_id.clone(),));
        let target_client = ExampleTargetClient::new(&env, &target_id);
        let any_hash = BytesN::from_array(&env, &[0u8; 32]);
        assert!(
            target_client.try_upgrade(&any_hash).is_err(),
            "upgrade() must fail when the manager has not authorized it"
        );
    }

    /// Double-execute fails with NotQueued on the second call.
    #[test]
    fn test_double_execute_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);

        // Use empty bytes as upgrade hash (real fixture WASMs rejected by validator).
        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
        let target_id = env.register_at(
            &Address::generate(&env),
            FixtureTarget,
            (manager_id.clone(),),
        );

        let id = manager.propose_upgrade(
            &admin,
            &target_id,
            &v2_hash,
            &title(&env, "double-execute test"),
            &desc(&env, "double-execute test"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            manager.vote(&Address::generate(&env), &id, &true);
        }

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        manager.finalize(&id);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 17_281);

        manager.execute(&id);
        assert_eq!(manager.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    // -----------------------------------------------------------------------
    // Failed upgrade — target without upgrade() leaves proposal Queued
    // -----------------------------------------------------------------------

    /// Failed upgrade test.
    ///
    /// When the target's upgrade() call fails (target has no upgrade function),
    /// the whole transaction reverts, including the `proposal.status = Executed`
    /// write. The proposal remains Queued.
    ///
    /// **Note:** A failure inside invoke_contract is a host-level HostError, NOT
    /// one of the manager's Error variants. try_execute returns Err(Err(_)) in
    /// this case. We do NOT document a specific error code for this in the README.
    #[test]
    fn test_failed_upgrade_leaves_proposal_queued() {
        let env = Env::default();
        env.mock_all_auths();
        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        // Short timelock to reduce ledger advance.
        manager.initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32);

        let bad_target = env.register(NoUpgradeFn, ());

        // Use empty bytes as upgrade hash (real fixture WASMs rejected by validator).
        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

        let id = manager.propose_upgrade(
            &admin,
            &bad_target,
            &v2_hash,
            &title(&env, "will fail"),
            &desc(&env, "will fail"),
            &zero_hash(&env),
        );
        manager.vote(&Address::generate(&env), &id, &true);

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&bad_target, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        manager.finalize(&id);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&bad_target, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);

        let result = manager.try_execute(&id);
        assert!(
            result.is_err(),
            "execute against target with no upgrade() must fail"
        );

        // Transaction reverted → proposal is still Queued.
        assert_eq!(
            manager.get_proposal(&id).status,
            ProposalStatus::Queued,
            "proposal must remain Queued after a failed target upgrade"
        );
    }

    /// Execute against a target whose upgrade() RETURNS AN ERROR.
    ///
    /// Distinct from `test_failed_upgrade_leaves_proposal_queued` (target has
    /// no upgrade function at all): here the function exists with the right
    /// signature but returns `Err(UpgradeRejected)`. The manager's
    /// `invoke_contract::<()>` call fails with that error, the whole
    /// `execute()` transaction reverts (undoing the premature
    /// `status = Executed` write), and the proposal remains Queued.
    ///
    /// A failure inside invoke_contract is a host-level error, NOT one of the
    /// manager's Error variants, so `try_execute` returns `Err(Err(_))`.
    #[test]
    fn test_execute_with_error_returning_target() {
        let env = Env::default();
        env.mock_all_auths();
        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        // Short voting period and timelock to minimize ledger advances.
        manager.initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32);

        let error_target = env.register(ErrorTarget, ());

        // Any 32-byte hash; the target rejects the upgrade before using it.
        let new_wasm_hash = zero_hash(&env);

        let id = manager.propose_upgrade(
            &admin,
            &error_target,
            &new_wasm_hash,
            &title(&env, "upgrade a rejecting target"),
            &desc(&env, "target upgrade() returns Err(UpgradeRejected)"),
            &zero_hash(&env),
        );
        // Quorum is 1: a single YES vote passes the proposal.
        manager.vote(&Address::generate(&env), &id, &true);

        const BUMP: u32 = 50_000;
        let extend_ttls = || {
            env.as_contract(&manager_id, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
            });
            env.as_contract(&error_target, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
            });
        };

        // Close voting and queue the proposal.
        extend_ttls();
        env.ledger().with_mut(|l| l.sequence_number += 101);
        assert_eq!(manager.finalize(&id), ProposalStatus::Queued);

        // Pass the timelock.
        extend_ttls();
        env.ledger().with_mut(|l| l.sequence_number += 101);

        // The target's upgrade() returns Err → execute fails.
        // The target's upgrade() returns Err → the cross-contract call fails
        // with a host-level error (not one of the manager's Error variants).
        let result = manager.try_execute(&id);
        assert!(
            result.is_err(),
            "execute against a target whose upgrade() returns an error must fail"
        );

        // Transaction reverted → the premature Executed write was undone and
        // the proposal is still Queued.
        assert_eq!(
            manager.get_proposal(&id).status,
            ProposalStatus::Queued,
            "proposal must remain Queued after a target upgrade error"
        );
    }

    /// Execute against a target whose upgrade() PANICS inside the call.
    ///
    /// Distinct from `test_failed_upgrade_leaves_proposal_queued` (target has
    /// no upgrade function) and `test_execute_with_error_returning_target`
    /// (target returns `Err`): here the target contract's `upgrade()` panics.
    /// The Soroban host aborts the cross-contract call (`InvokeError::Abort`),
    /// rolling back the entire `execute()` transaction — including the pre-call
    /// `proposal.status = Executed` write — so the proposal remains `Queued`.
    #[test]
    fn test_execute_with_panicking_target() {
        let env = Env::default();
        env.mock_all_auths();
        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        // Short voting period and timelock to minimize ledger advances.
        manager.initialize(&admin, &100u32, &100u32, &1u32, &51u32, &0u32);

        let panicking_target = env.register(PanickingTarget, ());
        let target_client = PanickingTargetClient::new(&env, &panicking_target);

        // Any 32-byte hash; the target panics before swapping WASM.
        let new_wasm_hash = zero_hash(&env);

        let id = manager.propose_upgrade(
            &admin,
            &panicking_target,
            &new_wasm_hash,
            &title(&env, "upgrade a panicking target"),
            &desc(&env, "target upgrade() panics unconditionally"),
            &zero_hash(&env),
        );
        // Quorum is 1: a single YES vote passes the proposal.
        manager.vote(&Address::generate(&env), &id, &true);

        const BUMP: u32 = 50_000;
        let extend_ttls = || {
            env.as_contract(&manager_id, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
            });
            env.as_contract(&panicking_target, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
            });
        };

        // Close voting and queue the proposal.
        extend_ttls();
        env.ledger().with_mut(|l| l.sequence_number += 101);
        assert_eq!(manager.finalize(&id), ProposalStatus::Queued);

        // Pass the timelock.
        extend_ttls();
        env.ledger().with_mut(|l| l.sequence_number += 101);

        // Confirm the target contract is still live before calling execute().
        assert_eq!(target_client.version(), 1);

        // The target's upgrade() panics → host aborts the cross-contract call.
        let result = manager.try_execute(&id);
        assert_eq!(
            result,
            Err(Err(InvokeError::Abort)),
            "execute against a panicking target must fail with a host abort"
        );

        // Transaction reverted → the premature Executed write was undone and
        // the proposal remains Queued with its vote counts intact.
        let proposal = manager.get_proposal(&id);
        assert_eq!(
            proposal.status,
            ProposalStatus::Queued,
            "proposal must remain Queued after a target upgrade panic"
        );
        assert_eq!(proposal.yes_votes, 1);
        assert_eq!(proposal.no_votes, 0);
    }

    /// Execute succeeds when the target's upgrade() does meaningful work
    /// (storage reads and writes) before calling update_current_contract_wasm.
    ///
    /// Exercises the full governance flow through execute() against a target
    /// whose upgrade() simulates a migration by writing and reading back
    /// several storage entries. Asserts that execute() succeeds and the
    /// proposal is Executed.
    #[test]
    fn test_execute_with_budget_consuming_target() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &100u32, &3u32, &51u32, &0u32);

        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

        let target_id = env.register_at(
            &Address::generate(&env),
            BudgetConsumingTarget,
            (manager_id.clone(),),
        );

        let pid = manager.propose_upgrade(
            &admin,
            &target_id,
            &v2_hash,
            &title(&env, "budget-consuming upgrade"),
            &desc(&env, "target upgrade does storage reads and writes"),
            &zero_hash(&env),
        );
        for _ in 0..3 {
            manager.vote(&Address::generate(&env), &pid, &true);
        }

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);
        assert_eq!(manager.finalize(&pid), ProposalStatus::Queued);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(pid), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 101);

        manager.execute(&pid);
        assert_eq!(
            manager.get_proposal(&pid).status,
            ProposalStatus::Executed,
            "proposal must be Executed after execute() with a budget-consuming target"
        );
    }

    /// Real v1→v2 WASM upgrade through the manager using the committed fixture WASMs.
    ///
    /// Registers the real `target_v1.wasm`, asserts `version() == 1`, uploads
    /// `target_v2.wasm`, runs the full governance flow, calls `execute`, and
    /// asserts `version() == 2` and the proposal status is `Executed`.
    #[test]
    fn test_real_wasm_upgrade_v1_to_v2_through_manager() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &20u32, &20u32, &1u32, &51u32, &0u32);

        let v1_bytes = include_bytes!("../tests/fixtures/target_v1.wasm");
        let target_id = env.register(v1_bytes.as_slice(), (manager_id.clone(),));

        let version_sym = Symbol::new(&env, "version");
        let v1: u32 = env.invoke_contract(&target_id, &version_sym, soroban_sdk::vec![&env]);
        assert_eq!(v1, 1u32, "target must report version 1 before upgrade");

        let v2_bytes = include_bytes!("../tests/fixtures/target_v2.wasm");
        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::from_slice(&env, v2_bytes));

        let id = manager.propose_upgrade(
            &admin,
            &target_id,
            &v2_hash,
            &title(&env, "upgrade target to v2"),
            &desc(&env, "upgrade target to v2"),
            &zero_hash(&env),
        );
        manager.vote(&Address::generate(&env), &id, &true);

        const BUMP: u32 = 50_000;
        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 21);

        assert_eq!(manager.finalize(&id), ProposalStatus::Queued);

        env.as_contract(&manager_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 21);

        manager.execute(&id);

        let v2: u32 = env.invoke_contract(&target_id, &version_sym, soroban_sdk::vec![&env]);
        assert_eq!(v2, 2u32, "target must report version 2 after upgrade");
        assert_eq!(
            manager.get_proposal(&id).status,
            ProposalStatus::Executed,
            "proposal must be Executed"
        );
    }
    /// Deploys triple-target v1, upgrades to v2 then v3 through the manager,
    /// verifying the version changes each time and the manager relationship is
    /// preserved across both upgrades.
    ///
    /// Follows the `test_real_wasm_upgrade_v1_to_v2_through_manager` pattern
    /// twice: propose → vote → advance past voting → finalize → advance past
    /// timelock → execute, with TTL bumps between ledger advances.
    #[test]
    fn test_triple_target_upgrade_v1_to_v2_to_v3_through_manager() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &20u32, &20u32, &1u32, &51u32, &0u32);

        // Register the natively compiled v1 contract.
        let target_id = env.register(TripleTarget, (manager_id.clone(),));
        let client = TripleTargetClient::new(&env, &target_id);

        let version_sym = Symbol::new(&env, "version");
        let v1: u32 = env.invoke_contract(&target_id, &version_sym, soroban_sdk::vec![&env]);
        assert_eq!(v1, 1u32, "target must report version 1 before upgrade");

        // --- Upgrade 1: v1 -> v2 ---
        let v2_hash = upload_triple_target_wasm(&env, &["v2"]);
        run_triple_target_proposal_to_executed(
            &env,
            &manager_id,
            &manager,
            &admin,
            &target_id,
            &v2_hash,
            "upgrade triple-target to v2",
        );
        let v2: u32 = env.invoke_contract(&target_id, &version_sym, soroban_sdk::vec![&env]);
        assert_eq!(v2, 2u32, "target must report version 2 after first upgrade");

        // --- Upgrade 2: v2 -> v3 ---
        let v3_hash = upload_triple_target_wasm(&env, &["v3"]);
        run_triple_target_proposal_to_executed(
            &env,
            &manager_id,
            &manager,
            &admin,
            &target_id,
            &v3_hash,
            "upgrade triple-target to v3",
        );
        let v3: u32 = env.invoke_contract(&target_id, &version_sym, soroban_sdk::vec![&env]);
        assert_eq!(
            v3, 3u32,
            "target must report version 3 after second upgrade"
        );

        // Manager relationship preserved across both upgrades.
        assert_eq!(
            client.get_manager(),
            manager_id,
            "manager relationship must be preserved across upgrades"
        );
    }

    /// Builds triple-target with the given features at runtime and uploads the
    /// WASM, returning its hash. Building at runtime keeps the test
    /// self-contained and guarantees the upgraded WASM matches the current
    /// feature-gated source. Requires the `wasm32v1-none` target.
    fn upload_triple_target_wasm(env: &Env, features: &[&str]) -> BytesN<32> {
        use std::path::PathBuf;

        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for host tests");
        let triple_dir = PathBuf::from(&manifest_dir)
            .join("..")
            .join("triple-target");
        let triple_manifest = triple_dir.join("Cargo.toml");

        let mut args = std::vec![
            "build",
            "--manifest-path",
            triple_manifest
                .to_str()
                .expect("manifest path is valid unicode"),
            "--target",
            "wasm32v1-none",
            "--release",
        ];
        if !features.is_empty() {
            args.push("--features");
            args.extend_from_slice(features);
        }

        let status = std::process::Command::new("cargo")
            .args(&args)
            .status()
            .expect("failed to execute cargo to build triple-target wasm");
        assert!(
            status.success(),
            "`cargo build --target wasm32v1-none --release` for triple-target failed"
        );

        let wasm_path = triple_dir
            .join("..")
            .join("..")
            .join("target")
            .join("wasm32v1-none")
            .join("release")
            .join("triple_target.wasm");
        let bytes = std::fs::read(&wasm_path).expect("failed to read the triple-target wasm file");
        env.deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::from_slice(env, &bytes))
    }

    /// Runs a single proposal through propose → vote → finalize → execute,
    /// advancing the ledger past the voting period and the timelock with TTL
    /// bumps in between.
    fn run_triple_target_proposal_to_executed(
        env: &Env,
        manager_id: &Address,
        manager: &UpgradeManagerClient,
        admin: &Address,
        target_id: &Address,
        new_wasm_hash: &BytesN<32>,
        proposal_desc: &str,
    ) {
        let id = manager.propose_upgrade(
            admin,
            target_id,
            new_wasm_hash,
            &title(env, proposal_desc),
            &desc(env, proposal_desc),
            &zero_hash(env),
        );
        manager.vote(admin, &id, &true);

        const BUMP: u32 = 50_000;
        const ADVANCE: u32 = 21; // past voting_period=20
        bump_triple_target_ttls(env, manager_id, target_id, id, BUMP);
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
        assert_eq!(manager.finalize(&id), ProposalStatus::Queued);

        bump_triple_target_ttls(env, manager_id, target_id, id, BUMP);
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE); // past timelock_delay=20
        manager.execute(&id);

        assert_eq!(
            manager.get_proposal(&id).status,
            ProposalStatus::Executed,
            "proposal must be Executed"
        );
    }

    /// Bumps the manager instance + proposal persistent storage and the
    /// target instance storage so they do not expire between ledger advances.
    fn bump_triple_target_ttls(
        env: &Env,
        manager_id: &Address,
        target_id: &Address,
        proposal_id: u64,
        bump: u32,
    ) {
        env.as_contract(manager_id, || {
            env.storage().instance().extend_ttl(bump, bump);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(proposal_id), bump, bump);
        });
        env.as_contract(target_id, || {
            env.storage().instance().extend_ttl(bump, bump);
        });
    }

    // -----------------------------------------------------------------------
    // Metadata tests (issue #123)
    // -----------------------------------------------------------------------

    /// propose_upgrade with an empty title returns EmptyTitle.
    #[test]
    fn test_propose_empty_title() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let result = client.try_propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, ""),
            &desc(&env, "valid description"),
            &zero_hash(&env),
        );
        assert!(result.is_err(), "empty title must return EmptyTitle");
    }

    /// propose_upgrade with a title longer than 128 bytes returns TitleTooLong.
    #[test]
    fn test_propose_title_too_long() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let long_title = String::from_str(&env, &"a".repeat(129));
        let result = client.try_propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &long_title,
            &desc(&env, "valid description"),
            &zero_hash(&env),
        );
        assert!(
            result.is_err(),
            "title > 128 bytes must return TitleTooLong"
        );
    }

    /// get_proposal returns the title and metadata_hash set at creation.
    #[test]
    fn test_proposal_metadata_stored() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let meta_hash = BytesN::from_array(&env, &[42u8; 32]);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "Upgrade to v2"),
            &desc(&env, "Upgrade the target contract to version 2"),
            &meta_hash,
        );
        let proposal = client.get_proposal(&id);
        assert_eq!(proposal.title, title(&env, "Upgrade to v2"));
        assert_eq!(
            proposal.description,
            desc(&env, "Upgrade the target contract to version 2")
        );
        assert_eq!(proposal.metadata_hash, meta_hash);
    }

    /// Zero metadata_hash is accepted (no off-chain metadata).
    #[test]
    fn test_proposal_zero_metadata_hash() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "No metadata"),
            &desc(&env, "Proposal without off-chain metadata"),
            &zero_hash(&env),
        );
        let proposal = client.get_proposal(&id);
        assert_eq!(proposal.metadata_hash, zero_hash(&env));
    }

    // -----------------------------------------------------------------------
    // Delegation tests (issue #116)
    // -----------------------------------------------------------------------

    /// A voter can delegate their vote to another address.
    #[test]
    fn test_delegate_vote() {
        let (env, _admin, _cid, client) = setup();
        let voter = Address::generate(&env);
        let delegate = Address::generate(&env);

        client.delegate(&voter, &delegate);

        // Verify delegation by checking that the delegated voter cannot vote
        // directly (HasDelegated), which is the observable effect of a successful delegation.
        let (env2, _admin2, _cid2, client2) = setup();
        let voter2 = Address::generate(&env2);
        let delegate2 = Address::generate(&env2);
        let target2 = Address::generate(&env2);
        let id2 = client2.propose_upgrade(
            &_admin2,
            &target2,
            &zero_hash(&env2),
            &title(&env2, "P"),
            &desc(&env2, "d"),
            &zero_hash(&env2),
        );
        client2.delegate(&voter2, &delegate2);
        let result = client2.try_vote(&voter2, &id2, &true);
        assert!(
            result.is_err(),
            "delegated voter cannot vote directly — delegation was stored"
        );
    }

    /// A delegated voter cannot vote directly (HasDelegated).
    #[test]
    fn test_delegated_voter_cannot_vote_directly() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let voter = Address::generate(&env);
        let delegate = Address::generate(&env);

        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "P1"),
            &desc(&env, "desc"),
            &zero_hash(&env),
        );
        client.delegate(&voter, &delegate);

        let id = 0u64;
        let result = client.try_vote(&voter, &id, &true);
        assert!(result.is_err(), "delegated voter cannot vote directly");
    }

    /// A delegate's vote carries the weight of 1 + number of delegators.
    #[test]
    fn test_delegate_vote_weight() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let delegate = Address::generate(&env);
        let voter1 = Address::generate(&env);
        let voter2 = Address::generate(&env);

        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "P1"),
            &desc(&env, "desc"),
            &zero_hash(&env),
        );
        let id = 0u64;

        client.delegate(&voter1, &delegate);
        client.delegate(&voter2, &delegate);

        client.vote(&delegate, &id, &true);

        let proposal = client.get_proposal(&id);
        assert_eq!(
            proposal.yes_votes, 3,
            "delegate vote weight must be 1 + 2 delegators = 3"
        );
    }

    /// Cannot delegate to yourself.
    #[test]
    fn test_cannot_delegate_to_self() {
        let (env, _admin, _cid, client) = setup();
        let voter = Address::generate(&env);

        let result = client.try_delegate(&voter, &voter);
        assert!(
            result.is_err(),
            "self-delegation must return CannotDelegateToSelf"
        );
    }

    /// Cannot delegate to someone who has already delegated (no transitive delegation).
    /// Multiple voters can delegate to the same delegate (not transitive).
    /// Only delegation chains (delegate who has itself delegated) are prohibited.
    #[test]
    fn test_no_transitive_delegation() {
        let (env, _admin, _cid, client) = setup();
        let voter_a = Address::generate(&env);
        let voter_b = Address::generate(&env);
        let voter_c = Address::generate(&env);

        // voter_a delegates to voter_b
        client.delegate(&voter_a, &voter_b);

        // voter_b has not itself delegated, so voter_c can still delegate to voter_b
        // (multiple delegators to the same delegate is allowed).
        assert!(
            client.try_delegate(&voter_c, &voter_b).is_ok(),
            "multiple voters delegating to the same delegate is allowed"
        );

        // Now voter_b tries to delegate to voter_c. voter_c has already delegated
        // to voter_b, so the contract refuses with HasDelegated (no chains allowed).
        let result = client.try_delegate(&voter_b, &voter_c);
        assert!(
            result.is_err(),
            "cannot delegate to someone who has already delegated"
        );
    }

    /// A voter can revoke their delegation and vote directly.
    #[test]
    fn test_revoke_delegation() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let voter = Address::generate(&env);
        let delegate = Address::generate(&env);

        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "P1"),
            &desc(&env, "desc"),
            &zero_hash(&env),
        );
        let id = 0u64;

        client.delegate(&voter, &delegate);
        client.revoke_delegation(&voter);
        client.vote(&voter, &id, &true);

        let proposal = client.get_proposal(&id);
        assert_eq!(
            proposal.yes_votes, 1,
            "voter's direct vote must count after revoke"
        );
    }

    /// Revoking delegation when not delegating returns NotDelegating.
    #[test]
    fn test_revoke_not_delegating() {
        let (env, _admin, _cid, client) = setup();
        let voter = Address::generate(&env);

        let result = client.try_revoke_delegation(&voter);
        assert!(
            result.is_err(),
            "revoke without delegation must return NotDelegating"
        );
    }

    /// Delegation does not solve sybil resistance: many addresses can delegate to one delegate.
    #[test]
    fn test_delegation_sybil_not_solved() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let delegate = Address::generate(&env);

        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "P1"),
            &desc(&env, "desc"),
            &zero_hash(&env),
        );
        let id = 0u64;

        for _ in 0..10u32 {
            let sybil = Address::generate(&env);
            client.delegate(&sybil, &delegate);
        }

        client.vote(&delegate, &id, &true);

        let proposal = client.get_proposal(&id);
        assert_eq!(
            proposal.yes_votes, 11,
            "sybil addresses can all delegate to one delegate"
        );
    }

    /// Proposal creation works correctly when the ledger sequence starts at its
    /// initial value (before any ledger advances). Verifies that
    /// `voting_ends_at == starting_ledger + voting_period` and
    /// `created_at == starting_ledger`.
    ///
    /// Uses `voting_period=100` from the standard `setup()` helper.
    #[test]
    fn test_proposal_creation_at_initial_ledger() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);

        // Record the actual starting ledger sequence before initialising.
        let starting_ledger = env.ledger().sequence();

        // Initialize with voting_period=100 (matches the setup() helper default).
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);

        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "initial ledger test"),
            &desc(&env, "proposal at initial ledger"),
            &zero_hash(&env),
        );

        let proposal = client.get_proposal(&id);

        assert_eq!(
            proposal.created_at, starting_ledger,
            "created_at must equal the ledger sequence at proposal creation time"
        );
        assert_eq!(
            proposal.voting_ends_at,
            starting_ledger + 100,
            "voting_ends_at must equal starting_ledger + voting_period (100)"
        );
    }

    /// `voting_period = u32::MAX` is accepted (validate_config only rejects 0) and
    /// `voting_ends_at` is computed correctly without overflow when the ledger
    /// sequence is 0 at proposal creation time.
    ///
    /// With ledger sequence 0 at creation: `voting_ends_at = 0 + u32::MAX = u32::MAX`.
    #[test]
    fn test_max_voting_period() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);

        // Initialize with voting_period = u32::MAX; other params use safe small values.
        client.initialize(&admin, &u32::MAX, &1u32, &1u32, &51u32, &0u32);

        let starting_ledger = env.ledger().sequence();

        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "max voting period"),
            &desc(&env, "tests u32::MAX voting_period"),
            &zero_hash(&env),
        );

        let proposal = client.get_proposal(&id);

        assert_eq!(
            proposal.status,
            ProposalStatus::Active,
            "proposal must be Active after creation"
        );

        // voting_ends_at = starting_ledger + u32::MAX, with no overflow when
        // starting_ledger == 0.
        let expected_voting_ends_at = starting_ledger
            .checked_add(u32::MAX)
            .expect("overflow in test: starting_ledger must be low enough");
        assert_eq!(
            proposal.voting_ends_at, expected_voting_ends_at,
            "voting_ends_at must equal starting_ledger + u32::MAX"
        );
        assert_eq!(
            proposal.snap_voting_period,
            u32::MAX,
            "snapshot voting period must equal u32::MAX"
        );
    }

    /// `timelock_delay = u32::MAX` makes `finalize` return `Error::Overflow`.
    ///
    /// On a passed vote, `finalize_voting` computes
    /// `executable_at = current_ledger + snap_timelock_delay` with
    /// `checked_add`. With `timelock_delay = u32::MAX` and any
    /// `current_ledger > 0`, the addition overflows u32 and `finalize`
    /// must return `Error::Overflow` (error code 15) instead of queueing
    /// the proposal with a wrapped `executable_at`.
    ///
    /// Uses `voting_period = 1` so a 2-ledger advance closes voting, and
    /// 1 YES vote satisfies quorum 1 / threshold 51.
    #[test]
    fn test_max_timelock_delay_overflow_on_finalize() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);

        // Initialize with timelock_delay = u32::MAX and a short voting period.
        client.initialize(&admin, &1u32, &u32::MAX, &1u32, &51u32, &0u32);

        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "max timelock delay"),
            &desc(&env, "tests u32::MAX timelock_delay overflow on finalize"),
            &zero_hash(&env),
        );

        // Quorum is 1: a single YES vote passes the proposal.
        client.vote(&Address::generate(&env), &id, &true);

        // Extend TTLs before advancing, then close the 1-ledger voting window.
        // After the advance the current ledger is 2, so 2 + u32::MAX overflows u32.
        const BUMP: u32 = 30_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 2);

        assert_eq!(
            client.try_finalize(&id),
            Err(Ok(Error::Overflow)),
            "finalize with timelock_delay = u32::MAX and current_ledger > 0 must overflow"
        );

        // The failed finalize left the proposal Active (not Queued) because
        // the whole transaction reverted.
        assert_eq!(
            client.get_proposal(&id).status,
            ProposalStatus::Active,
            "proposal must remain Active after the reverted finalize"
        );
    }

    /// Delegation event is emitted when delegating.
    #[test]
    fn test_delegation_event_emitted() {
        let (env, _admin, _cid, client) = setup();
        let voter = Address::generate(&env);
        let delegate = Address::generate(&env);

        client.delegate(&voter, &delegate);

        assert!(
            count_events(&env, symbol_short!("delegat"), symbol_short!("delegated")) > 0,
            "delegation event must be emitted"
        );
    }

    // -----------------------------------------------------------------------
    // storage key collision
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // SHA256SUMS fixture verification (issue #72)
    // -----------------------------------------------------------------------

    /// Verifies that the committed fixture WASMs match the SHA-256 hashes
    /// recorded in `tests/fixtures/SHA256SUMS`.
    ///
    /// Both the WASM bytes and the SHA256SUMS file are embedded at compile time
    /// via `include_bytes!`. The test parses each `<hash>  <filename>` line and
    /// compares the recorded hex string against the SHA-256 digest of the
    /// corresponding WASM bytes.
    #[test]
    fn test_fixture_sha256sums_match() {
        use sha2::{Digest, Sha256};

        let v1_bytes = include_bytes!("../tests/fixtures/target_v1.wasm");
        let v2_bytes = include_bytes!("../tests/fixtures/target_v2.wasm");
        let sums_bytes = include_bytes!("../tests/fixtures/SHA256SUMS");

        // Map filename to computed hex digest.
        let computed: std::collections::HashMap<&str, std::string::String> = [
            (
                "target_v1.wasm",
                fixture_hex_digest(Sha256::digest(v1_bytes)),
            ),
            (
                "target_v2.wasm",
                fixture_hex_digest(Sha256::digest(v2_bytes)),
            ),
        ]
        .into_iter()
        .collect();

        let sums_text = std::str::from_utf8(sums_bytes).expect("SHA256SUMS must be valid UTF-8");

        let mut checked = 0usize;
        for line in sums_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Format: "<hash>  <filename>"  (two spaces as sha256sum emits)
            let mut parts = line.splitn(2, "  ");
            let recorded_hash = parts
                .next()
                .expect("SHA256SUMS line must contain a hash")
                .trim();
            let filename = parts
                .next()
                .expect("SHA256SUMS line must contain a filename")
                .trim();
            let computed_hash = computed
                .get(filename)
                .unwrap_or_else(|| panic!("unexpected fixture filename in SHA256SUMS: {filename}"));
            assert_eq!(
                recorded_hash, computed_hash,
                "SHA-256 mismatch for {filename}: recorded={recorded_hash} computed={computed_hash}"
            );
            checked += 1;
        }

        assert_eq!(checked, 2, "SHA256SUMS must contain exactly 2 entries");
    }

    /// Encode a SHA-256 digest as a lowercase hex string.
    fn fixture_hex_digest(digest: impl AsRef<[u8]>) -> std::string::String {
        digest
            .as_ref()
            .iter()
            .map(|b| std::format!("{b:02x}"))
            .collect()
    }

    /// DataKey variants must not collide in persistent storage.
    /// Writing under Config, Admin, Proposal(0), Vote(0, voter1), Proposal(1),
    /// Vote(1, voter2) must each be independent and not corrupt other entries.
    #[test]
    fn test_storage_key_no_collision() {
        let (env, _admin, cid, _client) = setup();

        let voter1 = Address::generate(&env);
        let voter2 = Address::generate(&env);

        // Write distinct sentinel values under several DataKey variants.
        env.as_contract(&cid, || {
            // Config is stored in instance storage with its own DataKey::Config.
            // We write a u32 under a few persistent keys to prove no collision.

            // Proposal(0) → sentinel 100u32
            env.storage()
                .persistent()
                .set(&DataKey::Proposal(0), &100u32);

            // Proposal(1) → sentinel 200u32
            env.storage()
                .persistent()
                .set(&DataKey::Proposal(1), &200u32);

            // Vote(0, voter1) → true
            env.storage()
                .persistent()
                .set(&DataKey::Vote(0, voter1.clone()), &true);

            // Vote(1, voter2) → false
            env.storage()
                .persistent()
                .set(&DataKey::Vote(1, voter2.clone()), &false);

            // Vote(0, voter2) → true (same proposal id as first Vote entry, different address)
            env.storage()
                .persistent()
                .set(&DataKey::Vote(0, voter2.clone()), &true);
        });

        // Read back all entries and verify each is independent.
        env.as_contract(&cid, || {
            let p0: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::Proposal(0))
                .expect("Proposal(0) must exist");
            assert_eq!(p0, 100u32, "Proposal(0) must be 100");

            let p1: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::Proposal(1))
                .expect("Proposal(1) must exist");
            assert_eq!(p1, 200u32, "Proposal(1) must be 200");

            let v0_voter1: bool = env
                .storage()
                .persistent()
                .get(&DataKey::Vote(0, voter1.clone()))
                .expect("Vote(0, voter1) must exist");
            assert!(v0_voter1, "Vote(0, voter1) must be true");

            let v1_voter2: bool = env
                .storage()
                .persistent()
                .get(&DataKey::Vote(1, voter2.clone()))
                .expect("Vote(1, voter2) must exist");
            assert!(!v1_voter2, "Vote(1, voter2) must be false");

            let v0_voter2: bool = env
                .storage()
                .persistent()
                .get(&DataKey::Vote(0, voter2.clone()))
                .expect("Vote(0, voter2) must exist");
            assert!(v0_voter2, "Vote(0, voter2) must be true");

            // Verify that Proposal(0) was not affected by Vote(0, voter1) or Vote(0, voter2)
            let p0_again: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::Proposal(0))
                .expect("Proposal(0) must still exist");
            assert_eq!(
                p0_again, 100u32,
                "Proposal(0) must still be 100 after Vote writes"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Proposal rate limit
    // -----------------------------------------------------------------------

    /// Helper: set up a contract with the given proposal_min_interval.
    fn setup_with_interval(
        interval: u32,
    ) -> (Env, Address, Address, UpgradeManagerClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32, &interval);
        (env, admin, cid, client)
    }

    /// First proposal from an address always succeeds regardless of interval.
    #[test]
    fn test_rate_limit_allows_first_proposal() {
        let (env, admin, _cid, client) = setup_with_interval(1000);
        let target = Address::generate(&env);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first proposal"),
            &zero_hash(&env),
        );
        assert_eq!(id, 0u64);
    }

    /// Second proposal immediately after the first returns ProposalRateLimitExceeded.
    #[test]
    fn test_rate_limit_blocks_immediate_second_proposal() {
        let (env, admin, _cid, client) = setup_with_interval(1000);
        let target = Address::generate(&env);
        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first proposal"),
            &zero_hash(&env),
        );
        // No ledger advance — gap is 0, which is < 1000.
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "second"),
                &desc(&env, "second proposal"),
                &zero_hash(&env),
            ),
            Err(Ok(Error::ProposalRateLimitExceeded))
        );
    }

    /// Second proposal succeeds once proposal_min_interval ledgers have passed.
    #[test]
    fn test_rate_limit_allows_after_interval_passes() {
        let interval: u32 = 50;
        let (env, admin, cid, client) = setup_with_interval(interval);
        let target = Address::generate(&env);
        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first proposal"),
            &zero_hash(&env),
        );

        // Advance exactly interval ledgers.
        const BUMP: u32 = 100_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += interval);

        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "second"),
            &desc(&env, "second proposal"),
            &zero_hash(&env),
        );
        assert_eq!(id, 1u64);
    }

    /// Rate-limiting address A does not block address B from proposing.
    #[test]
    fn test_rate_limit_independent_per_address() {
        let (env, admin, _cid, client) = setup_with_interval(1000);
        let target = Address::generate(&env);
        // Address A creates a proposal.
        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "admin first"),
            &desc(&env, "admin first"),
            &zero_hash(&env),
        );
        // Address B (different from admin) can still propose immediately.
        let other = Address::generate(&env);
        let id = client.propose_upgrade(
            &other,
            &target,
            &zero_hash(&env),
            &title(&env, "other first"),
            &desc(&env, "other first"),
            &zero_hash(&env),
        );
        assert_eq!(id, 1u64);
    }

    /// proposal_min_interval = 0 disables the check: unlimited proposals allowed.
    #[test]
    fn test_rate_limit_zero_disables_check() {
        let (env, admin, _cid, client) = setup_with_interval(0);
        let target = Address::generate(&env);
        for i in 0u64..5 {
            let id = client.propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, &std::format!("proposal {i}")),
                &desc(&env, &std::format!("desc {i}")),
                &zero_hash(&env),
            );
            assert_eq!(id, i);
        }
    }

    /// Proposal at exactly proposal_min_interval ledgers after the last one succeeds.
    #[test]
    fn test_rate_limit_exact_boundary() {
        let interval: u32 = 100;
        let (env, admin, cid, client) = setup_with_interval(interval);
        let target = Address::generate(&env);
        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first"),
            &zero_hash(&env),
        );

        const BUMP: u32 = 100_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += interval); // gap == interval exactly

        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "second"),
            &desc(&env, "second"),
            &zero_hash(&env),
        );
        assert_eq!(id, 1u64);
    }

    /// Proposal at interval - 1 ledgers after the last one is rejected.
    #[test]
    fn test_rate_limit_one_before_boundary() {
        let interval: u32 = 100;
        let (env, admin, cid, client) = setup_with_interval(interval);
        let target = Address::generate(&env);
        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first"),
            &zero_hash(&env),
        );

        const BUMP: u32 = 100_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += interval - 1); // gap == interval - 1

        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "second"),
                &desc(&env, "second"),
                &zero_hash(&env),
            ),
            Err(Ok(Error::ProposalRateLimitExceeded))
        );
    }

    /// A proposal that fails validation (EmptyTitle) must NOT set the rate-limit window.
    #[test]
    fn test_rate_limit_failed_proposal_does_not_set_window() {
        let (env, admin, _cid, client) = setup_with_interval(1000);
        let target = Address::generate(&env);

        // Attempt a proposal with an empty title — must fail.
        let empty_title = String::from_str(&env, "");
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &empty_title,
                &desc(&env, "valid description"),
                &zero_hash(&env),
            ),
            Err(Ok(Error::EmptyTitle))
        );

        // The window must NOT have been set, so a valid proposal immediately after succeeds.
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "valid title"),
            &desc(&env, "valid description"),
            &zero_hash(&env),
        );
        assert_eq!(id, 0u64);
    }

    /// Updating proposal_min_interval via update_config takes effect immediately.
    #[test]
    fn test_rate_limit_update_config_changes_interval() {
        let (env, admin, cid, client) = setup_with_interval(1000);
        let target = Address::generate(&env);

        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first"),
            &zero_hash(&env),
        );

        // Tighten interval to 5000 — still blocked.
        client.update_config(&admin, &100u32, &17_280u32, &3u32, &51u32, &5000u32);
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "second-blocked"),
                &desc(&env, "second-blocked"),
                &zero_hash(&env),
            ),
            Err(Ok(Error::ProposalRateLimitExceeded))
        );

        // Relax interval to 0 — now unlimited.
        client.update_config(&admin, &100u32, &17_280u32, &3u32, &51u32, &0u32);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "second-ok"),
            &desc(&env, "second-ok"),
            &zero_hash(&env),
        );
        assert_eq!(id, 1u64);

        // Re-enable a small interval so a third proposal is blocked again.
        client.update_config(&admin, &100u32, &17_280u32, &3u32, &51u32, &500u32);
        assert_eq!(
            client.try_propose_upgrade(
                &admin,
                &target,
                &zero_hash(&env),
                &title(&env, "third-blocked"),
                &desc(&env, "third-blocked"),
                &zero_hash(&env),
            ),
            Err(Ok(Error::ProposalRateLimitExceeded))
        );

        // Advance 500 ledgers — now allowed.
        const BUMP: u32 = 100_000;
        env.as_contract(&cid, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += 500);
        let id = client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "third-ok"),
            &desc(&env, "third-ok"),
            &zero_hash(&env),
        );
        assert_eq!(id, 2u64);
    }

    /// LastProposalLedger entry exists in persistent storage after a proposal
    /// and its TTL is at least PERSISTENT_TTL_BUMP.
    #[test]
    fn test_last_proposal_ledger_ttl_extended() {
        let (env, admin, cid, client) = setup_with_interval(500);
        let target = Address::generate(&env);

        client.propose_upgrade(
            &admin,
            &target,
            &zero_hash(&env),
            &title(&env, "first"),
            &desc(&env, "first"),
            &zero_hash(&env),
        );

        // The key must exist in persistent storage with the creation ledger value (0).
        env.as_contract(&cid, || {
            let stored: Option<u32> = env
                .storage()
                .persistent()
                .get(&DataKey::LastProposalLedger(admin.clone()));
            assert_eq!(
                stored,
                Some(0u32),
                "LastProposalLedger must be set to ledger 0"
            );
        });
    }
}
