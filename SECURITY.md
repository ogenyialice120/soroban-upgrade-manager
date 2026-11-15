# Security Policy

## Before Reporting

Before submitting a vulnerability report, please review the
[Security Model & Limitations](README.md#%EF%B8%8F-security-model--limitations)
section in `README.md` and [`docs/claims-audit.md`](docs/claims-audit.md) to verify
whether the behavior you observed is already documented as an intentional design
tradeoff or known limitation.

Before filing a report, please also:

1. Verify the issue against the latest `main` branch commit.
2. Confirm that the issue reproduces against the `upgrade-manager` contract
   (`contracts/upgrade-manager`), not solely within mock test harnesses or the
   demonstration `example-target` contract.
3. Run the test suite (`cargo test --workspace`) and construct a minimal
   reproducer (unit test in `contracts/upgrade-manager/src/test.rs`, Stellar CLI
   invocation sequence, or transaction trace) demonstrating the security impact.

---

## Reporting a Vulnerability

If you discover a security vulnerability in `soroban-upgrade-manager`, please
report it privately rather than opening a public GitHub issue.

**Do NOT open a public issue for security vulnerabilities.**

### How to report

1. Go to the [Security tab](https://github.com/ogenyialice120/soroban-upgrade-manager/security) of this repository.
2. Click **"Report a vulnerability"** to use GitHub's private vulnerability reporting.
3. Include a clear description of the vulnerability, the steps to reproduce it,
   and the potential impact.

Alternatively, you may email the maintainers directly if GitHub's private
reporting is unavailable.

### What to include

- A description of the vulnerability and its potential impact.
- Steps to reproduce (code snippet, CLI commands, or transaction details).
- The contract version or commit hash where you found the issue.
- Any suggested mitigations or fixes.

### Response timeline

We will acknowledge your report within 72 hours and provide an initial
assessment within 7 days. We will keep you informed of progress toward a fix
and coordinate a disclosure timeline with you.

---

## What Is in Scope

This policy covers the security of the `soroban-upgrade-manager` smart contract
(`contracts/upgrade-manager`) and its associated verification tooling:

- **In scope:**
  - `contracts/upgrade-manager/src/lib.rs` (contract entry points, authorization, and execution ordering)
  - `contracts/upgrade-manager/src/governance.rs` (proposal lifecycle, voting, quorum/threshold calculation, config snapshots, and admin controls)
  - `contracts/upgrade-manager/src/timelock.rs` (timelock enforcement and readiness calculations)
  - `contracts/upgrade-manager/src/types.rs` (storage keys, data structures, TTL constants, and error codes)
  - `contracts/upgrade-manager/src/test.rs` and `scripts/check_claims.sh` (where a flaw masks a broken security invariant)
- **Out of scope:**
  - `contracts/example-target` — this is a minimal demonstration contract used for automated tests and testnet walkthroughs and is **not** intended for production use.
  - Third-party infrastructure (Stellar RPC nodes, Horizon, or upstream `soroban-sdk` bugs not specific to this contract's usage).

---

## Examples of Valid Reports

The following classes of vulnerabilities in `contracts/upgrade-manager` are valid
and in scope for private reporting:

- **Authentication / authorization bypass:**
  - Calling `initialize`, `cancel`, `update_config`, or `transfer_admin` without valid admin authorization (`require_auth()`).
  - Casting a vote or creating a proposal on behalf of another `Address` without their signature.
  - Executing a cross-contract upgrade on a proposal that has not passed governance (`ProposalStatus::Queued`) or whose `timelock_delay` has not yet elapsed (`ledger.sequence() < executable_at`).
- **Integer overflow or underflow:**
  - Arithmetic overflow/underflow in `voting_ends_at`, `executable_at`, `yes_votes`, `no_votes`, `next_proposal_id`, or quorum/approval percentage calculations (`yes_votes as u64 * 100 >= total_votes as u64 * snap_threshold_pct as u64`) that allows bypassing governance thresholds or bricking proposal state.
- **Reentrancy or state-ordering violations:**
  - Any flaw in `execute()` that violates the checks-effects-interactions pattern (for example, allowing a malicious or reentrant target contract invoked via `invoke_upgrade` to re-enter `upgrade-manager` and execute a proposal multiple times or corrupt state).
- **In-flight proposal snapshot bypass:**
  - Any path where `update_config` retroactively alters `snap_voting_period`, `snap_timelock_delay`, `snap_quorum`, or `snap_threshold_pct` on an already-created proposal.
- **Double-voting or state transition bypass:**
  - Allowing a single `Address` to vote more than once on the same `proposal_id`, or transitioning a `Defeated`, `Cancelled`, or `Executed` proposal back into `Active` or `Queued`.

---

## Examples of Invalid Reports

The following behaviors are **known architectural trade-offs and documented limitations**
(see [Security Model & Limitations](README.md#%EF%B8%8F-security-model--limitations)
in `README.md`) and will **not** be accepted as vulnerability reports:

- **Lack of sybil resistance (one-address-one-vote):**
  - Voting is unweighted (`1 Address = 1 vote`) with no token lock, stake weight, or identity verification. Multi-address sybil voting is a documented limitation.
- **No proposal deposit / proposal spam:**
  - `propose_upgrade` does not require a token deposit or bond; any authenticated address can submit proposals.
- **No proposal expiry:**
  - `Active` proposals do not automatically expire after `voting_ends_at`, and `Queued` proposals do not expire after `executable_at`. An unfinalized proposal can be finalized long after its voting window closes.
- **Timelock starts at `finalize()`, not `propose_upgrade()`:**
  - `executable_at` is computed as `current_ledger + snap_timelock_delay` when `finalize()` is invoked, which is intentional by design.
- **Admin power to cancel `Active` or `Queued` proposals or update config:**
  - The admin role is a documented safety valve capable of calling `cancel` (even after a proposal is `Queued`), `update_config`, and `transfer_admin`. It is recommended that production deployments assign `admin` to a multisig or DAO.
- **Lack of on-chain WASM storage-layout compatibility checks:**
  - `upgrade-manager` does not inspect the contents of `new_wasm_hash` for storage compatibility with the target contract; operators and voters must verify compatibility off-chain prior to voting.
- **Target contract missing `manager.require_auth()`:**
  - Target contracts are responsible for authenticating the upgrade manager address inside their own `upgrade()` entrypoint.

---

## Disclaimer

**This contract is unaudited.** It has not undergone a formal security audit.
Use at your own risk. See the [Security Model & Limitations](README.md#%EF%B8%8F-security-model--limitations)
section in `README.md` for details.
