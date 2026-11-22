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

use soroban_sdk::{contract, contractimpl, vec, Address, BytesN, Env, String, Vec};

use governance::{
    cancel_proposal, cast_vote, create_proposal, delegate, emit_admin_transferred,
    emit_config_updated, emit_executed, emit_initialized, finalize_voting, invoke_upgrade,
    load_admin, load_config, load_proposal, require_admin, revoke_delegation, save_admin,
    save_config, validate_config,
};
use timelock::{assert_timelock_passed, ledgers_until_executable};
use types::{
    Config, DataKey, Error, Proposal, ProposalStatus, INSTANCE_TTL_BUMP, MAX_PROPOSALS_PAGE_LIMIT,
};

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
    /// - `proposal_min_interval`  — Minimum ledgers between successive proposals from the same address. 0 disables the rate limit.
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
        proposal_min_interval: u32,
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
            proposal_min_interval,
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
        proposal_min_interval: u32,
    ) -> Result<(), Error> {
        require_admin(&env, &caller)?;

        let config = Config {
            voting_period,
            timelock_delay,
            quorum,
            approval_threshold_pct,
            proposal_min_interval,
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
    /// - `title`          — Short title (1–128 bytes).
    /// - `description`    — Human-readable description (1–256 bytes).
    /// - `metadata_hash`  — SHA-256 hash of off-chain metadata (all zeros if none).
    ///
    /// # Returns
    /// The new proposal ID (starts at 0, increments by 1).
    ///
    /// # Errors
    /// - `NotInitialized`    — contract is not yet initialised.
    /// - `EmptyTitle`        — title is empty.
    /// - `TitleTooLong`      — title exceeds 128 bytes.
    /// - `EmptyDescription`  — description is empty.
    /// - `DescriptionTooLong` — description exceeds 256 bytes.
    /// - `Overflow`          — ledger arithmetic overflow.
    pub fn propose_upgrade(
        env: Env,
        proposer: Address,
        target: Address,
        new_wasm_hash: BytesN<32>,
        title: String,
        description: String,
        metadata_hash: BytesN<32>,
    ) -> Result<u64, Error> {
        proposer.require_auth();
        create_proposal(
            &env,
            proposer,
            target,
            new_wasm_hash,
            title,
            description,
            metadata_hash,
        )
    }

    /// Vote on an active proposal.
    ///
    /// The voter must authenticate. Each address may vote exactly once.
    /// Voting is only allowed while ledger.sequence() <= voting_ends_at.
    /// If the voter has delegated their vote, this call fails with
    /// `HasDelegated` — the delegate must vote instead.
    /// If the voter is a delegate, their vote carries the weight of 1
    /// (themselves) plus the number of voters who delegated to them.
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
    /// - `HasDelegated`      — voter has delegated; delegate must vote.
    /// - `AlreadyVoted`      — this address already voted.
    /// - `Overflow`          — vote count arithmetic overflow.
    pub fn vote(env: Env, voter: Address, proposal_id: u64, approve: bool) -> Result<(), Error> {
        voter.require_auth();
        cast_vote(&env, proposal_id, voter, approve)
    }

    /// Delegate `caller`'s voting power to `delegate_to` for all proposals.
    ///
    /// The delegate casts votes on the caller's behalf. The caller cannot
    /// vote directly while delegated. Delegation is global (all proposals)
    /// and not transitive.
    ///
    /// # Parameters
    /// - `caller`     — Address delegating their vote (must sign).
    /// - `delegate_to` — Address receiving the delegated voting power.
    ///
    /// # Errors
    /// - `CannotDelegateToSelf` — caller cannot delegate to themselves.
    /// - `HasDelegated`         — delegate has already delegated (no transitive).
    pub fn delegate(env: Env, caller: Address, delegate_to: Address) -> Result<(), Error> {
        delegate(&env, caller, delegate_to)
    }

    /// Revoke `caller`'s delegation, restoring their ability to vote directly.
    ///
    /// # Errors
    /// - `NotDelegating` — caller has not delegated; nothing to revoke.
    pub fn revoke_delegation(env: Env, caller: Address) -> Result<(), Error> {
        revoke_delegation(&env, caller)
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

    /// Return a paginated list of proposals starting at `offset`, up to `limit` entries.
    ///
    /// Proposals are returned in ascending ID order. If `offset` is greater than or
    /// equal to the total proposal count, an empty vector is returned. If `limit` is 0,
    /// an empty vector is returned. `limit` is capped at `MAX_PROPOSALS_PAGE_LIMIT` (50)
    /// to bound gas costs.
    ///
    /// # Arguments
    /// - `offset` — The starting proposal ID (inclusive).
    /// - `limit`  — Maximum number of proposals to return (capped at 50).
    ///
    /// # Errors
    /// - `NotInitialized` — contract has not been initialised.
    pub fn get_proposals(env: Env, offset: u64, limit: u32) -> Result<Vec<Proposal>, Error> {
        let count = env
            .storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::ProposalCount)
            .unwrap_or(0u64);

        if offset >= count || limit == 0 {
            return Ok(vec![&env]);
        }

        let limit = limit.min(MAX_PROPOSALS_PAGE_LIMIT);
        let end = offset.checked_add(limit as u64).unwrap_or(count).min(count);

        let mut result = Vec::new(&env);
        let mut id = offset;
        while id < end {
            result.push_back(load_proposal(&env, id)?);
            id += 1;
        }

        Ok(result)
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
// Tests — moved to src/test.rs
// ---------------------------------------------------------------------------

// Tests run as a native binary and need std, even though the library
// crate is #![no_std] for the WASM target.
#[cfg(test)]
extern crate std;

#[cfg(test)]
mod test;
