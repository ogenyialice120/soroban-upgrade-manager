# Pre-written Issues for Stellar Wave

This file contains ready-to-post GitHub issues for the Stellar Wave Program.
Copy each issue into GitHub with the indicated labels and complexity level.

---

## Issue 1 — `good first issue` | Trivial (100 pts)

**Title:** Add `LICENSE` file to the repository

**Body:**
The repository is missing a `LICENSE` file. The `Cargo.toml` declares `license = "MIT"` but there is no corresponding `LICENSE` file at the repository root.

**Task:**
- Add a standard MIT `LICENSE` file at the root of the repository.
- The copyright line should read: `Copyright (c) 2026 ogenyialice120`

**Acceptance criteria:**
- A `LICENSE` file exists at the root.
- It contains a valid MIT license with the correct copyright year and author.

**Labels:** `good first issue`, `Stellar Wave`, `documentation`

---

## Issue 2 — `good first issue` | Trivial (100 pts)

**Title:** Add `.gitignore` file for Rust projects

**Body:**
The repository lacks a `.gitignore` file, which means build artifacts (`target/`, `*.wasm`) and editor files may accidentally be committed.

**Task:**
- Add a `.gitignore` at the repository root suitable for a Rust/Cargo workspace.
- It should at minimum ignore: `target/`, `*.wasm`, `.DS_Store`, and common editor directories (`.idea/`, `.vscode/`).

**Acceptance criteria:**
- A `.gitignore` file exists at the root.
- Running `cargo build` and then `git status` shows no untracked build artifacts.

**Labels:** `good first issue`, `Stellar Wave`, `chore`

---

## Issue 3 — `good first issue` | Trivial (100 pts)

**Title:** Add `get_proposal_count` query function

**Body:**
There is currently no way for callers to know how many proposals have been created without iterating from ID 0. The `ProposalCount` storage key exists but is only used internally.

**Task:**
- Expose a new read-only contract function `get_proposal_count(env: Env) -> u64` that returns the current proposal counter value.
- Add at least one unit test verifying that the count increments correctly after proposals are created.

**Acceptance criteria:**
- `get_proposal_count()` returns `0` on a freshly initialised contract.
- After creating N proposals, it returns N.
- The function is documented with a `///` doc comment.

**Labels:** `good first issue`, `Stellar Wave`, `enhancement`

---

## Issue 4 — Medium (150 pts)

**Title:** Implement voting deadline — close voting at a fixed ledger, not only on `finalize()`

**Body:**
Currently voting stays open indefinitely until someone calls `finalize()`. This is a UX problem: a proposal could accumulate votes long after the expected voting window.

**Task:**
- Add a `voting_period` field to `Config` (in ledgers, e.g. default 7 days ≈ 120 960 ledgers).
- Store a `voting_ends_at` field on `Proposal` (set to `created_at + voting_period` at creation time).
- `cast_vote()` must reject votes once `env.ledger().sequence() > voting_ends_at`.
- `finalize_voting()` must also enforce that the voting window has closed before transitioning the proposal.
- Update `initialize()` and `update_config()` to accept and validate the new `voting_period` parameter.
- Add or update tests to cover the new time-gating logic.

**Acceptance criteria:**
- Votes cast after `voting_ends_at` return `Error::VotingClosed`.
- `finalize_voting()` before `voting_ends_at` returns `Error::VotingClosed`.
- All existing tests continue to pass.
- New tests cover the new behaviour.

**Labels:** `Stellar Wave`, `enhancement`

---

## Issue 5 — Medium (150 pts)

**Title:** Add an `events` module — emit contract events for all state transitions

**Body:**
The contract currently has no event emission. On-chain indexers and off-chain dashboards cannot track proposal state transitions without scanning the full ledger.

**Task:**
- Create `contracts/upgrade-manager/src/events.rs`.
- Define and emit a structured event for each state transition:
  - `proposal_created` — emitted in `create_proposal()`, includes `proposal_id`, `target`, `proposer`.
  - `vote_cast` — emitted in `cast_vote()`, includes `proposal_id`, `voter`, `approve`.
  - `voting_finalized` — emitted in `finalize_voting()`, includes `proposal_id`, `status`.
  - `proposal_executed` — emitted in `execute()`, includes `proposal_id`, `target`, `new_wasm_hash`.
  - `proposal_cancelled` — emitted in `cancel_proposal()`, includes `proposal_id`.
- Use `env.events().publish()` from the Soroban SDK.
- Add the new module to `lib.rs`.
- Add tests that assert events were emitted using `env.events().all()`.

**Acceptance criteria:**
- All five events are emitted at the correct points.
- Event topics and data are documented with `///` comments.
- Tests verify event emission for at least `proposal_created`, `vote_cast`, and `proposal_executed`.

**Labels:** `Stellar Wave`, `enhancement`

---

## Issue 6 — Medium (150 pts)

**Title:** Write a deployment and usage guide (`docs/deployment.md`)

**Body:**
The README covers basic usage but a developer deploying this contract to Testnet or Mainnet for the first time needs a step-by-step guide.

**Task:**
- Create `docs/deployment.md`.
- Cover:
  1. Building the WASM artifact.
  2. Uploading the WASM with `stellar contract upload`.
  3. Deploying the contract instance.
  4. Calling `initialize()` with recommended production parameters.
  5. A worked example: propose → vote → finalize → execute on Testnet, with actual CLI commands.
  6. Troubleshooting common errors (e.g. `AlreadyInitialized`, `TimelockNotExpired`).
- All CLI commands must be correct and copy-pasteable.

**Acceptance criteria:**
- `docs/deployment.md` exists and covers all six sections above.
- CLI commands are tested against Testnet and confirmed to work (or clearly marked as illustrative with placeholder values explained).
- README links to `docs/deployment.md`.

**Labels:** `Stellar Wave`, `documentation`

---

## Issue 7 — High (200 pts)

**Title:** Add token-weighted voting — votes weighted by SEP-41 token balance

**Body:**
The current voting model gives every address exactly one vote. Production governance systems typically weight votes by token balance (e.g. a governance token). This issue implements SEP-41-compatible weighted voting as an **opt-in mode**.

**Task:**
- Add an optional `governance_token: Option<Address>` field to `Config`.
- When `governance_token` is `Some(token_address)`, `cast_vote()` should query the caller's balance via the SEP-41 `balance()` function on that token contract and record the vote weight accordingly.
- When `governance_token` is `None`, retain the existing one-vote-per-address behaviour.
- Update `yes_votes` and `no_votes` on `Proposal` to `i128` to accommodate token amounts (or add separate `yes_weight`/`no_weight` fields — document your design choice).
- Update `finalize_voting()` to use the weighted totals when `governance_token` is set.
- Update `initialize()` and `update_config()` accordingly.
- Add tests using a mock SEP-41 token contract.

**Acceptance criteria:**
- Weighted voting is correctly applied when `governance_token` is configured.
- Unweighted voting is unaffected when `governance_token` is `None`.
- `quorum` and `approval_threshold_pct` work correctly against weighted totals.
- All existing tests pass.
- New tests cover weighted vote accumulation and threshold calculation.
- Design decisions are documented in a `docs/weighted-voting.md` file.

**Labels:** `Stellar Wave`, `enhancement`, `high complexity`

---

## Issue 8 — High (200 pts)

**Title:** Add a TypeScript SDK client for the upgrade manager contract

**Body:**
The contract has no TypeScript bindings, making it difficult to integrate into dApps or scripts. This issue adds a typed SDK client.

**Task:**
- Create a `sdk/` directory at the repository root.
- Generate or hand-write TypeScript bindings using `stellar contract bindings typescript` or manually using `@stellar/stellar-sdk`.
- Export typed functions for all public contract methods: `initialize`, `proposeUpgrade`, `vote`, `finalize`, `execute`, `cancel`, `updateConfig`, `transferAdmin`, and all read-only queries.
- Include a `sdk/README.md` with installation and usage instructions.
- Add a `sdk/package.json` with correct package name, version, and `main`/`types` fields.
- Provide at least one runnable example script (`sdk/examples/propose-and-vote.ts`) targeting Testnet.

**Acceptance criteria:**
- All contract functions are accessible via the SDK with correct TypeScript types.
- The example script runs against Testnet without errors.
- `sdk/README.md` explains how to install and use the SDK.
- The root `README.md` links to the SDK directory.

**Labels:** `Stellar Wave`, `enhancement`, `high complexity`
