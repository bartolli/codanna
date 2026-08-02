#!/usr/bin/env bash
# Run the test suite in a Linux container against the committed tree.
#
# The tag-triggered CI suite is otherwise the tests' only Linux
# execution; this makes that execution locally repeatable BEFORE a tag
# exists. Gate rule: green before any `/log --tag`.
#
# Committed tree only (clones from the repo): commit before gating.
# Cargo caches persist in named volumes; the first run compiles cold,
# later runs are incremental.
#
# Runs as a NON-ROOT user inside the container: permission-based
# fault-injection tests are inert for root (root writes through a
# 0o444 file, so forced-save-failure tests fail), and CI runners are
# non-root.
#
# Usage:
#   ./contributing/scripts/linux-check.sh                # full suite
#   ./contributing/scripts/linux-check.sh test --all-features --test cli_tests serve_stdio_listen
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
ARGS="${*:-test --all-features}"

exec docker run --rm \
  -v "$REPO":/src:ro \
  -v codanna-linux-cargo:/cargo/registry \
  -v codanna-linux-target:/tmp/target \
  rust:latest \
  bash -c "useradd -m runner \
    && mkdir -p /cargo/registry /tmp/target \
    && chown -R runner /cargo /tmp/target \
    && git clone -q /src /tmp/repo \
    && chown -R runner /tmp/repo \
    && echo \"linux gate at: \$(git -C /tmp/repo log --oneline -1)\" \
    && su runner -c 'export CARGO_HOME=/cargo CARGO_TARGET_DIR=/tmp/target; cd /tmp/repo && cargo $ARGS'"
