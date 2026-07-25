#!/bin/bash
# Auto-fix common issues before committing
# This modifies files to fix formatting and linting issues

set -e

# Match GitHub Actions environment
export CARGO_TERM_COLOR=always
export RUST_BACKTRACE=1

echo "🔧 Auto-fixing common issues..."
echo "================================"
echo ""

# Ensure we're using the latest stable Rust (matches GitHub Actions)
echo "0️⃣ Ensuring Rust toolchain is up-to-date..."
rustup update stable --no-self-update > /dev/null 2>&1 || true
current_version=$(rustc --version)
echo "   Using: $current_version"
echo ""

# Auto-format code
echo "1️⃣ Auto-formatting code..."
cargo fmt
echo "✓ Code formatted"

echo ""
echo "2️⃣ Auto-fixing clippy issues (all targets and features)..."
cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
echo "✓ Clippy fixes applied (where possible)"

echo ""
echo "3️⃣ Checking if all issues are fixed..."
echo ""

# Run quick check to verify
echo "Running quick-check to verify fixes..."
echo "--------------------------------------"
./contributing/scripts/quick-check.sh

echo ""
echo "🎉 Auto-fix complete!"
echo ""
echo "Next steps:"
echo "   - Review the changes with 'git diff'"
echo "   - Stage changes with 'git add -p' (interactive) or 'git add .'"
echo "   - Commit with a descriptive message"