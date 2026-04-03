#!/bin/bash
# Local mirror of .github/workflows/full-test.yml
# Run this before pushing to catch ALL GitHub Actions failures
# NOTE: Keep this in sync with full-test.yml - if you update one, update the other!

set -e  # Exit on first error

# Set environment variables like GitHub Actions
export CARGO_TERM_COLOR=always
export RUST_BACKTRACE=1

echo "Running Codanna CI locally (mirrors full-test.yml)"
echo "==================================================="

# Ensure we're using the latest stable Rust (matches GitHub Actions)
echo ""
echo "Ensuring Rust toolchain is up-to-date..."
rustup update stable --no-self-update > /dev/null 2>&1 || true
current_version=$(rustc --version)
echo "   Using: $current_version"

# Job: Test Suite
echo ""
echo "Job: Test Suite"
echo "==============="

# Fast checks first
echo ""
echo "[1/6] Check formatting"
cargo fmt --check
echo "PASS: formatting"

echo ""
echo "[2/6] Clippy with project rules"
cargo clippy --all-targets --all-features -- -D warnings
echo "PASS: clippy"

# Verify no-default-features compiles (check only, no linking)
echo ""
echo "[3/6] Check no-default-features"
cargo check --no-default-features
echo "PASS: no-default-features"

# Run tests (implicitly builds debug binary and all test targets)
echo ""
echo "[4/6] Run tests"
cargo test --verbose
echo "PASS: tests"

# CLI smoke tests using the debug binary built by cargo test
echo ""
echo "[5/6] Verify CLI commands"
./target/debug/codanna --help > /dev/null
echo "  main help: ok"
./target/debug/codanna index --help > /dev/null
echo "  index help: ok"
./target/debug/codanna retrieve --help > /dev/null
echo "  retrieve help: ok"
echo "PASS: CLI commands"

# Documentation
echo ""
echo "[6/6] Check docs build"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
echo "PASS: docs"

# Local-only: MCP server test (not in GitHub Actions)
echo ""
echo "Local-only: MCP server test"
if [ -d ".codanna/index" ]; then
    ./target/debug/codanna mcp-test
    if [ $? -eq 0 ]; then
        echo "PASS: MCP server"
    else
        echo "FAIL: MCP server test"
        exit 1
    fi
else
    echo "SKIP: no index found (run 'codanna init && codanna index src' first)"
fi

echo ""
echo "==================================================="
echo "All checks passed. Safe to push."
echo "==================================================="
