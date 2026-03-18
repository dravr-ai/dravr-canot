#!/usr/bin/env bash
# ABOUTME: Pre-push validation script for dravr-canot
# ABOUTME: Runs format, clippy, and tests, then creates a validation marker
set -euo pipefail

echo "=== dravr-canot pre-push validation ==="

echo "[1/3] Checking formatting..."
cargo fmt --all -- --check
echo "  Format OK"

echo "[2/3] Running clippy..."
cargo clippy --workspace --all-targets --features all-channels --quiet -- -D warnings
echo "  Clippy OK"

echo "[3/3] Running tests..."
cargo test --workspace --features all-channels --quiet
echo "  Tests OK"

# Create validation marker
MARKER=".git/validation-passed"
echo "$(date +%s) $(git rev-parse HEAD)" > "$MARKER"
echo ""
echo "Validation passed. Marker written to $MARKER (valid for 15 minutes)."
