# Contributing to Soroban Upgrade Manager

Thank you for your interest in contributing! This project participates in the **[Stellar Wave Program](https://www.drips.network/wave/stellar)**, where contributors earn rewards for merged pull requests.

---

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [How to Contribute](#how-to-contribute)
- [Stellar Wave Contributors](#stellar-wave-contributors)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Pull Request Process](#pull-request-process)
- [Issue Labels](#issue-labels)

---

## Code of Conduct

Be respectful and constructive. We follow the [Contributor Covenant](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) code of conduct.

---

## Getting Started

1. **Fork** the repository on GitHub.
2. **Clone** your fork locally:
   ```bash
   git clone https://github.com/<your-username>/soroban-upgrade-manager.git
   cd soroban-upgrade-manager
   ```
3. **Add the upstream remote**:
   ```bash
   git remote add upstream https://github.com/ogenyialice120/soroban-upgrade-manager.git
   ```
4. **Install dependencies** (see [Development Setup](#development-setup)).
5. **Create a branch** for your work:
   ```bash
   git checkout -b feat/your-feature-name
   ```

---

## How to Contribute

### Reporting Bugs

Open an issue with:
- A clear title and description
- Steps to reproduce
- Expected vs actual behaviour
- Relevant error messages or logs

### Suggesting Features

Open an issue with the `enhancement` label. Describe the problem you want to solve and your proposed solution before writing any code — this avoids wasted effort.

### Fixing Issues

1. Find an open issue (see [Issue Labels](#issue-labels)).
2. Comment on the issue to signal your intent.
3. Create a PR once your work is ready (see [Pull Request Process](#pull-request-process)).

---

## Stellar Wave Contributors

This repo participates in the Stellar Wave Program. During an active Wave:

1. **Apply** for an issue at [drips.network/wave/stellar](https://www.drips.network/wave/stellar).
2. **Wait for assignment** — a maintainer will assign you on GitHub or through the Drips dashboard.
3. **Work on the issue** within the Wave window (typically one week).
4. **Submit a PR** and link it to the issue.
5. **Earn Points** when your PR is merged and the issue is resolved.

Points translate to rewards funded by the Stellar Development Foundation.

> Issues tagged `Stellar Wave` are part of the active Wave cycle. Issues tagged `good first issue` are beginner-friendly and worth starting with.

---

## Development Setup

### Prerequisites

| Tool | Version | Install |
|---|---|---|
| Rust | 1.74+ | [rustup.rs](https://rustup.rs/) |
| wasm32 target | — | `rustup target add wasm32-unknown-unknown` |
| Stellar CLI | v22+ | [Stellar docs](https://developers.stellar.org/docs/tools/developer-tools/stellar-cli) |

### Build

```bash
# Check that everything compiles
cargo build

# Build the production WASM artifact
cargo build --target wasm32-unknown-unknown --release \
  --manifest-path contracts/upgrade-manager/Cargo.toml
```

### Run Tests

```bash
cargo test
```

---

## Project Structure

```
soroban-upgrade-manager/
├── contracts/
│   └── upgrade-manager/
│       ├── src/
│       │   ├── lib.rs          # Contract entry point, public API, tests
│       │   ├── governance.rs   # Proposal creation, voting, finalization
│       │   ├── timelock.rs     # Timelock enforcement helpers
│       │   └── types.rs        # Shared types, storage keys, errors
│       └── Cargo.toml
└── Cargo.toml
```

**Where to make changes:**

| What you want to do | File |
|---|---|
| Add a new contract function | `lib.rs` |
| Change proposal/vote logic | `governance.rs` |
| Change timelock rules | `timelock.rs` |
| Add a new type or error code | `types.rs` |

---

## Coding Standards

- **Rust edition 2021** throughout.
- `#![no_std]` — do not use the standard library.
- Document every public function with `///` doc comments covering parameters, return value, and all error variants.
- Run `cargo fmt` before committing.
- Run `cargo clippy -- -D warnings` and fix all warnings.
- Keep functions focused. If a function body exceeds ~60 lines, consider splitting it.
- Prefer descriptive variable names over short abbreviations.

---

## Testing

All logic must be tested. The test suite lives in `lib.rs` under `#[cfg(test)]`.

- Use `Env::default()` with `env.mock_all_auths()` for unit tests.
- Test the **happy path** and at least one **error path** per function.
- Advance the ledger with `env.ledger().with_mut(|l| l.sequence_number += N)` to test time-dependent logic.
- New features must include at least one new test. Bug fixes must include a regression test.

Run tests with:

```bash
cargo test
```

To see output from passing tests:

```bash
cargo test -- --nocapture
```

---

## Pull Request Process

1. **Keep PRs focused** — one issue per PR.
2. **Write a clear description** — what changed and why; link the issue with `Closes #N`.
3. **Pass all checks** — `cargo test`, `cargo fmt --check`, `cargo clippy -- -D warnings`.
4. **Update docs** — if your change affects the public API, update `README.md`.
5. **Request review** — tag a maintainer if none is assigned within 24 hours.

### PR Title Format

```
<type>: <short description>

Types: feat | fix | docs | refactor | test | chore
```

Examples:
- `feat: add weighted voting support`
- `fix: prevent double-vote on finalized proposal`
- `docs: clarify timelock_delay units in README`

---

## Issue Labels

| Label | Meaning |
|---|---|
| `good first issue` | Beginner-friendly; well-scoped and documented |
| `Stellar Wave` | Part of the active Wave cycle; eligible for rewards |
| `bug` | Something is broken |
| `enhancement` | New feature or improvement |
| `documentation` | Docs-only change |
| `help wanted` | Maintainer would especially appreciate help here |
| `high complexity` | Significant design or implementation work required |

---

Questions? Open a [GitHub Discussion](https://github.com/ogenyialice120/soroban-upgrade-manager/discussions) or ask in the [Stellar Discord](https://discord.gg/stellardev).
