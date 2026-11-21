# Release Process

This document describes how to cut a tagged release of Soroban Upgrade Manager,
what CI does during a release, what artifacts are produced, how to verify them,
and how to deploy from a release.

---

## Overview

Releases are triggered by pushing a Git tag that matches `v*` (e.g. `v1.0.0`,
`v0.2.1`). The release workflow (`.github/workflows/release.yml`) then builds
the upgrade-manager WASM in release mode, computes its SHA-256 checksum, creates
a GitHub Release, and attaches both the WASM and the checksum file as release
assets.

---

## Tag format

The release workflow triggers on any tag whose name starts with `v`:

```
v<MAJOR>.<MINOR>.<PATCH>
```

Examples: `v1.0.0`, `v0.2.0`, `v1.1.3-beta`.

The workflow trigger in `.github/workflows/release.yml` is:

```yaml
on:
  push:
    tags:
      - 'v*'
```

---

## How to create a release

1. **Ensure the main branch is clean and all CI checks pass.**

2. **Tag the commit you want to release:**

   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```

   Pushing the tag is what triggers the release workflow. No manual workflow
   dispatch is needed.

3. **Monitor the workflow** in the Actions tab of the repository. The job is
   named "Build and Release WASM" and runs on `ubuntu-latest`.

---

## What CI does

The release workflow performs these steps in order:

1. **Checkout** — checks out the repository at the tagged commit
   (`actions/checkout@v4`).

2. **Install Rust toolchain** — installs Rust `1.88.0` with the
   `wasm32v1-none` target using `dtolnay/rust-toolchain@stable`.

3. **Cache Rust dependencies** — restores and saves the Cargo registry and
   `target/` directory cache (`actions/cache@v4`) keyed on `Cargo.lock`.

4. **Build upgrade-manager WASM** — compiles the contract in release mode:

   ```bash
   cargo build --target wasm32v1-none --release \
     --manifest-path contracts/upgrade-manager/Cargo.toml
   ```

   The output WASM is written to:
   `target/wasm32v1-none/release/soroban_upgrade_manager.wasm`

5. **Compute SHA-256** — runs `sha256sum` on the WASM and writes the hash to a
   sidecar file:

   ```bash
   sha256sum target/wasm32v1-none/release/soroban_upgrade_manager.wasm
   # hash is also written to soroban_upgrade_manager.wasm.sha256
   ```

6. **Create GitHub Release** — calls `softprops/action-gh-release@v2` to create
   a GitHub Release for the tag with:
   - Auto-generated release notes (`generate_release_notes: true`).
   - A release body containing the WASM filename, the SHA-256 hash, and
     `stellar contract deploy` instructions.
   - Both release assets attached (see below).

---

## Artifacts produced

Each release attaches two files:

| File | Description |
|------|-------------|
| `soroban_upgrade_manager.wasm` | Compiled WASM binary of the upgrade-manager contract |
| `soroban_upgrade_manager.wasm.sha256` | Plain-text file containing the SHA-256 hash of the WASM |

The SHA-256 hash is also printed in the release body for quick reference.

---

## How to verify a release

After downloading `soroban_upgrade_manager.wasm` from the release assets, run:

```bash
sha256sum soroban_upgrade_manager.wasm
```

The output should match the hash printed in the release body and stored in
`soroban_upgrade_manager.wasm.sha256`.

Example output:

```
d4696206a5ba03d4326fea8da1f3e2ca5b693fe632a6dd8d459683dc994b25cd  soroban_upgrade_manager.wasm
```

Compare that hash against the `SHA-256` line in the GitHub Release description.
If the hashes do not match, do not deploy the WASM — the file may be corrupt or
tampered with.

---

## How to deploy from a release

Download `soroban_upgrade_manager.wasm` from the release assets page, verify its
SHA-256 (see above), then deploy using the Stellar CLI:

```bash
stellar contract deploy \
  --wasm soroban_upgrade_manager.wasm \
  --source <ADMIN_KEY> \
  --network <NETWORK>
```

Replace `<ADMIN_KEY>` with the name or address of the account that will act as
contract admin, and `<NETWORK>` with `testnet` or `mainnet`.

After deployment, initialize the contract:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network <NETWORK> \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --voting_period 17280 \
  --timelock_delay 17280 \
  --quorum 3 \
  --approval_threshold_pct 51
```

See [docs/walkthrough.md](walkthrough.md) for a full step-by-step deployment
guide including target contract setup, proposal creation, voting, finalization,
and execution.

---

## Version numbering

The current contract version is `0.1.0` (see `Cargo.toml`). Tags follow
[Semantic Versioning](https://semver.org/):

- **PATCH** (`v0.1.1`) — bug fixes with no API changes.
- **MINOR** (`v0.2.0`) — new backwards-compatible functionality.
- **MAJOR** (`v1.0.0`) — breaking API changes.

The contract is currently unaudited. Treat all releases before `v1.0.0` as
pre-production.
