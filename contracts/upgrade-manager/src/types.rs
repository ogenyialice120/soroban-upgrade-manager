use soroban_sdk::{contracterror, contracttype, Address, BytesN};

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
// TTL constants
// ---------------------------------------------------------------------------

/// Minimum TTL bump applied to instance storage on every state-changing call.
/// 518_400 ledgers ≈ 30 days at ~5 s/ledger.
pub const INSTANCE_TTL_BUMP: u32 = 518_400;

/// Minimum TTL applied to persistent Proposal and Vote entries on write and read.
/// 2_592_000 ledgers ≈ 150 days — long enough to outlive any realistic proposal.
pub const PERSISTENT_TTL_BUMP: u32 = 2_592_000;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Global contract configuration set during initialisation.
///
/// All fields are copied into each `Proposal` at creation time so that
/// `update_config` cannot change the rules of in-flight proposals.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Config {
    /// Ledgers between proposal creation and the end of the voting window.
    /// Must be >= 1. At ~5 s/ledger, 17_280 ledgers ≈ 24 hours.
    pub voting_period: u32,
    /// Minimum ledgers that must pass between `finalize` and `execute`.
    /// At ~5 s/ledger, 17_280 ledgers ≈ 24 hours. Must be >= 1.
    pub timelock_delay: u32,
    /// Minimum number of total votes (YES + NO) needed for a proposal to pass.
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
    /// Ledger sequence at which voting closes (created_at + snap_voting_period).
    /// Votes are accepted while ledger.sequence() <= voting_ends_at.
    /// `finalize` is allowed only after voting_ends_at.
    pub voting_ends_at: u32,
    /// Ledger sequence after which execution is permitted.
    /// Set to finalize_ledger + snap_timelock_delay when finalize() is called.
    /// Zero until the proposal is finalized.
    pub executable_at: u32,
    /// Cumulative YES votes.
    pub yes_votes: u32,
    /// Cumulative NO votes.
    pub no_votes: u32,
    /// Current lifecycle status.
    pub status: ProposalStatus,
    /// Address that created the proposal.
    pub proposer: Address,
    /// Human-readable description (min 1 char, max 256 bytes).
    pub description: soroban_sdk::String,
    // --- Snapshotted governance rules (set at creation, never changed) ---
    /// Snapshot of Config.voting_period at creation time.
    pub snap_voting_period: u32,
    /// Snapshot of Config.timelock_delay at creation time.
    pub snap_timelock_delay: u32,
    /// Snapshot of Config.quorum at creation time.
    pub snap_quorum: u32,
    /// Snapshot of Config.approval_threshold_pct at creation time.
    pub snap_threshold_pct: u32,
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
    /// Note: finalize() returns ProposalStatus::Defeated rather than this error.
    ProposalDefeated = 8,
    /// Provided description exceeds the 256-byte limit.
    DescriptionTooLong = 9,
    /// approval_threshold_pct must be between 1 and 100.
    InvalidThreshold = 10,
    /// timelock_delay must be at least 1 ledger.
    InvalidTimelockDelay = 11,
    /// quorum must be at least 1.
    InvalidQuorum = 12,
    /// Proposal is already Executed, Cancelled, or Defeated; cannot change state.
    ProposalFinalized = 13,
    /// Contract has not been initialised; call initialize() first.
    NotInitialized = 14,
    /// Arithmetic overflow in ledger or vote calculation.
    Overflow = 15,
    /// The voting period for this proposal has ended; no more votes accepted.
    VotingPeriodEnded = 16,
    /// The voting period for this proposal has not ended yet; finalize is not allowed.
    VotingStillActive = 17,
    /// Description must not be empty.
    EmptyDescription = 18,
    /// voting_period must be at least 1 ledger.
    InvalidVotingPeriod = 19,
}
