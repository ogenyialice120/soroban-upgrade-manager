use soroban_sdk::{Address, BytesN, Env, String};

use crate::types::{Config, DataKey, Error, Proposal, ProposalStatus};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Load the global config. Panics if contract is uninitialised.
pub fn load_config(env: &Env) -> Config {
    env.storage().instance().get(&DataKey::Config).unwrap()
}

/// Persist the global config.
pub fn save_config(env: &Env, config: &Config) {
    env.storage().instance().set(&DataKey::Config, config);
}

/// Load the admin address. Panics if contract is uninitialised.
pub fn load_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

/// Persist the admin address.
pub fn save_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Fetch a proposal by ID, returning `Error::ProposalNotFound` if absent.
pub fn load_proposal(env: &Env, proposal_id: u64) -> Result<Proposal, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(proposal_id))
        .ok_or(Error::ProposalNotFound)
}

/// Persist a proposal.
pub fn save_proposal(env: &Env, proposal: &Proposal) {
    env.storage()
        .persistent()
        .set(&DataKey::Proposal(proposal.id), proposal);
}

/// Return the current proposal counter and increment it atomically.
fn next_proposal_id(env: &Env) -> u64 {
    let key = DataKey::ProposalCount;
    let current: u64 = env.storage().instance().get(&key).unwrap_or(0u64);
    env.storage().instance().set(&key, &(current + 1));
    current
}

/// Check whether `voter` has already voted on `proposal_id`.
pub fn has_voted(env: &Env, proposal_id: u64, voter: &Address) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Vote(proposal_id, voter.clone()))
}

/// Record that `voter` has voted on `proposal_id`.
pub fn record_vote(env: &Env, proposal_id: u64, voter: &Address) {
    env.storage()
        .persistent()
        .set(&DataKey::Vote(proposal_id, voter.clone()), &true);
}

// ---------------------------------------------------------------------------
// Governance actions
// ---------------------------------------------------------------------------

/// Create a new upgrade proposal. Returns the new proposal ID.
///
/// # Errors
/// - `DescriptionTooLong` if `description` > 256 characters.
pub fn create_proposal(
    env: &Env,
    proposer: Address,
    target: Address,
    new_wasm_hash: BytesN<32>,
    description: String,
) -> Result<u64, Error> {
    // Validate description length (Soroban String::len returns u32 bytes).
    if description.len() > 256 {
        return Err(Error::DescriptionTooLong);
    }

    let config = load_config(env);
    let current_ledger = env.ledger().sequence();
    let executable_at = current_ledger + config.timelock_delay;

    let id = next_proposal_id(env);

    let proposal = Proposal {
        id,
        target,
        new_wasm_hash,
        created_at: current_ledger,
        executable_at,
        yes_votes: 0,
        no_votes: 0,
        status: ProposalStatus::Active,
        proposer,
        description,
    };

    save_proposal(env, &proposal);
    Ok(id)
}

/// Cast a vote on an active proposal.
///
/// # Errors
/// - `ProposalNotFound` if the ID is unknown.
/// - `VotingClosed` if the proposal is not Active.
/// - `AlreadyVoted` if `voter` already voted.
pub fn cast_vote(
    env: &Env,
    proposal_id: u64,
    voter: Address,
    approve: bool,
) -> Result<(), Error> {
    let mut proposal = load_proposal(env, proposal_id)?;

    if proposal.status != ProposalStatus::Active {
        return Err(Error::VotingClosed);
    }

    if has_voted(env, proposal_id, &voter) {
        return Err(Error::AlreadyVoted);
    }

    if approve {
        proposal.yes_votes += 1;
    } else {
        proposal.no_votes += 1;
    }

    record_vote(env, proposal_id, &voter);
    save_proposal(env, &proposal);
    Ok(())
}

/// Close voting on a proposal and move it to Queued or Defeated.
///
/// Anyone can call this once the timelock delay has passed (which implies
/// the voting window is also over — the timelock starts at creation time,
/// so callers should wait until `executable_at` before calling).
///
/// # Errors
/// - `ProposalNotFound` / `VotingClosed` if the proposal is not Active.
/// - `TimelockNotExpired` if called before `executable_at`.
pub fn finalize_voting(env: &Env, proposal_id: u64) -> Result<ProposalStatus, Error> {
    let mut proposal = load_proposal(env, proposal_id)?;

    if proposal.status != ProposalStatus::Active {
        return Err(Error::VotingClosed);
    }

    if env.ledger().sequence() < proposal.executable_at {
        return Err(Error::TimelockNotExpired);
    }

    let config = load_config(env);

    let total_votes = proposal.yes_votes + proposal.no_votes;
    let passed = total_votes >= config.quorum
        && (proposal.yes_votes as u64 * 100)
            >= (total_votes as u64 * config.approval_threshold_pct as u64);

    proposal.status = if passed {
        ProposalStatus::Queued
    } else {
        ProposalStatus::Defeated
    };

    save_proposal(env, &proposal);
    Ok(proposal.status)
}

/// Cancel an Active or Queued proposal. Only callable by the admin.
///
/// # Errors
/// - `Unauthorized` if the caller is not the admin.
/// - `ProposalNotFound` if the ID is unknown.
/// - `ProposalFinalized` if the proposal is already Executed or Cancelled.
pub fn cancel_proposal(env: &Env, caller: Address, proposal_id: u64) -> Result<(), Error> {
    let admin = load_admin(env);
    if caller != admin {
        return Err(Error::Unauthorized);
    }

    let mut proposal = load_proposal(env, proposal_id)?;

    match proposal.status {
        ProposalStatus::Executed | ProposalStatus::Cancelled => {
            return Err(Error::ProposalFinalized)
        }
        _ => {}
    }

    proposal.status = ProposalStatus::Cancelled;
    save_proposal(env, &proposal);
    Ok(())
}
