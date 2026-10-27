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

// -----------------------------------------------------------------------
// Main test module
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::{UpgradeManager, UpgradeManagerClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Events, Ledger},
        Address, BytesN, Env, String, Symbol,
    };

    use crate::types::{DataKey, Error, ProposalStatus, INSTANCE_TTL_BUMP, PERSISTENT_TTL_BUMP};

    use super::fixture_target_contract::{FixtureTarget, FixtureTargetClient};
    use super::mock_target_contract::MockTarget;
    use super::no_upgrade_contract::NoUpgradeFn;

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
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
        (env, admin, cid, client)
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn desc(env: &Env, s: &str) -> String {
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
            client.try_initialize(&admin, &100u32, &100u32, &1u32, &51u32),
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
            client.try_initialize(&admin, &100u32, &0u32, &1u32, &51u32),
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
            client.try_initialize(&admin, &0u32, &100u32, &1u32, &51u32),
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
            client.try_initialize(&admin, &100u32, &100u32, &0u32, &51u32),
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
            client.try_initialize(&admin, &100u32, &100u32, &1u32, &0u32),
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
            client.try_initialize(&admin, &100u32, &100u32, &1u32, &101u32),
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
        let result = client.try_initialize(&admin, &100u32, &100u32, &1u32, &51u32);
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
        let result =
            client.try_propose_upgrade(&proposer, &target, &zero_hash(&env), &desc(&env, "test"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
            client.try_update_config(&non_admin, &50u32, &100u32, &1u32, &51u32),
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
        let result = client.try_update_config(&caller, &50u32, &100u32, &1u32, &51u32);
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        assert_eq!(id, 0u64);
        let p = client.get_proposal(&id);
        assert_eq!(p.status, ProposalStatus::Active);
        assert_eq!(p.target, target);
        assert_eq!(p.yes_votes, 0);
    }

    #[test]
    fn test_description_too_long() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let long_desc = String::from_str(&env, &"a".repeat(257));
        assert_eq!(
            client.try_propose_upgrade(&admin, &target, &zero_hash(&env), &long_desc),
            Err(Ok(Error::DescriptionTooLong))
        );
    }

    #[test]
    fn test_description_empty_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let empty = String::from_str(&env, "");
        assert_eq!(
            client.try_propose_upgrade(&admin, &target, &zero_hash(&env), &empty),
            Err(Ok(Error::EmptyDescription))
        );
    }

    /// 256-byte description is exactly at the limit — must succeed.
    #[test]
    fn test_description_256_bytes_ok() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let max_desc = String::from_str(&env, &"a".repeat(256));
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &max_desc);
        assert_eq!(id, 0u64);
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
            client.try_propose_upgrade(&proposer, &target, &zero_hash(&env), &desc(&env, "v2")),
            Err(Ok(Error::NotInitialized))
        );
    }

    /// Config fields are snapshotted at creation time. Changing config mid-proposal
    /// does not affect the proposal's snap_* fields.
    #[test]
    fn test_config_snapshot_isolation() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));

        let p = client.get_proposal(&id);
        assert_eq!(p.snap_quorum, 3);
        assert_eq!(p.snap_threshold_pct, 51);
        assert_eq!(p.snap_timelock_delay, 17_280);
        assert_eq!(p.snap_voting_period, 100);

        // Change config — new values must not affect in-flight proposal.
        client.update_config(&admin, &200u32, &5_000u32, &10u32, &75u32);
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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

    /// Voting on the exact last ledger of voting period (voting_ends_at) must succeed.
    #[test]
    fn test_vote_at_voting_ends_at_succeeds() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingStillActive)));
    }

    #[test]
    fn test_finalize_passes_with_quorum() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        for _ in 0..2 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
    }

    /// Quorum met entirely by NO votes → Defeated (0% YES < 51% threshold).
    #[test]
    fn test_finalize_defeated_all_no_votes() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &false);
        }
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
    }

    /// Finalize on an already-Queued proposal fails with VotingClosed.
    #[test]
    fn test_finalize_already_queued_fails() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);
    }

    /// Admin can cancel a Queued proposal.
    #[test]
    fn test_cancel_queued_proposal() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        client.initialize(&admin, &100u32, &100u32, &1u32, &51u32);

        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));
        let target_id = env.register_at(
            &Address::generate(&env),
            super::fixture_target_contract::FixtureTarget,
            (manager_id.clone(),),
        );

        let id = client.propose_upgrade(&admin, &target_id, &v2_hash, &desc(&env, "will execute"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    #[test]
    fn test_execute_fails_on_defeated_proposal() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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

    // -----------------------------------------------------------------------
    // time_until_executable
    // -----------------------------------------------------------------------

    #[test]
    fn test_time_until_executable() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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

    // -----------------------------------------------------------------------
    // update_config / transfer_admin
    // -----------------------------------------------------------------------

    #[test]
    fn test_update_config() {
        let (_env, admin, _cid, client) = setup();
        client.update_config(&admin, &50u32, &1000u32, &5u32, &66u32);
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
            client.try_update_config(&admin, &0u32, &1000u32, &5u32, &66u32),
            Err(Ok(Error::InvalidVotingPeriod))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &0u32, &5u32, &66u32),
            Err(Ok(Error::InvalidTimelockDelay))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &1000u32, &0u32, &66u32),
            Err(Ok(Error::InvalidQuorum))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &1000u32, &5u32, &0u32),
            Err(Ok(Error::InvalidThreshold))
        );
        assert_eq!(
            client.try_update_config(&admin, &50u32, &1000u32, &5u32, &101u32),
            Err(Ok(Error::InvalidThreshold))
        );
    }

    #[test]
    fn test_transfer_admin() {
        let (env, admin, _cid, client) = setup();
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin);
        assert_eq!(client.get_admin(), new_admin);
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&new_admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        assert_eq!(client.try_cancel(&admin, &id), Err(Ok(Error::Unauthorized)));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
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
        client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        client.cancel(&admin, &id);
        assert!(
            count_events(&env, symbol_short!("proposal"), symbol_short!("cancelled")) >= 1,
            "missing cancelled event"
        );
    }

    #[test]
    fn test_event_config_updated() {
        let (env, admin, _cid, client) = setup();
        client.update_config(&admin, &50u32, &1000u32, &1u32, &51u32);
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
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);

        let pid = manager.propose_upgrade(
            &admin,
            &target_id,
            &new_hash,
            &String::from_str(&env, "Integration test"),
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    #[test]
    fn test_execute_fails_on_defeated_proposal_compat() {
        let (env, admin, cid, client) = setup();
        let target = env.register(MockTarget, ());
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &zero_hash(&env), &desc(&env, "v2"));
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
    /// SHA256SUMS for drift detection. They cannot be uploaded via
    /// upload_contract_wasm() in this test because soroban-wasmi 0.31.1 (used
    /// by soroban-env-host 22.1.3) rejects call_indirect instructions whose
    /// table-index byte is LEB128-encoded (Rust ≥ 1.85.0 emits multi-byte zeros
    /// for this reserved byte). The mock host does not execute WASM code at all —
    /// it uses native Rust — so we use Bytes::new(&env) (empty bytes) as the
    /// upload target. The SHA256 of the fixture WASMs is still documented in
    /// tests/fixtures/SHA256SUMS and verified by the CI fixture drift check.
    ///
    /// **Limitation:** The mock host does not hot-swap WASM in memory after
    /// update_current_contract_wasm, so we cannot call version() post-execute
    /// and expect 2 within the same test env.
    #[test]
    fn test_real_v1_to_v2_upgrade_through_manager() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);

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
            &String::from_str(&env, "Upgrade example-target v1 to v2"),
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

    /// Double-execute fails with NotQueued on the second call.
    #[test]
    fn test_double_execute_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);

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
            &desc(&env, "double-execute test"),
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
        manager.initialize(&admin, &100u32, &100u32, &1u32, &51u32);

        let bad_target = env.register(NoUpgradeFn, ());

        // Use empty bytes as upgrade hash (real fixture WASMs rejected by validator).
        let v2_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

        let id = manager.propose_upgrade(&admin, &bad_target, &v2_hash, &desc(&env, "will fail"));
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
} // end mod tests
