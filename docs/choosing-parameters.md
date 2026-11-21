# Choosing Governance Parameters

This guide explains how to choose values for the four governance parameters
configured at initialization. Each parameter directly shapes how hard it is to
pass, delay, or block an upgrade. Good values depend on your deployment context.

---

## Ledger-to-time reference

All time-based parameters are measured in ledgers. At the documented rate of
approximately 5 seconds per ledger:

| Duration | Ledger count |
|---|---|
| 1 minute | 12 |
| 1 hour | 720 |
| 6 hours | 4 320 |
| 12 hours | 8 640 |
| 24 hours | 17 280 |
| 48 hours | 34 560 |
| 7 days | 120 960 |
| 14 days | 241 920 |
| 30 days | 518 400 |

**Verification:** 3 600 s/hour ÷ 5 s/ledger = 720 ledgers/hour.
17 280 ÷ 720 = 24 hours. 120 960 ÷ 720 = 168 hours = 7 days.

The 5 s/ledger rate is an approximation. Actual ledger close times vary
slightly with network conditions. For timing-critical deployments, monitor the
actual close rate before choosing values.

---

## The four parameters

### `voting_period`

**What it is:** The number of ledgers after proposal creation during which votes
are accepted. Votes are allowed while `ledger.sequence() <= voting_ends_at`.
After the voting period ends, no new votes can be cast and `finalize()` becomes
callable.

**Minimum:** 1 ledger.

**Trade-off:** A short voting period forces voters to act quickly, which is
practical for a small trusted team but excludes participants who are offline or
unaware of the proposal. A long voting period gives every stakeholder time to
review and vote, but slows down the upgrade process and may leave proposals
open for so long that they become stale.

**Rule of thumb:** Match the voting period to the realistic availability of
your voter set. A team that checks in daily can use 24 hours (17 280 ledgers).
A broad community with global time zones needs at least 7 days (120 960 ledgers).

---

### `timelock_delay`

**What it is:** The number of ledgers between `finalize()` and the earliest
permitted `execute()`. The delay starts when someone calls `finalize()`, not
when the proposal was created or when voting ended. `executable_at` is set to
`finalize_ledger + timelock_delay`.

**Minimum:** 1 ledger.

**Trade-off:** A longer timelock gives users more time to review the upgrade,
verify the WASM hash, and exit if they disagree. A shorter timelock speeds up
deployment and reduces the window during which the admin can cancel an already
approved proposal (the admin can cancel Queued proposals during the timelock).
Setting the timelock too short defeats its purpose: a 1-ledger timelock gives
users about 5 seconds of warning.

**Rule of thumb:** Match the timelock to the complexity and risk of the upgrade.
Routine parameter changes may need only a few hours. Upgrades that change
storage layout or core logic should give users 24–48 hours minimum.

---

### `quorum`

**What it is:** The minimum total number of votes (YES + NO combined) that must
be cast for a proposal to be considered. A proposal that does not reach quorum
is Defeated regardless of the YES/NO ratio.

**Minimum:** 1 vote.

**Trade-off:** A low quorum (e.g., 1 or 2) makes proposals easy to pass, which
is convenient for a small team but vulnerable to a single address pushing through
an upgrade unilaterally. A high quorum requires broad participation but risks
proposals being stuck if voter turnout is low.

**Important:** Because voting is one-address-one-vote with no sybil resistance
(see below), quorum counts distinct addresses, not any measure of economic
stake. An attacker who controls multiple addresses can reach any quorum value.
Quorum is most useful as a signal of genuine community engagement in honest
deployments.

---

### `approval_threshold_pct`

**What it is:** The minimum percentage of YES votes out of total votes
(YES + NO) required for a proposal to pass. A proposal where
`yes_votes / (yes_votes + no_votes) * 100 < approval_threshold_pct` is Defeated.

**Range:** 1–100 (inclusive).

**Trade-off:** A low threshold (e.g., 51) allows a simple majority to approve.
A high threshold (e.g., 75 or 90) requires a strong supermajority, which
makes it harder to pass controversial upgrades but also harder to pass
uncontested ones. A threshold of 100 requires unanimous approval, which is
impractical for large groups.

**Note:** The contract computes the threshold as
`yes_votes * 100 >= total_votes * approval_threshold_pct` using integer
arithmetic, so the threshold is a floor: at 51%, an upgrade with 51 YES
and 49 NO passes (51 × 100 = 5100 ≥ 100 × 51 = 5100).

---

## Scenario examples

### Small trusted team (3–5 members)

A team of developers who all participate actively and trust each other. Speed
matters more than extensive public review.

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- initialize \
  --admin "$ADMIN_ADDR" \
  --voting_period 720 \
  --timelock_delay 720 \
  --quorum 2 \
  --approval_threshold_pct 51
```

| Parameter | Value | Reasoning |
|---|---|---|
| `voting_period` | 720 (≈ 1 hour) | Small team, available to respond quickly |
| `timelock_delay` | 720 (≈ 1 hour) | Short delay is acceptable; team reviews before proposing |
| `quorum` | 2 | At least 2 of the 3–5 team members must vote |
| `approval_threshold_pct` | 51 | Simple majority; ties go against the proposal |

**Risk:** If any two team members collude, they can pass any proposal. If the
admin key is compromised, the attacker can cancel approved proposals during the
1-hour timelock. Mitigate by using a 2-of-3 multisig as admin.

---

### Public DAO (100+ participants)

A community-governed contract where many independent addresses participate.
Broad participation and meaningful review time are more important than speed.

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- initialize \
  --admin "$ADMIN_ADDR" \
  --voting_period 120960 \
  --timelock_delay 34560 \
  --quorum 10 \
  --approval_threshold_pct 66
```

| Parameter | Value | Reasoning |
|---|---|---|
| `voting_period` | 120 960 (≈ 7 days) | Gives all community members time to review and vote across time zones |
| `timelock_delay` | 34 560 (≈ 48 hours) | 48 hours for users to verify the WASM hash and exit if needed |
| `quorum` | 10 | Requires meaningful participation; prevents a handful of addresses from passing proposals silently |
| `approval_threshold_pct` | 66 | Supermajority; contentious upgrades require clear consensus |

**Risk:** Low voter turnout can stall governance. Consider whether your
community reliably reaches the quorum in practice. Start with a low quorum
and raise it once participation patterns are established.

---

### Emergency upgrade path

A secondary emergency path for critical security fixes where the standard
7-day cycle is too slow. This requires a separate, pre-authorized set of
trusted responders acting as the voter set.

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- initialize \
  --admin "$ADMIN_ADDR" \
  --voting_period 720 \
  --timelock_delay 1440 \
  --quorum 3 \
  --approval_threshold_pct 51
```

| Parameter | Value | Reasoning |
|---|---|---|
| `voting_period` | 720 (≈ 1 hour) | Security incident demands fast response |
| `timelock_delay` | 1 440 (≈ 2 hours) | Minimal but nonzero delay for a final sanity check |
| `quorum` | 3 | Requires 3 trusted responders to agree; prevents a single compromised key from pushing an upgrade |
| `approval_threshold_pct` | 51 | Simple majority among the response team |

**Important:** An emergency path with low quorum and a short timelock is only
safe if the voter set is small and trusted. If the voter addresses are publicly
known, an attacker who compromises two of them can push any upgrade in under
3 hours. Restrict this manager instance to a known, well-secured set of
addresses. The admin role for this instance should be held by a cold multisig.

---

## The sybil-resistance limitation

Voting is **one-address-one-vote** with no stake, token weight, or identity
requirement. This means:

- Any address can vote; there is no way to distinguish a human voter from an
  attacker who generated 1 000 addresses.
- Quorum and threshold values count addresses, not any measure of economic
  interest. A quorum of 10 can be satisfied by a single person who controls
  10 addresses.
- The contract cannot prevent a motivated attacker from reaching any quorum
  value by generating enough addresses.

**What this means for parameter choice:**

- Do not rely on quorum alone as a security gate. High quorum values are only
  meaningful if you have confidence that your voter set is genuine (for example,
  a small team of identified developers, or a system that uses an external
  identity layer).
- The timelock delay is your most reliable safety mechanism. It gives everyone
  time to notice and react to a suspicious proposal, regardless of how votes
  were cast.
- For high-value contracts, consider transferring the admin role to a multisig
  wallet. The admin can cancel Queued proposals during the timelock, providing
  a last-resort block against fraudulent upgrades.

See the [Security Model & Limitations](../README.md#%EF%B8%8F-security-model--limitations)
section in the README for the full discussion.

---

## Parameters and validation rules

The contract enforces the following hard minimums. Any value outside these
ranges returns an error at initialization or when calling `update_config`.

| Parameter | Valid range | Error if violated |
|---|---|---|
| `voting_period` | >= 1 | `InvalidVotingPeriod` |
| `timelock_delay` | >= 1 | `InvalidTimelockDelay` |
| `quorum` | >= 1 | `InvalidQuorum` |
| `approval_threshold_pct` | 1–100 | `InvalidThreshold` |

There is no maximum for `voting_period`, `timelock_delay`, or `quorum`, but
extremely large values can make governance unusable in practice.

---

## Changing parameters after deployment

You can update all four parameters at any time by calling `update_config` as
the admin:

```bash
stellar contract invoke \
  --id "$MANAGER_ID" \
  --source admin \
  --network testnet \
  -- update_config \
  --caller "$ADMIN_ADDR" \
  --voting_period 120960 \
  --timelock_delay 34560 \
  --quorum 5 \
  --approval_threshold_pct 51
```

**Important:** Config changes only affect proposals created after the update.
Each proposal snapshots the governance rules at creation time. In-flight
proposals continue under the rules that were in effect when they were created.
This means you can safely raise the quorum or extend the voting period without
retroactively affecting active proposals.

---

## See also

- [docs/walkthrough.md](walkthrough.md) — full deployment walkthrough using
  `voting_period=17280` and `quorum=3`
- [README.md — Configuration Reference](../README.md#-configuration-reference) — parameter types and minimums
- [README.md — Security Model & Limitations](../README.md#%EF%B8%8F-security-model--limitations) — sybil resistance and admin key risks
