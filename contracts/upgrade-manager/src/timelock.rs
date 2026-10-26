use soroban_sdk::Env;

use crate::types::{Error, Proposal, ProposalStatus};

// ---------------------------------------------------------------------------
// Timelock enforcement
// ---------------------------------------------------------------------------

/// Verify that a proposal is Queued and that the timelock delay has elapsed.
///
/// Returns `Ok(())` when both conditions are met; otherwise returns the
/// appropriate `Error` variant so the caller can surface it cleanly.
///
/// # Errors
/// - `NotQueued`           — proposal is not in the Queued state.
/// - `TimelockNotExpired`  — the current ledger is before `executable_at`.
pub fn assert_timelock_passed(env: &Env, proposal: &Proposal) -> Result<(), Error> {
    if proposal.status != ProposalStatus::Queued {
        return Err(Error::NotQueued);
    }

    if env.ledger().sequence() < proposal.executable_at {
        return Err(Error::TimelockNotExpired);
    }

    Ok(())
}

/// Return how many ledgers remain until the timelock expires.
///
/// Returns `0` if the proposal is not Queued, or if execution is already
/// permitted (current ledger >= executable_at).
pub fn ledgers_until_executable(env: &Env, proposal: &Proposal) -> u32 {
    if proposal.status != ProposalStatus::Queued {
        return 0;
    }
    let current = env.ledger().sequence();
    proposal.executable_at.saturating_sub(current)
}
