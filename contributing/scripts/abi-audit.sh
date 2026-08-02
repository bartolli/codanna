#!/usr/bin/env bash
# Regenerate the tracked ABI-15 grammar-audit artifacts under
# contributing/parsers/ (AUDIT_REPORT.md, GRAMMAR_ANALYSIS.md,
# node_discovery.txt, TREE_STRUCT.md).
#
# On-demand only: every artifact carries a generation timestamp, so
# the ordinary test suite runs the analysis WITHOUT writing. Run this
# when parser or grammar changes make the artifacts stale, then
# commit the diff.
set -euo pipefail

cd "$(dirname "$0")/../.."
CODANNA_ABI_AUDIT=1 cargo test --test exploration_tests abi15_grammar_audit -- --nocapture "$@"
git status --porcelain contributing/parsers/
