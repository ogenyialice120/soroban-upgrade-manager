# CI Documentation

This document describes every step in the CI and release workflows, what each step checks, what a failure means, and how to fix common failures.

---

## CI workflow (`.github/workflows/ci.yml`)

The CI workflow is named **Build, Test, Lint** and runs on every push or pull request targeting `main` or `master`.

### Trigger

```yaml
on:
  push:
    branches: [ main, master ]
  pull_request:
    branches: [ main, master ]
```

A failure on any step blocks the PR from merging.

---

### Step-by-step

#### 1. Checkout code

```yaml
- uses: actions/checkout@v4
```

**What it does:** Checks out the repository at the commit being tested, including all files and history needed by subsequent steps.

**Why it matters:** Every other step operates on the checked-out source tree. Without this, the runner has no code.

**Failure:** Extremely rare; usually a GitHub infrastructure issue. Re-running the workflow resolves it.

---

#### 2. Install Rust toolchain

```yaml
- uses: dtolnay/rust-toolchain@stable
  with:
    toolchain: 1.88.0
    targets: wasm32v1-none
    components: rustfmt, clippy
```

**What it does:** Installs Rust 1.88.0 (the pinned version) with the `wasm32v1-none` target and the `rustfmt` and `clippy` components.

**Why it matters:** The contract must compile to `wasm32v1-none` for deployment on Soroban. Pinning to 1.88.0 ensures every contributor and every CI run uses the same compiler, eliminating "works on my machine" issues caused by compiler differences.

**Failure:** Network or toolchain server issues. Re-running usually fixes it. If the toolchain version was removed from the Rust release archive, update the pinned version in both workflow files and `rust-toolchain.toml`.

**Common fix:** If you see `rustup: error: toolchain '1.88.0' is not installed`, your local environment may be pinned differently. Run `rustup toolchain install 1.88.0 --target wasm32v1-none --component rustfmt clippy` locally.

---

#### 3. Cache Rust dependencies

```yaml
- uses: actions/cache@v4
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
      target
    key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
    restore-keys: |
      ${{ runner.os }}-cargo-
```

**What it does:** Saves and restores the Cargo registry, git dependencies, and the `target` build directory between runs. The cache key includes the OS and a hash of all `Cargo.lock` files, so the cache is invalidated whenever a dependency changes.

**Why it matters:** Fetching and compiling all dependencies from scratch on every run takes several minutes. Caching cuts that to seconds on cache hits.

**Failure:** A cache miss (first run, or after any `Cargo.lock` change) means a full dependency fetch. This is not a failure — the step will still succeed, just slower. If the cache action itself errors, the build continues without caching.

---

#### 4. Check formatting

```yaml
- run: cargo fmt --all -- --check
```

**What it does:** Runs `rustfmt` across the entire workspace in check mode. It does not modify any files; it exits with a non-zero code if any file would change.

**Why it matters:** Consistent formatting makes diffs easier to read and prevents style churn. `--all` covers every crate in the workspace.

**Failure:** One or more files are not formatted according to `rustfmt` defaults.

**Common fix:** Run `cargo fmt --all` locally, review the diff, and commit the result. Do not add `rustfmt::skip` attributes unless there is a specific reason a block cannot be formatted normally.

---

#### 5. Run clippy

```yaml
- run: cargo clippy --workspace --all-targets -- -D warnings
```

**What it does:** Runs the Clippy linter across all crates and all targets (lib, tests, examples). `-D warnings` promotes every warning to an error, so any lint finding causes the step to fail.

**Why it matters:** Clippy catches a broad range of correctness and performance issues that the compiler itself does not flag: redundant clones, panicking arithmetic in production code, needless borrows, and more. Treating warnings as errors prevents lint debt from accumulating.

**Failure:** One or more Clippy lints fired.

**Common fix:** Read the lint message; Clippy usually suggests the fix. Apply it, verify with `cargo clippy --workspace --all-targets -- -D warnings` locally, and commit. If a lint is a false positive, add a targeted `#[allow(...)]` with a comment explaining why, rather than suppressing all warnings.

---

#### 6. Build (native)

```yaml
- run: cargo build --workspace
```

**What it does:** Compiles all crates in the workspace for the native (host) target in debug mode.

**Why it matters:** Verifies the code compiles cleanly before running tests or the WASM build. A clean native build is a prerequisite for `cargo test`.

**Failure:** Compilation errors in any crate.

**Common fix:** Fix the compiler errors reported in the output. Run `cargo build --workspace` locally to reproduce.

---

#### 7. Run tests

```yaml
- run: cargo test --workspace
```

**What it does:** Compiles and runs all tests in the workspace, including unit tests, integration tests, and doc tests.

**Why it matters:** Tests are the primary verification that the contract behaves correctly. All governance logic, timelock math, error paths, and edge cases are covered here.

**Failure:** One or more tests failed or panicked.

**Common fix:** Run `cargo test --workspace` locally to reproduce. The failing test name and location are shown in the output. Fix the code or the test, whichever is wrong, and verify all tests pass before pushing.

---

#### 8. Build (wasm)

```yaml
- run: cargo build --target wasm32v1-none --release --manifest-path contracts/upgrade-manager/Cargo.toml
```

**What it does:** Compiles only the `upgrade-manager` crate for the `wasm32v1-none` target in release mode. This is the binary that gets deployed to the Soroban network.

**Why it matters:** The native build (step 6) and the WASM build have different constraints. `wasm32v1-none` has no standard library, no OS, and no floating-point. Code that compiles natively may still fail here if it accidentally pulls in `std` dependencies or uses features unavailable in the WASM target.

**Failure:** Compilation errors specific to the `wasm32v1-none` target, often caused by a dependency that is not `no_std`-compatible.

**Common fix:** Ensure any new dependency is compatible with `#![no_std]`. Check its `Cargo.toml` for a `std` feature flag and add `default-features = false` in your dependency declaration if needed.

---

#### 9. Verify claims audit

```yaml
- run: bash scripts/check_claims.sh
```

**What it does:** Runs `scripts/check_claims.sh`, which parses `docs/claims-audit.md` and verifies:
- Every `VERIFIED` row references a test function name in backticks.
- Every referenced test function exists as `fn <name>` in `contracts/upgrade-manager/src/test.rs`.
- Every status value is one of `VERIFIED`, `REWORDED`, or `REMOVED`.

**Why it matters:** The claims audit documents every behavioural claim made about the contract and links each claim to a test. This step ensures the audit stays honest: a claim cannot be marked `VERIFIED` if its test does not exist.

**Failure:** A test name in `claims-audit.md` does not match any function in `test.rs`, a row has an invalid status, or a `VERIFIED` row has no test name.

**Common fix:**
- If you renamed a test, update the corresponding row in `docs/claims-audit.md`.
- If you added a new test to verify a claim, add or update the row in `docs/claims-audit.md`.
- Run `bash scripts/check_claims.sh` locally to confirm it passes before pushing.

---

#### 10. Verify committed fixtures

```yaml
- run: |
    cd contracts/upgrade-manager/tests/fixtures && sha256sum -c SHA256SUMS
```

**What it does:** Changes into the fixtures directory and runs `sha256sum -c SHA256SUMS`, which recomputes the SHA-256 of every file listed in `SHA256SUMS` and compares each result to the recorded hash.

**Why it matters:** Test fixtures are pre-compiled WASM binaries used in integration tests. If a fixture is accidentally modified or corrupted, test results become unreliable. This step ensures the committed fixtures are exactly the ones the tests were written against.

**Failure:** One or more fixture files have a different SHA-256 than the value recorded in `SHA256SUMS`.

**Common fix:**
- If you intentionally rebuilt a fixture, update `SHA256SUMS` by running `sha256sum *.wasm > SHA256SUMS` inside the fixtures directory and committing the new file.
- If the change was accidental, restore the original fixture from git: `git checkout HEAD -- contracts/upgrade-manager/tests/fixtures/`.

---

## Release workflow (`.github/workflows/release.yml`)

The release workflow is named **Build and Release WASM** and runs only when a tag matching `v*` is pushed.

### Trigger

```yaml
on:
  push:
    tags:
      - 'v*'
```

Use semantic versioning for tag names (e.g. `v1.0.0`, `v0.3.1`). The workflow requires `contents: write` permission to create a GitHub Release.

### Steps

The release workflow shares the checkout, toolchain installation (1.88.0, `wasm32v1-none`), and dependency cache steps with CI. After those:

#### Build upgrade-manager WASM (release)

```yaml
- run: cargo build --target wasm32v1-none --release --manifest-path contracts/upgrade-manager/Cargo.toml
```

Produces `target/wasm32v1-none/release/soroban_upgrade_manager.wasm`. This is the same build command as CI step 8, run in release mode with full optimisation.

#### Compute SHA-256

```yaml
- id: sha
  run: |
    WASM_FILE="target/wasm32v1-none/release/soroban_upgrade_manager.wasm"
    SHA256=$(sha256sum "$WASM_FILE" | awk '{print $1}')
    echo "sha256=$SHA256" >> "$GITHUB_OUTPUT"
    echo "$SHA256" > "$WASM_FILE.sha256"
```

Computes the SHA-256 of the compiled WASM, writes it to a step output (`steps.sha.outputs.sha256`), and writes a companion `.sha256` file. Both the `.wasm` and `.sha256` are attached to the release.

**Why it matters:** Deployers can verify the binary they downloaded matches the one produced by CI before deploying to mainnet. Run `sha256sum soroban_upgrade_manager.wasm` after downloading and compare to the value in the release body.

#### Create GitHub Release

```yaml
- uses: softprops/action-gh-release@v2
  with:
    generate_release_notes: true
    files: |
      target/wasm32v1-none/release/soroban_upgrade_manager.wasm
      target/wasm32v1-none/release/soroban_upgrade_manager.wasm.sha256
```

Creates a GitHub Release at the pushed tag, auto-generates release notes from merged PRs since the last tag, attaches the `.wasm` and `.sha256` files, and includes the SHA-256 and a deployment command snippet in the release body.

**Failure:** Usually a permissions issue (`contents: write` not set) or a network error. Re-running resolves transient failures.

---

## Quick reference

| Step | Command | Reproduce locally |
|---|---|---|
| Formatting | `cargo fmt --all -- --check` | `cargo fmt --all` to fix |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | Same command |
| Native build | `cargo build --workspace` | Same command |
| Tests | `cargo test --workspace` | Same command |
| WASM build | `cargo build --target wasm32v1-none --release --manifest-path contracts/upgrade-manager/Cargo.toml` | Same command |
| Claims audit | `bash scripts/check_claims.sh` | Same command |
| Fixture checksums | `cd contracts/upgrade-manager/tests/fixtures && sha256sum -c SHA256SUMS` | Same command |
