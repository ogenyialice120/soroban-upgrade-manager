use soroban_sdk::{symbol_short, vec, Address, BytesN, Env, String};

use crate::types::{
    Config, DataKey, Error, Proposal, ProposalStatus, INSTANCE_TTL_BUMP, PERSISTENT_TTL_BUMP,
};

// ---------------------------------------------------------------------------
// Internal helpers — config & admin
// ---------------------------------------------------------------------------

/// Load the global config.
///
/// # Errors
/// - `NotInitialized` if the contract has not been initialised.
pub fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

/// Persist the global config and bump instance TTL.
pub fn save_config(env: &Env, config: &Config) {
    env.storage().instance().set(&DataKey::Config, config);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_BUMP, INSTANCE_TTL_BUMP);
}

/// Load the admin address.
///
/// # Errors
/// - `NotInitialized` if the contract has not been initialised.
pub fn load_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

/// Persist the admin address and bump instance TTL.
pub fn save_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_BUMP, INSTANCE_TTL_BUMP);
}

// ---------------------------------------------------------------------------
// Internal helpers — auth
// ---------------------------------------------------------------------------

/// Require that `caller` is authenticated and equals the stored admin.
///
/// # Errors
/// - `NotInitialized` if no admin has been stored yet.
/// - `Unauthorized`   if `caller` is not the admin.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), Error> {
    caller.require_auth();
    let admin = load_admin(env)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers — config validation
// ---------------------------------------------------------------------------

/// Validate all config fields, returning the appropriate error on failure.
///
/// # Errors
/// - `InvalidVotingPeriod`  — `voting_period` is 0.
/// - `InvalidTimelockDelay` — `timelock_delay` is 0.
/// - `InvalidQuorum`        — `quorum` is 0.
/// - `InvalidThreshold`     — `approval_threshold_pct` is 0 or > 100.
pub fn validate_config(config: &Config) -> Result<(), Error> {
    if config.voting_period == 0 {
        return Err(Error::InvalidVotingPeriod);
    }
    if config.timelock_delay == 0 {
        return Err(Error::InvalidTimelockDelay);
    }
    if config.quorum == 0 {
        return Err(Error::InvalidQuorum);
    }
    if config.approval_threshold_pct == 0 || config.approval_threshold_pct > 100 {
        return Err(Error::InvalidThreshold);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers — proposal & vote storage
// ---------------------------------------------------------------------------

/// Fetch a proposal by ID, returning `Error::ProposalNotFound` if absent.
/// Extends the persistent TTL for the entry on read.
pub fn load_proposal(env: &Env, proposal_id: u64) -> Result<Proposal, Error> {
    let key = DataKey::Proposal(proposal_id);
    let proposal: Proposal = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(Error::ProposalNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
    Ok(proposal)
}

/// Persist a proposal and extend its persistent TTL.
pub fn save_proposal(env: &Env, proposal: &Proposal) {
    let key = DataKey::Proposal(proposal.id);
    env.storage().persistent().set(&key, proposal);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
    // Also bump instance storage (ProposalCount lives there).
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_BUMP, INSTANCE_TTL_BUMP);
}

/// Return the current proposal counter and increment it atomically.
fn next_proposal_id(env: &Env) -> Result<u64, Error> {
    let key = DataKey::ProposalCount;
    let current: u64 = env.storage().instance().get(&key).unwrap_or(0u64);
    let next = current.checked_add(1).ok_or(Error::Overflow)?;
    env.storage().instance().set(&key, &next);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_BUMP, INSTANCE_TTL_BUMP);
    Ok(current)
}

/// Check whether `voter` has already voted on `proposal_id`.
pub fn has_voted(env: &Env, proposal_id: u64, voter: &Address) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Vote(proposal_id, voter.clone()))
}

/// Record that `voter` has voted on `proposal_id` and extend persistent TTL.
pub fn record_vote(env: &Env, proposal_id: u64, voter: &Address) {
    let key = DataKey::Vote(proposal_id, voter.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_BUMP, PERSISTENT_TTL_BUMP);
}

// ---------------------------------------------------------------------------
// Governance actions
// ---------------------------------------------------------------------------

/// Create a new upgrade proposal. Returns the new proposal ID.
///
/// Copies the current config rules into the proposal so that `update_config`
/// cannot affect this proposal's outcome.
///
/// # Errors
/// - `NotInitialized`   if the contract is not yet initialised.
/// - `EmptyDescription` if `description` is empty.
/// - `DescriptionTooLong` if `description` > 256 bytes.
/// - `Overflow`         if ledger arithmetic overflows.
pub fn create_proposal(
    env: &Env,
    proposer: Address,
    target: Address,
    new_wasm_hash: BytesN<32>,
    description: String,
) -> Result<u64, Error> {
    // Validate description length (Soroban String::len returns u32 bytes).
    if description.is_empty() {
        return Err(Error::EmptyDescription);
    }
    if description.len() > 256 {
        return Err(Error::DescriptionTooLong);
    }

    let config = load_config(env)?;
    let current_ledger = env.ledger().sequence();

    // Snapshot the rules at creation time.
    let voting_ends_at = current_ledger
        .checked_add(config.voting_period)
        .ok_or(Error::Overflow)?;

    let id = next_proposal_id(env)?;

    let proposal = Proposal {
        id,
        target: target.clone(),
        new_wasm_hash: new_wasm_hash.clone(),
        created_at: current_ledger,
        voting_ends_at,
        executable_at: 0, // set in finalize_voting
        yes_votes: 0,
        no_votes: 0,
        status: ProposalStatus::Active,
        proposer: proposer.clone(),
        description: description.clone(),
        snap_voting_period: config.voting_period,
        snap_timelock_delay: config.timelock_delay,
        snap_quorum: config.quorum,
        snap_threshold_pct: config.approval_threshold_pct,
    };

    save_proposal(env, &proposal);

    // Emit "proposal_created" event.
    env.events().publish(
        (symbol_short!("proposal"), symbol_short!("created")),
        (id, proposer, target, new_wasm_hash),
    );

    Ok(id)
}

/// Cast a vote on an active proposal.
///
/// # Errors
/// - `ProposalNotFound`   if the ID is unknown.
/// - `VotingClosed`       if the proposal is not Active.
/// - `VotingPeriodEnded`  if the voting period has ended.
/// - `AlreadyVoted`       if `voter` already voted.
/// - `Overflow`           if vote arithmetic overflows.
pub fn cast_vote(env: &Env, proposal_id: u64, voter: Address, approve: bool) -> Result<(), Error> {
    let mut proposal = load_proposal(env, proposal_id)?;

    if proposal.status != ProposalStatus::Active {
        return Err(Error::VotingClosed);
    }

    // Voting is only allowed while the voting period is open.
    if env.ledger().sequence() > proposal.voting_ends_at {
        return Err(Error::VotingPeriodEnded);
    }

    if has_voted(env, proposal_id, &voter) {
        return Err(Error::AlreadyVoted);
    }

    if approve {
        proposal.yes_votes = proposal.yes_votes.checked_add(1).ok_or(Error::Overflow)?;
    } else {
        proposal.no_votes = proposal.no_votes.checked_add(1).ok_or(Error::Overflow)?;
    }

    record_vote(env, proposal_id, &voter);
    save_proposal(env, &proposal);

    // Emit "vote_cast" event.
    env.events().publish(
        (symbol_short!("vote"), symbol_short!("cast")),
        (proposal_id, voter, approve),
    );

    Ok(())
}

/// Close voting on a proposal and move it to Queued or Defeated.
///
/// May be called by anyone once the voting period has ended.
/// On pass: status becomes Queued and executable_at is set to
/// `current_ledger + snapshot_timelock_delay`.
/// On fail: status becomes Defeated.
///
/// # Errors
/// - `ProposalNotFound`   — unknown proposal ID.
/// - `VotingClosed`       — proposal is not Active.
/// - `VotingStillActive`  — the voting period has not ended yet.
/// - `Overflow`           — ledger arithmetic overflow.
pub fn finalize_voting(env: &Env, proposal_id: u64) -> Result<ProposalStatus, Error> {
    let mut proposal = load_proposal(env, proposal_id)?;

    if proposal.status != ProposalStatus::Active {
        return Err(Error::VotingClosed);
    }

    let current_ledger = env.ledger().sequence();

    // Finalize is only allowed once the voting period has ended.
    if current_ledger <= proposal.voting_ends_at {
        return Err(Error::VotingStillActive);
    }

    let total_votes = proposal
        .yes_votes
        .checked_add(proposal.no_votes)
        .ok_or(Error::Overflow)?;

    let passed = total_votes >= proposal.snap_quorum
        && (proposal.yes_votes as u64 * 100)
            >= (total_votes as u64 * proposal.snap_threshold_pct as u64);

    if passed {
        // Timelock starts NOW (at finalize time), not at creation.
        let executable_at = current_ledger
            .checked_add(proposal.snap_timelock_delay)
            .ok_or(Error::Overflow)?;
        proposal.executable_at = executable_at;
        proposal.status = ProposalStatus::Queued;
    } else {
        proposal.status = ProposalStatus::Defeated;
    }

    save_proposal(env, &proposal);

    // Emit "finalized" event.
    env.events().publish(
        (symbol_short!("proposal"), symbol_short!("finalized")),
        (proposal_id, proposal.status.clone()),
    );

    Ok(proposal.status)
}

/// Cancel an Active or Queued proposal. Only callable by the admin.
///
/// # Errors
/// - `NotInitialized`    if the contract has not been initialised.
/// - `Unauthorized`      if the caller is not the admin.
/// - `ProposalNotFound`  if the ID is unknown.
/// - `ProposalFinalized` if the proposal is already Executed, Cancelled, or Defeated.
pub fn cancel_proposal(env: &Env, caller: &Address, proposal_id: u64) -> Result<(), Error> {
    require_admin(env, caller)?;

    let mut proposal = load_proposal(env, proposal_id)?;

    match proposal.status {
        ProposalStatus::Executed | ProposalStatus::Cancelled | ProposalStatus::Defeated => {
            return Err(Error::ProposalFinalized)
        }
        _ => {}
    }

    proposal.status = ProposalStatus::Cancelled;
    save_proposal(env, &proposal);

    // Emit "cancelled" event.
    env.events().publish(
        (symbol_short!("proposal"), symbol_short!("cancelled")),
        (proposal_id, caller.clone()),
    );

    Ok(())
}

/// Emit the "initialized" event. Called once from lib.rs initialize().
pub fn emit_initialized(env: &Env, admin: &Address) {
    env.events().publish(
        (symbol_short!("contract"), symbol_short!("init")),
        admin.clone(),
    );
}

/// Emit the "config_updated" event.
pub fn emit_config_updated(env: &Env, caller: &Address) {
    env.events().publish(
        (symbol_short!("config"), symbol_short!("updated")),
        caller.clone(),
    );
}

/// Emit the "admin_transferred" event.
pub fn emit_admin_transferred(env: &Env, old_admin: &Address, new_admin: &Address) {
    env.events().publish(
        (symbol_short!("admin"), symbol_short!("transfer")),
        (old_admin.clone(), new_admin.clone()),
    );
}

/// Emit the "executed" event.
pub fn emit_executed(env: &Env, proposal_id: u64) {
    env.events().publish(
        (symbol_short!("proposal"), symbol_short!("executed")),
        proposal_id,
    );
}

/// Call target.upgrade(new_wasm_hash) via cross-contract invocation.
/// Should be called by execute() after setting proposal status to Executed.
pub fn invoke_upgrade(env: &Env, target: &Address, new_wasm_hash: &BytesN<32>) {
    env.invoke_contract::<()>(
        target,
        &symbol_short!("upgrade"),
        vec![env, new_wasm_hash.to_val()],
    );
}
