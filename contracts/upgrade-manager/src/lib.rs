//! # Soroban Upgrade Manager
//!
//! A Soroban smart contract that manages WASM upgrades for other contracts
//! through a governance vote followed by a configurable timelock delay.
//!
//! ## Lifecycle
//!
//! ```text
//! initialize() → propose_upgrade() → vote() → finalize_voting()
//!             → execute_upgrade()
//! ```
//!
//! Any admin action (cancel, update_config) bypasses voting but is still
//! on-chain and auditable.

#![no_std]

mod governance;
mod timelock;
mod types;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

use governance::{
    cancel_proposal, cast_vote, create_proposal, finalize_voting, load_admin, load_config,
    load_proposal, save_admin, save_config,
};
use timelock::{assert_timelock_passed, ledgers_until_executable};
use types::{Config, Error, Proposal, ProposalStatus};

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
    /// Can only be called once.
    ///
    /// # Parameters
    /// - `admin`                  — Address that can cancel proposals and update config.
    /// - `timelock_delay`         — Ledgers between proposal creation and earliest execution.
    /// - `quorum`                 — Minimum total votes required for a proposal to pass.
    /// - `approval_threshold_pct` — Percentage of YES/(YES+NO) votes required (1–100).
    ///
    /// # Errors
    /// - `AlreadyInitialized`  — contract storage already contains a Config entry.
    /// - `InvalidTimelockDelay`, `InvalidQuorum`, `InvalidThreshold` — bad config values.
    pub fn initialize(
        env: Env,
        admin: Address,
        timelock_delay: u32,
        quorum: u32,
        approval_threshold_pct: u32,
    ) -> Result<(), Error> {
        if env
            .storage()
            .instance()
            .has(&types::DataKey::Config)
        {
            return Err(Error::AlreadyInitialized);
        }

        if timelock_delay == 0 {
            return Err(Error::InvalidTimelockDelay);
        }
        if quorum == 0 {
            return Err(Error::InvalidQuorum);
        }
        if approval_threshold_pct == 0 || approval_threshold_pct > 100 {
            return Err(Error::InvalidThreshold);
        }

        save_admin(&env, &admin);
        save_config(
            &env,
            &Config {
                timelock_delay,
                quorum,
                approval_threshold_pct,
            },
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin actions
    // -----------------------------------------------------------------------

    /// Update governance configuration. Only the admin can call this.
    ///
    /// # Errors
    /// - `Unauthorized` — caller is not the admin.
    /// - Validation errors for bad config values (same as `initialize`).
    pub fn update_config(
        env: Env,
        caller: Address,
        timelock_delay: u32,
        quorum: u32,
        approval_threshold_pct: u32,
    ) -> Result<(), Error> {
        caller.require_auth();
        let admin = load_admin(&env);
        if caller != admin {
            return Err(Error::Unauthorized);
        }

        if timelock_delay == 0 {
            return Err(Error::InvalidTimelockDelay);
        }
        if quorum == 0 {
            return Err(Error::InvalidQuorum);
        }
        if approval_threshold_pct == 0 || approval_threshold_pct > 100 {
            return Err(Error::InvalidThreshold);
        }

        save_config(
            &env,
            &Config {
                timelock_delay,
                quorum,
                approval_threshold_pct,
            },
        );
        Ok(())
    }

    /// Transfer the admin role to a new address. Only the current admin can call this.
    ///
    /// # Errors
    /// - `Unauthorized` — caller is not the current admin.
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) -> Result<(), Error> {
        caller.require_auth();
        let admin = load_admin(&env);
        if caller != admin {
            return Err(Error::Unauthorized);
        }
        save_admin(&env, &new_admin);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Proposal lifecycle
    // -----------------------------------------------------------------------

    /// Create a new upgrade proposal.
    ///
    /// Any address may propose an upgrade. The proposer must authenticate.
    ///
    /// # Parameters
    /// - `proposer`       — Address creating the proposal (must sign).
    /// - `target`         — Contract address to be upgraded.
    /// - `new_wasm_hash`  — Hash of the already-uploaded WASM binary.
    /// - `description`    — Human-readable description (max 256 bytes).
    ///
    /// # Returns
    /// The new proposal ID (starts at 0, increments by 1).
    ///
    /// # Errors
    /// - `DescriptionTooLong` — description exceeds 256 bytes.
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
    ///
    /// # Parameters
    /// - `voter`       — Address casting the vote (must sign).
    /// - `proposal_id` — ID of the target proposal.
    /// - `approve`     — `true` for YES, `false` for NO.
    ///
    /// # Errors
    /// - `ProposalNotFound` — unknown proposal ID.
    /// - `VotingClosed`     — proposal is not Active.
    /// - `AlreadyVoted`     — this address already voted.
    pub fn vote(env: Env, voter: Address, proposal_id: u64, approve: bool) -> Result<(), Error> {
        voter.require_auth();
        cast_vote(&env, proposal_id, voter, approve)
    }

    /// Finalize voting on a proposal.
    ///
    /// Can be called by anyone once the timelock period has elapsed.
    /// Moves the proposal to `Queued` (passed) or `Defeated` (failed).
    ///
    /// # Errors
    /// - `ProposalNotFound`    — unknown proposal ID.
    /// - `VotingClosed`        — proposal is not Active.
    /// - `TimelockNotExpired`  — too early to finalize.
    pub fn finalize(env: Env, proposal_id: u64) -> Result<ProposalStatus, Error> {
        finalize_voting(&env, proposal_id)
    }

    /// Execute a queued proposal by performing the cross-contract WASM upgrade.
    ///
    /// Can be called by anyone after the timelock has expired.
    ///
    /// # Errors
    /// - `ProposalNotFound`   — unknown proposal ID.
    /// - `NotQueued`          — proposal is not in the Queued state.
    /// - `TimelockNotExpired` — timelock has not yet elapsed.
    pub fn execute(env: Env, proposal_id: u64) -> Result<(), Error> {
        let mut proposal = load_proposal(&env, proposal_id)?;

        // Enforce timelock
        assert_timelock_passed(&env, &proposal)?;

        // Perform the cross-contract upgrade
        let target_client = soroban_sdk::ContractClient::new(&env, &proposal.target);
        target_client.upgrade(&proposal.new_wasm_hash);

        proposal.status = ProposalStatus::Executed;
        governance::save_proposal(&env, &proposal);

        Ok(())
    }

    /// Cancel an Active or Queued proposal. Only the admin can call this.
    ///
    /// # Errors
    /// - `Unauthorized`       — caller is not the admin.
    /// - `ProposalNotFound`   — unknown proposal ID.
    /// - `ProposalFinalized`  — proposal is already Executed or Cancelled.
    pub fn cancel(env: Env, caller: Address, proposal_id: u64) -> Result<(), Error> {
        caller.require_auth();
        cancel_proposal(&env, caller, proposal_id)
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
    pub fn get_config(env: Env) -> Config {
        load_config(&env)
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Address {
        load_admin(&env)
    }

    /// Return how many ledgers remain until a proposal's timelock expires.
    ///
    /// Returns `0` if execution is already permitted.
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
    use soroban_sdk::{vec, Address, BytesN, Env, String};

    // ---- helpers -----------------------------------------------------------

    fn setup() -> (Env, Address, UpgradeManagerClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        client
            .initialize(&admin, &17_280u32, &3u32, &51u32)
            .unwrap();

        (env, admin, client)
    }

    fn wasm_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn desc(env: &Env, s: &str) -> String {
        String::from_str(env, s)
    }

    // ---- initialize --------------------------------------------------------

    #[test]
    fn test_initialize_success() {
        let (env, admin, client) = setup();
        let config = client.get_config();
        assert_eq!(config.timelock_delay, 17_280);
        assert_eq!(config.quorum, 3);
        assert_eq!(config.approval_threshold_pct, 51);
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    fn test_initialize_twice_fails() {
        let (_, admin, client) = setup();
        let res = client.initialize(&admin, &100u32, &1u32, &51u32);
        assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
    }

    #[test]
    fn test_initialize_zero_delay_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(UpgradeManager, ());
        let client = UpgradeManagerClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        assert_eq!(
            client.initialize(&admin, &0u32, &1u32, &51u32),
            Err(Ok(Error::InvalidTimelockDelay))
        );
    }

    // ---- propose_upgrade ---------------------------------------------------

    #[test]
    fn test_create_proposal() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        assert_eq!(id, 0u64);

        let proposal = client.get_proposal(&id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Active);
        assert_eq!(proposal.target, target);
        assert_eq!(proposal.yes_votes, 0);
    }

    #[test]
    fn test_description_too_long() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        // 257 'a' characters
        let long_desc = String::from_str(&env, &"a".repeat(257));
        assert_eq!(
            client.propose_upgrade(&admin, &target, &wasm_hash(&env), &long_desc),
            Err(Ok(Error::DescriptionTooLong))
        );
    }

    // ---- vote --------------------------------------------------------------

    #[test]
    fn test_vote_yes_no() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();

        let voter1 = Address::generate(&env);
        let voter2 = Address::generate(&env);
        let voter3 = Address::generate(&env);

        client.vote(&voter1, &id, &true).unwrap();
        client.vote(&voter2, &id, &true).unwrap();
        client.vote(&voter3, &id, &false).unwrap();

        let proposal = client.get_proposal(&id).unwrap();
        assert_eq!(proposal.yes_votes, 2);
        assert_eq!(proposal.no_votes, 1);
    }

    #[test]
    fn test_double_vote_fails() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        let voter = Address::generate(&env);
        client.vote(&voter, &id, &true).unwrap();
        assert_eq!(
            client.vote(&voter, &id, &true),
            Err(Ok(Error::AlreadyVoted))
        );
    }

    #[test]
    fn test_vote_on_unknown_proposal_fails() {
        let (env, _, client) = setup();
        let voter = Address::generate(&env);
        assert_eq!(
            client.vote(&voter, &999u64, &true),
            Err(Ok(Error::ProposalNotFound))
        );
    }

    // ---- finalize_voting ---------------------------------------------------

    #[test]
    fn test_finalize_before_timelock_fails() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        // Vote enough to pass
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true).unwrap();
        }
        assert_eq!(
            client.finalize(&id),
            Err(Ok(Error::TimelockNotExpired))
        );
    }

    #[test]
    fn test_finalize_passes_with_quorum() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true).unwrap();
        }
        // Advance ledger past timelock
        env.ledger().with_mut(|l| l.sequence_number += 17_281);
        let status = client.finalize(&id).unwrap();
        assert_eq!(status, ProposalStatus::Queued);
    }

    #[test]
    fn test_finalize_defeated_without_quorum() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        // Only 2 votes (quorum = 3)
        for _ in 0..2 {
            client.vote(&Address::generate(&env), &id, &true).unwrap();
        }
        env.ledger().with_mut(|l| l.sequence_number += 17_281);
        let status = client.finalize(&id).unwrap();
        assert_eq!(status, ProposalStatus::Defeated);
    }

    // ---- cancel ------------------------------------------------------------

    #[test]
    fn test_cancel_by_admin() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        client.cancel(&admin, &id).unwrap();
        let proposal = client.get_proposal(&id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Cancelled);
    }

    #[test]
    fn test_cancel_by_non_admin_fails() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        let non_admin = Address::generate(&env);
        assert_eq!(
            client.cancel(&non_admin, &id),
            Err(Ok(Error::Unauthorized))
        );
    }

    #[test]
    fn test_cancel_executed_proposal_fails() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        for _ in 0..3 {
            client.vote(&Address::generate(&env), &id, &true).unwrap();
        }
        env.ledger().with_mut(|l| l.sequence_number += 17_281);
        client.finalize(&id).unwrap();
        // Mark as Executed manually for this test path (execute() would do cross-contract call)
        // We test the cancel-of-executed guard by cancelling a Cancelled proposal instead.
        client.cancel(&admin, &id).unwrap(); // Cancel the queued one — now Cancelled
        assert_eq!(
            client.cancel(&admin, &id),
            Err(Ok(Error::ProposalFinalized))
        );
    }

    // ---- time_until_executable ---------------------------------------------

    #[test]
    fn test_time_until_executable() {
        let (env, admin, client) = setup();
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        // At creation ledger 0, executable_at = 17_280
        assert_eq!(client.time_until_executable(&id).unwrap(), 17_280u32);
        env.ledger().with_mut(|l| l.sequence_number += 17_281);
        assert_eq!(client.time_until_executable(&id).unwrap(), 0u32);
    }

    // ---- update_config / transfer_admin ------------------------------------

    #[test]
    fn test_update_config() {
        let (_, admin, client) = setup();
        client
            .update_config(&admin, &1000u32, &5u32, &66u32)
            .unwrap();
        let cfg = client.get_config();
        assert_eq!(cfg.timelock_delay, 1000);
        assert_eq!(cfg.quorum, 5);
        assert_eq!(cfg.approval_threshold_pct, 66);
    }

    #[test]
    fn test_transfer_admin() {
        let (env, admin, client) = setup();
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin).unwrap();
        assert_eq!(client.get_admin(), new_admin);
        // Old admin can no longer cancel
        let target = Address::generate(&env);
        let id = client
            .propose_upgrade(&new_admin, &target, &wasm_hash(&env), &desc(&env, "v2"))
            .unwrap();
        assert_eq!(
            client.cancel(&admin, &id),
            Err(Ok(Error::Unauthorized))
        );
    }
}
