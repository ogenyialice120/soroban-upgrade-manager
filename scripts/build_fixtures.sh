#!/usr/bin/env bash
# scripts/build_fixtures.sh
#
# Build the v1 and v2 WASM binaries for example-target and write them
# to contracts/upgrade-manager/tests/fixtures/.
#
# Usage (from repo root):
#   bash scripts/build_fixtures.sh
#
# To regenerate committed fixtures after changing example-target source:
#   bash scripts/build_fixtures.sh
#   git add contracts/upgrade-manager/tests/fixtures/
#   git commit -m "chore: regenerate example-target fixture WASMs"
#
# TOOLCHAIN:
# The WASMs are built with the toolchain pinned in rust-toolchain.toml
# (1.88.0), matching the toolchain used for the rest of the workspace.
# This version is used to keep builds consistent with the pinned toolchain;
# no compatibility claim is made beyond that.
#
# NOTE: The committed fixtures were not verified to upload in the test host.
#
# NOTE: CI does not rebuild fixtures. It only verifies that the committed
# fixtures match SHA256SUMS. Rebuilding on another machine may produce
# different bytes (build paths are embedded), so a checksum match is the
# only check performed in CI.
#
#   rustup install 1.88.0
#   rustup target add wasm32v1-none --toolchain 1.88.0

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/contracts/upgrade-manager/tests/fixtures"
MANIFEST="$REPO_ROOT/contracts/example-target/Cargo.toml"
TOOLCHAIN="+1.88.0"

echo "[build_fixtures] Repo root:    $REPO_ROOT"
echo "[build_fixtures] Fixture dir:  $FIXTURE_DIR"
echo "[build_fixtures] Toolchain:    $TOOLCHAIN"

# Ensure the toolchain and wasm32 target are available.
rustup install 1.88.0 2>/dev/null || true
rustup target add wasm32v1-none --toolchain 1.88.0 2>/dev/null || true

mkdir -p "$FIXTURE_DIR"

# ---- v1 ----
echo "[build_fixtures] Building v1 ..."
cargo "$TOOLCHAIN" build \
  --manifest-path "$MANIFEST" \
  --target wasm32v1-none \
  --release \
  2>&1

cp "$REPO_ROOT/target/wasm32v1-none/release/example_target.wasm" \
   "$FIXTURE_DIR/target_v1.wasm"
echo "[build_fixtures] Wrote target_v1.wasm"

# ---- v2 (--features v2) ----
echo "[build_fixtures] Building v2 ..."
cargo "$TOOLCHAIN" build \
  --manifest-path "$MANIFEST" \
  --target wasm32v1-none \
  --release \
  --features v2 \
  2>&1

cp "$REPO_ROOT/target/wasm32v1-none/release/example_target.wasm" \
   "$FIXTURE_DIR/target_v2.wasm"
echo "[build_fixtures] Wrote target_v2.wasm"

# ---- SHA256SUMS ----
cd "$FIXTURE_DIR"
sha256sum target_v1.wasm target_v2.wasm > SHA256SUMS
echo "[build_fixtures] Wrote SHA256SUMS:"
cat SHA256SUMS

echo "[build_fixtures] Done."
