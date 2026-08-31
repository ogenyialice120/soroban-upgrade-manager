use soroban_sdk::{contracttype, contracterror, Address, BytesN};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract admin (can cancel proposals, update config)
    Admin,
    /// Global configuration
    Config,
    /// Proposal by ID
    Proposal(u64),
    /// Next proposal ID counter
    ProposalCount,
    /// Vote cast by a voter on a proposal: (proposal_id, voter)
    Vote(u64, Address),
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Global contract configuration set during initialisation.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Config {
    /// Minimum ledgers that must pass between proposal creation and execution.
    /// At ~5 s/ledger, 17_280 ledgers ≈ 24 hours.
    pub timelock_delay: u32,
    /// Minimum number of YES votes needed for a proposal to pass.
    pub quorum: u32,
    /// Votes are weighted equally (1 per address); this is the threshold of
    /// yes/(yes+no) * 100 that must be reached (e.g. 51 = simple majority).
    pub approval_threshold_pct: u32,
}

// ---------------------------------------------------------------------------
// Proposal
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    /// Open for voting.
    Active,
    /// Voting passed; waiting for timelock to expire.
    Queued,
    /// Timelock expired and upgrade was executed.
    Executed,
    /// Proposal was cancelled by the admin.
    Cancelled,
    /// Voting period ended without reaching quorum/threshold.
    Defeated,
}

/// A pending or historical upgrade proposal.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    /// Auto-incremented identifier.
    pub id: u64,
    /// Address of the contract to be upgraded.
    pub target: Address,
    /// Hash of the new WASM bytecode already uploaded to the network.
    pub new_wasm_hash: BytesN<32>,
    /// Ledger sequence at which the proposal was created.
    pub created_at: u32,
    /// Ledger sequence after which execution is permitted (created_at + timelock_delay).
    pub executable_at: u32,
    /// Cumulative YES votes.
    pub yes_votes: u32,
    /// Cumulative NO votes.
    pub no_votes: u32,
    /// Current lifecycle status.
    pub status: ProposalStatus,
    /// Address that created the proposal.
    pub proposer: Address,
    /// Optional human-readable description (max 256 chars enforced in lib.rs).
    pub description: soroban_sdk::String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// Contract has already been initialised.
    AlreadyInitialized = 1,
    /// Caller is not the admin.
    Unauthorized = 2,
    /// The requested proposal does not exist.
    ProposalNotFound = 3,
    /// Proposal is not in the Active state; voting is closed.
    VotingClosed = 4,
    /// This address has already voted on the proposal.
    AlreadyVoted = 5,
    /// Proposal is not in the Queued state.
    NotQueued = 6,
    /// The timelock period has not yet elapsed.
    TimelockNotExpired = 7,
    /// Proposal did not reach the required quorum or approval threshold.
    ProposalDefeated = 8,
    /// Provided description exceeds the 256-character limit.
    DescriptionTooLong = 9,
    /// approval_threshold_pct must be between 1 and 100.
    InvalidThreshold = 10,
    /// timelock_delay must be at least 1 ledger.
    InvalidTimelockDelay = 11,
    /// quorum must be at least 1.
    InvalidQuorum = 12,
    /// Proposal is already cancelled or executed; cannot change state.
    ProposalFinalized = 13,
}
