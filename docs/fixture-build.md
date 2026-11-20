# Fixture build and verification process

This document explains how the example-target fixture WASMs are built, how to
verify them locally, when to rebuild them, and the toolchain requirement. It is
intended for contributors who need to rebuild or verify the fixtures.

## What the fixtures are

The fixture WASMs are compiled binaries of the example-target contract used by
the upgrade-manager test suite:

| File | Description |
|------|-------------|
| `contracts/upgrade-manager/tests/fixtures/target_v1.wasm` | example-target built without the `v2` feature flag (`version()` returns `1`) |
| `contracts/upgrade-manager/tests/fixtures/target_v2.wasm` | example-target built with `--features v2` (`version()` returns `2`) |
| `contracts/upgrade-manager/tests/fixtures/SHA256SUMS` | SHA-256 checksums of the two fixture WASMs |

The fixtures live in
`contracts/upgrade-manager/tests/fixtures/`. They are committed to the repo so
the test suite can reference them without rebuilding, and CI verifies that the
committed binaries match their recorded checksums.

## How to build the fixtures

Run the fixture build script from the repo root:

```bash
bash scripts/build_fixtures.sh
```

What `scripts/build_fixtures.sh` does:

1. Ensures the Rust toolchain `1.88.0` and the `wasm32v1-none` target are
   installed (`rustup install 1.88.0`, `rustup target add wasm32v1-none --toolchain 1.88.0`).
2. Builds `target_v1.wasm` from
   `contracts/example-target/Cargo.toml` with
   `cargo +1.88.0 build --target wasm32v1-none --release` (no `v2` feature).
3. Copies the artifact to
   `contracts/upgrade-manager/tests/fixtures/target_v1.wasm`.
4. Builds `target_v2.wasm` with the same command plus `--features v2`.
5. Copies the artifact to
   `contracts/upgrade-manager/tests/fixtures/target_v2.wasm`.
6. Writes
   `contracts/upgrade-manager/tests/fixtures/SHA256SUMS` via
   `sha256sum target_v1.wasm target_v2.wasm > SHA256SUMS`.

To regenerate committed fixtures after changing the example-target source:

```bash
bash scripts/build_fixtures.sh
git add contracts/upgrade-manager/tests/fixtures/
git commit -m "chore: regenerate example-target fixture WASMs"
```

## Toolchain requirement

The fixtures are built with the toolchain pinned in `rust-toolchain.toml`:
**Rust 1.88.0**. This matches the toolchain used for the rest of the workspace.
This version is used to keep builds consistent with the pinned toolchain; no
compatibility claim is made beyond that.

Install and target setup:

```bash
rustup install 1.88.0
rustup target add wasm32v1-none --toolchain 1.88.0
```

The build script itself attempts these installs (best-effort, `|| true`) but it
is your responsibility to have the `1.88.0` toolchain available.

## How to verify the checksums

From the fixtures directory:

```bash
cd contracts/upgrade-manager/tests/fixtures
sha256sum -c SHA256SUMS
```

If the committed WASMs match their recorded checksums, the command prints `OK`
for each file and exits `0`. Otherwise it reports a mismatch and exits non-zero.

This is the same check CI runs (see `.github/workflows/ci.yml`, "Verify
committed fixtures match SHA256SUMS" step).

## When to rebuild

Rebuild the fixtures **after changing the `example-target` source** (e.g.
`contracts/example-target/src/lib.rs` or its `Cargo.toml` features), because the
committed WASMs must reflect the current target contract behavior.

Do not rebuild for unrelated changes — fixture WASMs are not byte-identical
across machines (build paths are embedded in the binary), so rebuilding on a
different machine produces different bytes even from identical source.

## CI verifies checksums only — it does not rebuild

**Important:** CI does **not** rebuild the fixtures. It only verifies that the
committed fixtures match `SHA256SUMS`:

```yaml
- name: Verify committed fixtures match SHA256SUMS
  run: |
    cd contracts/upgrade-manager/tests/fixtures && sha256sum -c SHA256SUMS
```

Because WASM builds are not byte-identical across machines (build paths are
embedded), the old "rebuild and git diff" drift check failed on GitHub runners.
CI now performs checksum verification only. Rebuilding on another machine may
produce different bytes, so a checksum match is the only check performed in CI.

## Summary of commands

| Task | Command |
|------|---------|
| Build fixtures | `bash scripts/build_fixtures.sh` |
| Verify checksums | `cd contracts/upgrade-manager/tests/fixtures && sha256sum -c SHA256SUMS` |
| Install toolchain | `rustup install 1.88.0` |
| Add WASM target | `rustup target add wasm32v1-none --toolchain 1.88.0` |
