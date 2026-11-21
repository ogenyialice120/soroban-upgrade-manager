# Contributor Onboarding

This guide walks a new contributor through cloning the repository, setting up
the toolchain, building, running the test suite, understanding the project
structure, and finding a first issue.

---

## Prerequisites

- **Rust 1.88.0** (pinned in `rust-toolchain.toml`) — install via
  [rustup](https://rustup.rs/).
- **Stellar CLI v22+** — required to deploy and invoke contracts on a network.
  Install with the official script:
  ```bash
  curl -fsSL https://github.com/stellar/stellar-cli/raw/main/install.sh | sh
  ```
- **Git**

---

## Step 1 — Clone the repository

```bash
git clone https://github.com/ogenyialice120/soroban-upgrade-manager.git
cd soroban-upgrade-manager
```

---

## Step 2 — Install the Rust toolchain and WASM target

The repository pins Rust to `1.88.0` via `rust-toolchain.toml`. Running any
`cargo` command inside the repository directory will trigger `rustup` to install
the pinned toolchain automatically if it is not already present.

Add the `wasm32v1-none` compilation target:

```bash
rustup target add wasm32v1-none
```

---

## Step 3 — Build

Verify the native build compiles without errors:

```bash
cargo build --workspace
```

Verify the WASM build compiles without errors:

```bash
cargo build --target wasm32v1-none --release \
  --manifest-path contracts/upgrade-manager/Cargo.toml
```

The compiled WASM is written to
`target/wasm32v1-none/release/soroban_upgrade_manager.wasm`.

---

## Step 4 — Run the test suite

```bash
cargo test --workspace
```

All tests should pass. The test suite currently contains more than 80 unit and
integration tests in `contracts/upgrade-manager/src/test.rs`.

---

## Step 5 — Run the linters

CI enforces these two checks on every pull request. Run them locally before
pushing:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo fmt --all -- --check` fails if any file is not correctly formatted.
Run `cargo fmt --all` (without `--check`) to auto-format.

`cargo clippy --workspace --all-targets -- -D warnings` fails on any compiler
warning.

---

## Step 6 — Verify the claims audit

The script `scripts/check_claims.sh` validates that every VERIFIED claim in
`docs/claims-audit.md` names a test function that exists in `test.rs`. Run it
after any change that touches `claims-audit.md` or `test.rs`:

```bash
bash scripts/check_claims.sh
```

CI runs this script on every push. It should exit with:

```
OK: All claims audit checks passed.
```

---

## Step 7 — Read the claims audit

`docs/claims-audit.md` maps every behavioral claim in the README, walkthrough,
CONTRIBUTING, and source doc comments to the function that implements it and the
test that verifies it. Reading it gives you a quick map of what the contract does
and where each behavior is tested.

---

## Step 8 — Find a good first issue

Browse the repository's [GitHub Issues](https://github.com/ogenyialice120/soroban-upgrade-manager/issues)
and filter by the `good first issue` label. Comment on the issue to claim it
before starting work, then follow the contribution workflow in
[CONTRIBUTING.md](../CONTRIBUTING.md).

---

## Project structure

```
soroban-upgrade-manager/
├── contracts/
│   ├── upgrade-manager/        # The main governance contract
│   │   ├── src/
│   │   │   ├── lib.rs          # Public API entry points
│   │   │   ├── governance.rs   # Proposal creation, voting, finalization
│   │   │   ├── timelock.rs     # Timelock enforcement helpers
│   │   │   ├── types.rs        # DataKey enum, Config, Proposal, Error
│   │   │   └── test.rs         # Full test suite
│   │   └── tests/fixtures/     # Committed example-target WASM files + SHA256SUMS
│   └── example-target/         # Minimal upgradable contract for tests/walkthrough
│       └── src/lib.rs
├── scripts/
│   ├── build_fixtures.sh       # Rebuild example-target WASM fixtures
│   └── check_claims.sh         # Validate claims-audit.md against test.rs
├── docs/
│   ├── claims-audit.md         # Claim → function → test mapping
│   ├── walkthrough.md          # Step-by-step deployment guide
│   └── ...                     # Other reference documents
├── .github/workflows/
│   ├── ci.yml                  # CI: build, test, lint, claims audit
│   └── release.yml             # Release: build WASM, publish GitHub Release
├── Cargo.toml                  # Workspace manifest
├── rust-toolchain.toml         # Pinned toolchain (1.88.0)
├── CONTRIBUTING.md             # PR guidelines and changelog requirements
└── _CHANGELOG.md               # Append-only change log
```

### Key contracts

**`contracts/upgrade-manager`** is the governance contract. It is the only
contract deployed to the network in production. Its public API (`lib.rs`) exposes
`initialize`, `propose_upgrade`, `vote`, `finalize`, `execute`, `cancel`,
`update_config`, `transfer_admin`, and several read-only getters.

**`contracts/example-target`** is a minimal contract used in tests and the
walkthrough to demonstrate the upgrade pattern. It stores a manager address at
deploy time and authenticates the manager inside its `upgrade()` function.

---

## Common commands reference

| Task | Command |
|------|---------|
| Build (native) | `cargo build --workspace` |
| Build (WASM) | `cargo build --target wasm32v1-none --release --manifest-path contracts/upgrade-manager/Cargo.toml` |
| Run all tests | `cargo test --workspace` |
| Check formatting | `cargo fmt --all -- --check` |
| Auto-format | `cargo fmt --all` |
| Run lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| Validate claims | `bash scripts/check_claims.sh` |
| Rebuild fixtures | `bash scripts/build_fixtures.sh` |
