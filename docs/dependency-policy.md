# Dependency Review Policy

This document defines the security and engineering policy for evaluating, adding,
updating, and auditing dependencies in the `soroban-upgrade-manager` workspace.

---

## 1. Current Dependency Footprint

The workspace intentionally keeps its dependency surface minimal to reduce supply-chain
risk and ensure deterministic Soroban WASM builds (`wasm32v1-none`):

- **Workspace dependencies (`Cargo.toml`):**
  - `soroban-sdk = "22.0.11"` (`default-features = false` / `features = []`) — maintained by the Stellar Development Foundation (SDF).
- **Contract runtime dependencies (`contracts/upgrade-manager/Cargo.toml`):**
  - `soroban-sdk = { workspace = true }` (the **only** production runtime dependency).
- **Contract development dependencies (`contracts/upgrade-manager/Cargo.toml`):**
  - `soroban-sdk = { workspace = true, features = ["testutils"] }`
  - `example-target = { path = "../example-target" }` (workspace member used only in tests).

Because `contracts/upgrade-manager` compiles to `#![no_std]` WebAssembly deployed on
Stellar Soroban, every runtime dependency directly affects the compiled WASM binary
size, host resource metering, and smart contract security boundary.

---

## 2. Criteria for Adding or Updating Dependencies

New runtime (`[dependencies]`) or build/test (`[dev-dependencies]` / `[build-dependencies]`)
crates should only be introduced when the functionality cannot be reasonably and safely
implemented directly inside the contract. Any proposed dependency must satisfy all of
the following criteria:

1. **Actively Maintained:**
   - The crate has active maintainers, a public source repository, recent releases compatible with the workspace MSRV (`rust-version = "1.88.0"`), and responsive issue handling.
2. **Zero Known CVEs or RustSec Advisories:**
   - Neither the crate nor any of its transitive dependencies may have open vulnerabilities or `unmaintained` / `unsound` warnings in the [RustSec Advisory Database](https://rustsec.org/).
3. **Minimal Transitive Dependency Tree:**
   - Runtime dependencies must support `#![no_std]` and compile cleanly to `wasm32v1-none` without pulling in unnecessary transitive crates, procedural macros from untrusted sources, or `build.rs` network/system side effects.
4. **Permissive, Compatible License:**
   - The crate and its transitive tree must be licensed under `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, or `ISC` to remain compatible with this repository's `MIT` license. Copyleft licenses (`GPL`, `LGPL`, `AGPL`) are not permitted.
5. **No Unsafe Code in Contract Runtime (Unless Formally Justified):**
   - Runtime crates compiled into `soroban_upgrade_manager.wasm` should forbid or avoid `unsafe` blocks unless required by low-level Soroban host bindings (such as `soroban-sdk` itself).

---

## 3. Who Reviews Dependency Changes

- **Maintainer Sign-Off Required:** Every pull request that modifies `Cargo.toml`, `Cargo.lock`, or `contracts/*/Cargo.toml` must be explicitly reviewed and approved by a repository maintainer before merge.
- **PR Justification:** The PR author must include a written justification in the PR description explaining:
  - Why the dependency or version bump is needed.
  - Why an in-tree implementation was not preferable.
  - Output from the verification checks below.

---

## 4. How to Verify a Dependency Before Submitting a PR

Contributors and reviewers must run the following checks locally before adding or
upgrading any crate:

### Step 1: Audit for known vulnerabilities with `cargo-audit`

```bash
cargo install cargo-audit --locked
cargo audit
```

Verify that `cargo audit` reports **0 vulnerabilities** and **0 unsound/unmaintained**
warnings across `Cargo.lock`.

### Step 2: Check license and advisory policies with `cargo-deny` (if installed)

```bash
cargo install cargo-deny --locked
cargo deny check advisories bans licenses sources
```

*(See also the repository's CI tracking issue for integrating automated `cargo-audit` / `cargo-deny` checks into `.github/workflows/ci.yml`, e.g., `#196` / security CI hardening issues.)*

### Step 3: Inspect the transitive dependency tree

```bash
cargo tree -p soroban-upgrade-manager --edges normal
```

Confirm that no unexpected transitive crates were added to the production contract's
normal dependency graph.

### Step 4: Inspect the crate on crates.io and docs.rs

- Verify the crate name spelling on [crates.io](https://crates.io/) to prevent typosquatting.
- Check download history, publisher identity, repository link, and recent diff between versions (`cargo metadata` / `docs.rs` source view).
- Verify that the workspace still builds cleanly for `wasm32v1-none` and passes all claims and unit tests:

```bash
cargo build --target wasm32v1-none --release --manifest-path contracts/upgrade-manager/Cargo.toml
cargo test --workspace
bash scripts/check_claims.sh
```
