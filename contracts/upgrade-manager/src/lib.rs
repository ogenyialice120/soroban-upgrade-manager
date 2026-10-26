//! # Soroban Upgrade Manager
//!
//! A Soroban smart contract that manages WASM upgrades for other contracts
//! through a governance vote followed by a configurable timelock delay.
//!
//! ## Lifecycle
//!
//! ```text
//! initialize() → propose_upgrade() → vote() → finalize() → execute()
//! ```
//!
//! 1. **propose_upgrade** — any authenticated address creates a proposal.
//! 2. **vote** — any address votes YES/NO; voting closes at `voting_ends_at`.
//! 3. **finalize** — after `voting_ends_at`, anyone closes the vote and moves
//!    the proposal to `Queued` (pass) or `Defeated` (fail). On pass,
//!    `executable_at` is set to `finalize_ledger + timelock_delay`.
//! 4. **execute** — after `executable_at`, anyone calls the target's `upgrade()`.
//!
//! Admin actions (cancel, update_config, transfer_admin) bypass voting but
//! are on-chain and emitted as events.

#![no_std]

mod governance;
mod timelock;
mod types;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

use governance::{
    cancel_proposal, cast_vote, create_proposal, emit_admin_transferred, emit_config_updated,
    emit_executed, emit_initialized, finalize_voting, invoke_upgrade, load_admin, load_config,
    load_proposal, require_admin, save_admin, save_config, validate_config,
};
use timelock::{assert_timelock_passed, ledgers_until_executable};
use types::{Config, DataKey, Error, Proposal, ProposalStatus, INSTANCE_TTL_BUMP};

// ---------------------------------------------------------------------------
// Contract struct
// ---------------------------------------------------------------------------

#[contract]
pub struct UpgradeManager;

#[contractimpl]
impl UpgradeManager {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the contract with an admin address and governance config.
    ///
    /// Can only be called once. The admin must sign the transaction.
    ///
    /// # Parameters
    /// - `admin`                  — Address that can cancel proposals, update config, and transfer admin.
    /// - `voting_period`          — Ledgers during which votes are accepted. Must be >= 1.
    /// - `timelock_delay`         — Ledgers between finalization and earliest execution. Must be >= 1.
    /// - `quorum`                 — Minimum total votes (YES+NO) required for a proposal to pass. Must be >= 1.
    /// - `approval_threshold_pct` — Percentage of YES/(YES+NO) votes required (1–100).
    ///
    /// # Errors
    /// - `AlreadyInitialized`  — contract storage already contains a Config entry.
    /// - `InvalidVotingPeriod`, `InvalidTimelockDelay`, `InvalidQuorum`, `InvalidThreshold` — bad config values.
    pub fn initialize(
        env: Env,
        admin: Address,
        voting_period: u32,
        timelock_delay: u32,
        quorum: u32,
        approval_threshold_pct: u32,
    ) -> Result<(), Error> {
        // Prevent front-running: the intended admin must sign.
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }

        let config = Config {
            voting_period,
            timelock_delay,
            quorum,
            approval_threshold_pct,
        };
        validate_config(&config)?;

        save_admin(&env, &admin);
        save_config(&env, &config);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_BUMP, INSTANCE_TTL_BUMP);

        emit_initialized(&env, &admin);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin actions
    // -----------------------------------------------------------------------

    /// Update governance configuration. Only the admin can call this.
    ///
    /// This does NOT affect proposals that are already in flight; each proposal
    /// snapshots the config rules at creation time.
    ///
    /// # Errors
    /// - `NotInitialized` — contract is not yet initialised.
    /// - `Unauthorized`   — caller is not the admin.
    /// - Validation errors for bad config values (same as `initialize`).
    pub fn update_config(
        env: Env,
        caller: Address,
        voting_period: u32,
        timelock_delay: u32,
        quorum: u32,
        approval_threshold_pct: u32,
    ) -> Result<(), Error> {
        require_admin(&env, &caller)?;

        let config = Config {
            voting_period,
            timelock_delay,
            quorum,
            approval_threshold_pct,
        };
        validate_config(&config)?;

        save_config(&env, &config);
        emit_config_updated(&env, &caller);
        Ok(())
    }

    /// Transfer the admin role to a new address. Only the current admin can call this.
    ///
    /// # Errors
    /// - `NotInitialized` — contract is not yet initialised.
    /// - `Unauthorized`   — caller is not the current admin.
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) -> Result<(), Error> {
        require_admin(&env, &caller)?;
        save_admin(&env, &new_admin);
        emit_admin_transferred(&env, &caller, &new_admin);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Proposal lifecycle
    // -----------------------------------------------------------------------

    /// Create a new upgrade proposal.
    ///
    /// Any authenticated address may propose an upgrade. The governance rules
    /// (voting_period, timelock_delay, quorum, approval_threshold_pct) are
    /// snapshotted into the proposal at creation time.
    ///
    /// # Parameters
    /// - `proposer`       — Address creating the proposal (must sign).
    /// - `target`         — Contract address to be upgraded.
    /// - `new_wasm_hash`  — Hash of the already-uploaded WASM binary.
    /// - `description`    — Human-readable description (1–256 bytes).
    ///
    /// # Returns
    /// The new proposal ID (starts at 0, increments by 1).
    ///
    /// # Errors
    /// - `NotInitialized`    — contract is not yet initialised.
    /// - `EmptyDescription`  — description is empty.
    /// - `DescriptionTooLong` — description exceeds 256 bytes.
    /// - `Overflow`          — ledger arithmetic overflow.
    pub fn propose_upgrade(
        env: Env,
        proposer: Address,
        target: Address,
        new_wasm_hash: BytesN<32>,
        description: String,
    ) -> Result<u64, Error> {
        proposer.require_auth();
        create_proposal(&env, proposer, target, new_wasm_hash, description)
    }

    /// Vote on an active proposal.
    ///
    /// The voter must authenticate. Each address may vote exactly once.
    /// Voting is only allowed while ledger.sequence() <= voting_ends_at.
    ///
    /// # Parameters
    /// - `voter`       — Address casting the vote (must sign).
    /// - `proposal_id` — ID of the target proposal.
    /// - `approve`     — `true` for YES, `false` for NO.
    ///
    /// # Errors
    /// - `ProposalNotFound`  — unknown proposal ID.
    /// - `VotingClosed`      — proposal is not Active.
    /// - `VotingPeriodEnded` — the voting window has closed.
    /// - `AlreadyVoted`      — this address already voted.
    /// - `Overflow`          — vote count arithmetic overflow.
    pub fn vote(env: Env, voter: Address, proposal_id: u64, approve: bool) -> Result<(), Error> {
        voter.require_auth();
        cast_vote(&env, proposal_id, voter, approve)
    }

    /// Finalize voting on a proposal.
    ///
    /// Can be called by anyone once the voting period has ended
    /// (ledger.sequence() > voting_ends_at).
    /// On pass: status → Queued, executable_at = current_ledger + snapshot_timelock_delay.
    /// On fail: status → Defeated.
    ///
    /// # Errors
    /// - `ProposalNotFound`   — unknown proposal ID.
    /// - `VotingClosed`       — proposal is not Active.
    /// - `VotingStillActive`  — the voting period has not ended yet.
    /// - `Overflow`           — ledger arithmetic overflow.
    pub fn finalize(env: Env, proposal_id: u64) -> Result<ProposalStatus, Error> {
        finalize_voting(&env, proposal_id)
    }

    /// Execute a queued proposal by performing the cross-contract WASM upgrade.
    ///
    /// Can be called by anyone after the timelock has expired
    /// (ledger.sequence() >= executable_at).
    ///
    /// The proposal status is set to Executed BEFORE invoking the target
    /// (checks-effects-interactions pattern). If the target call fails, the
    /// entire transaction reverts, keeping the proposal Queued.
    ///
    /// # Errors
    /// - `ProposalNotFound`   — unknown proposal ID.
    /// - `NotQueued`          — proposal is not in the Queued state.
    /// - `TimelockNotExpired` — timelock has not yet elapsed.
    pub fn execute(env: Env, proposal_id: u64) -> Result<(), Error> {
        let mut proposal = load_proposal(&env, proposal_id)?;

        // Enforce timelock (also checks Queued status).
        assert_timelock_passed(&env, &proposal)?;

        // Checks-effects-interactions: update state BEFORE external call.
        proposal.status = ProposalStatus::Executed;
        governance::save_proposal(&env, &proposal);

        // Emit executed event before the external call.
        emit_executed(&env, proposal_id);

        // Perform the cross-contract upgrade by invoking the target contract's
        // `upgrade` function. If this call fails, the entire transaction reverts,
        // and the proposal returns to Queued.
        invoke_upgrade(&env, &proposal.target, &proposal.new_wasm_hash);

        Ok(())
    }

    /// Cancel an Active or Queued proposal. Only the admin can call this.
    ///
    /// # Errors
    /// - `NotInitialized`    — contract is not yet initialised.
    /// - `Unauthorized`      — caller is not the admin.
    /// - `ProposalNotFound`  — unknown proposal ID.
    /// - `ProposalFinalized` — proposal is already Executed, Cancelled, or Defeated.
    pub fn cancel(env: Env, caller: Address, proposal_id: u64) -> Result<(), Error> {
        cancel_proposal(&env, &caller, proposal_id)
    }

    // -----------------------------------------------------------------------
    // Read-only queries
    // -----------------------------------------------------------------------

    /// Return the full proposal struct for a given ID.
    ///
    /// # Errors
    /// - `ProposalNotFound` — unknown proposal ID.
    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, Error> {
        load_proposal(&env, proposal_id)
    }

    /// Return the current governance configuration.
    ///
    /// # Errors
    /// - `NotInitialized` — contract has not been initialised.
    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    /// Return the current admin address.
    ///
    /// # Errors
    /// - `NotInitialized` — contract has not been initialised.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        load_admin(&env)
    }

    /// Return how many ledgers remain until a proposal's timelock expires.
    ///
    /// Returns `0` if execution is already permitted or if the proposal is not Queued.
    ///
    /// # Errors
    /// - `ProposalNotFound` — unknown proposal ID.
    pub fn time_until_executable(env: Env, proposal_id: u64) -> Result<u32, Error> {
        let proposal = load_proposal(&env, proposal_id)?;
        Ok(ledgers_until_executable(&env, &proposal))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Returns (env, admin, contract_id, client).
    /// Uses voting_period=100, timelock_delay=17_280, quorum=3, threshold=51.
    fn setup() -> (Env, Address, Address, UpgradeManagerClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
        (env, admin, contract_id, client)
    }

    fn wasm_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn desc(env: &Env, s: &str) -> String {
        String::from_str(env, s)
    }

    /// Advance ledger past the voting period so finalize is allowed.
    fn advance_past_voting(env: &Env, contract_id: &Address, proposal_id: u64) {
        use types::DataKey;
        const ADVANCE: u32 = 101; // past voting_period=100
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(contract_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
    }

    /// Advance ledger past both voting period and timelock so execute is allowed.
    /// Also extends vote entry TTLs for all `voter_ids` if provided.
    fn advance_past_timelock(env: &Env, contract_id: &Address, proposal_id: u64) {
        use types::DataKey;
        // finalize first, then timelock_delay=17_280 more ledgers
        // We advance far enough to be past voting (101) + timelock (17_280) + margin
        const ADVANCE: u32 = 17_382; // 101 + 17_280 + 1
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(contract_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
        });
        env.ledger().with_mut(|l| l.sequence_number += ADVANCE);
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

    // -----------------------------------------------------------------------
    // propose_upgrade
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_proposal() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        assert_eq!(id, 0u64);
        let proposal = client.get_proposal(&id);
        assert_eq!(proposal.status, ProposalStatus::Active);
        assert_eq!(proposal.target, target);
        assert_eq!(proposal.yes_votes, 0);
    }

    #[test]
    fn test_description_too_long() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let long_desc = String::from_str(&env, &"a".repeat(257));
        assert_eq!(
            client.try_propose_upgrade(&admin, &target, &wasm_hash(&env), &long_desc),
            Err(Ok(Error::DescriptionTooLong))
        );
    }

    // -----------------------------------------------------------------------
    // vote
    // -----------------------------------------------------------------------

    #[test]
    fn test_vote_yes_no() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &false);
        let proposal = client.get_proposal(&id);
        assert_eq!(proposal.yes_votes, 2);
        assert_eq!(proposal.no_votes, 1);
    }

    #[test]
    fn test_double_vote_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
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

    // -----------------------------------------------------------------------
    // finalize
    // -----------------------------------------------------------------------

    #[test]
    fn test_finalize_before_voting_ends_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        // Voting period = 100 ledgers, not yet elapsed
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingStillActive)));
    }

    // Keep old test name as alias so snapshots that reference it still compile
    #[test]
    fn test_finalize_before_timelock_fails() {
        test_finalize_before_voting_ends_fails_inner();
    }
    fn test_finalize_before_voting_ends_fails_inner() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &cid);
        let admin = Address::generate(&env);
        client.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        assert_eq!(client.try_finalize(&id), Err(Ok(Error::VotingStillActive)));
    }

    #[test]
    fn test_finalize_passes_with_quorum() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        // Only 2 votes, quorum = 3 → defeated
        for _ in 0..2 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        assert_eq!(client.finalize(&id), ProposalStatus::Defeated);
    }

    // -----------------------------------------------------------------------
    // cancel
    // -----------------------------------------------------------------------

    #[test]
    fn test_cancel_by_admin() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        client.cancel(&admin, &id);
        assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);
    }

    #[test]
    fn test_cancel_by_non_admin_fails() {
        let (env, admin, _cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
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
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id); // → Queued
        client.cancel(&admin, &id); // → Cancelled
        assert_eq!(
            client.try_cancel(&admin, &id),
            Err(Ok(Error::ProposalFinalized))
        );
    }

    // -----------------------------------------------------------------------
    // time_until_executable
    // -----------------------------------------------------------------------

    #[test]
    fn test_time_until_executable() {
        let (env, admin, cid, client) = setup();
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        // Not yet Queued → returns 0
        assert_eq!(client.time_until_executable(&id), 0u32);

        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id); // → Queued, executable_at set
                              // Just finalized: timelock_delay=17_280 ledgers remain
        assert_eq!(client.time_until_executable(&id), 17_280u32);

        advance_past_timelock(&env, &cid, id);
        // After timelock: 0
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
    fn test_transfer_admin() {
        let (env, admin, _cid, client) = setup();
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin);
        assert_eq!(client.get_admin(), new_admin);
        // Old admin can no longer cancel
        let target = Address::generate(&env);
        let id = client.propose_upgrade(&new_admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        assert_eq!(client.try_cancel(&admin, &id), Err(Ok(Error::Unauthorized)));
    }

    // -----------------------------------------------------------------------
    // Integration: full governance-gated upgrade flow
    // -----------------------------------------------------------------------

    /// A minimal upgradable contract used as the upgrade target in integration tests.
    #[contract]
    pub struct UpgradableTarget;

    #[contractimpl]
    impl UpgradableTarget {
        pub fn version(_env: Env) -> u32 {
            1
        }

        pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
            env.deployer().update_current_contract_wasm(new_wasm_hash);
        }
    }

    /// Full governance-gated upgrade integration test.
    ///
    /// Flow: propose → vote × 3 → advance past voting → finalize (Queued) →
    ///       advance past timelock → execute → proposal Executed.
    ///
    /// Note: In test environment, update_current_contract_wasm succeeds with
    /// the empty-bytes hash uploaded by env.deployer().upload_contract_wasm().
    #[test]
    fn test_full_governance_upgrade_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_id = env.register(UpgradeManager, ());
        let manager = UpgradeManagerClient::new(&env, &manager_id);
        let target_id = env.register(UpgradableTarget, ());

        let new_hash = env
            .deployer()
            .upload_contract_wasm(soroban_sdk::Bytes::new(&env));

        let admin = Address::generate(&env);
        manager.initialize(&admin, &100u32, &17_280u32, &3u32, &51u32);

        let proposal_id = manager.propose_upgrade(
            &admin,
            &target_id,
            &new_hash,
            &String::from_str(&env, "Integration test: upgrade target to v2"),
        );
        assert_eq!(proposal_id, 0u64);

        let proposal = manager.get_proposal(&proposal_id);
        assert_eq!(proposal.status, ProposalStatus::Active);

        let voter1 = Address::generate(&env);
        let voter2 = Address::generate(&env);
        let voter3 = Address::generate(&env);
        manager.vote(&voter1, &proposal_id, &true);
        manager.vote(&voter2, &proposal_id, &true);
        manager.vote(&voter3, &proposal_id, &true);

        let proposal = manager.get_proposal(&proposal_id);
        assert_eq!(proposal.yes_votes, 3);

        // Extend target TTL before advancing ledger
        const ADVANCE: u32 = 17_382;
        const BUMP: u32 = ADVANCE + 30_000;
        env.as_contract(&target_id, || {
            env.storage().instance().extend_ttl(BUMP, BUMP);
        });

        // Advance past voting period
        {
            use types::DataKey;
            env.as_contract(&manager_id, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
            });
        }
        env.ledger().with_mut(|l| l.sequence_number += 101); // past voting_period=100

        // Finalize → Queued
        let status = manager.finalize(&proposal_id);
        assert_eq!(status, ProposalStatus::Queued);

        // Advance past timelock
        {
            use types::DataKey;
            env.as_contract(&manager_id, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Proposal(proposal_id), BUMP, BUMP);
            });
            env.as_contract(&target_id, || {
                env.storage().instance().extend_ttl(BUMP, BUMP);
            });
        }
        env.ledger().with_mut(|l| l.sequence_number += 17_281); // past timelock_delay=17_280

        assert_eq!(manager.time_until_executable(&proposal_id), 0u32);

        manager.execute(&proposal_id);

        let final_proposal = manager.get_proposal(&proposal_id);
        assert_eq!(final_proposal.status, ProposalStatus::Executed);
    }

    /// Verify that execute() fails if the proposal is still Active.
    #[test]
    fn test_execute_fails_on_active_proposal() {
        let (env, admin, _cid, client) = setup();
        let target = env.register(UpgradableTarget, ());
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    /// Verify that execute() fails if the proposal was Defeated.
    #[test]
    fn test_execute_fails_on_defeated_proposal() {
        let (env, admin, cid, client) = setup();
        let target = env.register(UpgradableTarget, ());
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        client.vote(&Address::generate(&env), &id, &true);
        client.vote(&Address::generate(&env), &id, &true);
        advance_past_voting(&env, &cid, id);
        let status = client.finalize(&id);
        assert_eq!(status, ProposalStatus::Defeated);
        assert_eq!(client.try_execute(&id), Err(Ok(Error::NotQueued)));
    }

    /// Verify that execute() fails before the timelock has expired.
    #[test]
    fn test_execute_fails_before_timelock() {
        let (env, admin, cid, client) = setup();
        let target = env.register(UpgradableTarget, ());
        let id = client.propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"));
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true);
        }
        advance_past_voting(&env, &cid, id);
        client.finalize(&id); // → Queued

        // Execute before timelock elapses → TimelockNotExpired
        assert_eq!(client.try_execute(&id), Err(Ok(Error::TimelockNotExpired)));
    }
}
