# Ledger Time vs. Seconds

## How Stellar ledger time works

Stellar closes a new ledger roughly every 5 seconds. This is a statistical
approximation, not a guarantee. The actual interval varies with network
conditions and validator consensus timing. Stellar's protocol documentation
states that ledgers close every 5–7 seconds; 5 seconds is the commonly used
approximation for planning purposes.

This contract uses ledger sequence numbers, not Unix timestamps, for all
time-related checks. `voting_period`, `timelock_delay`, and
`proposal_min_interval` are all specified in ledgers. The fields
`voting_ends_at` and `executable_at` stored on each proposal are ledger
sequence numbers, not Unix timestamps.

Because ledger close times are variable, any real-world duration converted to
ledgers is an approximation. A `voting_period` of 17 280 ledgers is
*approximately* 24 hours, not exactly 24 hours.

## Conversion table

| Ledgers | Approximate wall-clock time |
|--------:|:---------------------------|
| 1 | ≈ 5 seconds |
| 12 | ≈ 1 minute |
| 720 | ≈ 1 hour |
| 17 280 | ≈ 24 hours |
| 120 960 | ≈ 7 days |

These figures use the 5 s/ledger approximation. At 7 s/ledger the same
17 280 ledgers would take about 33.6 hours.

## Where ledger sequence is used in this contract

Every time-based check in `contracts/upgrade-manager/src/governance.rs` reads
`env.ledger().sequence()` and compares it to a stored sequence number:

- `voting_ends_at` — the ledger sequence at which voting closes. Set to
  `created_at + voting_period` when `propose_upgrade` is called. Votes are
  accepted while `ledger.sequence() <= voting_ends_at`; `finalize` is only
  permitted after `voting_ends_at`.
- `executable_at` — the ledger sequence after which execution is permitted.
  Set to `finalize_ledger + timelock_delay` when `finalize` is called. The
  timelock starts when someone calls `finalize`, not when the proposal is
  created.

Both fields are `u32` ledger sequence numbers stored on the `Proposal` struct
in `contracts/upgrade-manager/src/types.rs`. Neither is a Unix timestamp.

## Practical guidance

When choosing `voting_period` and `timelock_delay`, factor in that ledger
times are approximate and may drift toward the slower end of the 5–7 s range.
If you need *at least* 24 hours of voting, use a value somewhat larger than
17 280 — for example, 20 000 ledgers gives a comfortable buffer. For a
governance system with real users, 17 280 (≈ 24 h) to 120 960 (≈ 7 d) is a
reasonable voting period.

The TTL constants in `types.rs` also use this approximation:
`INSTANCE_TTL_BUMP = 518_400` (≈ 30 days) and
`PERSISTENT_TTL_BUMP = 2_592_000` (≈ 150 days).

## See also

- [docs/walkthrough.md](walkthrough.md) — full deployment walkthrough
- [README.md](../README.md) — configuration reference with parameter descriptions
- [Stellar documentation — ledger close time](https://developers.stellar.org/docs/learn/fundamentals/stellar-data-structures/ledgers)
