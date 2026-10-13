# Agent Contributor Guide

This repository is Codanna: a local code-intelligence MCP server and CLI for AI coding agents. Keep changes small, local-first, and easy to verify.

## Start Here

Before changing code, read:

- `README.md` for product scope, supported modes, and user-facing behavior.
- `CONTRIBUTING.md` for the short contribution workflow.
- `contributing/README.md` and `contributing/development/guidelines.md` for Rust development rules.
- `contributing/development/language-support.md` before parser or language-support work.
- `.mcp_stdio.json`, `CLAUDE.md.example`, and `.codannaignore` when touching MCP integration, agent handoff, or indexing behavior.

## Important Areas

Be extra careful with changes to:

- `src/` parser, indexing, relationship, semantic-search, and MCP code.
- MCP stdio / HTTP / HTTPS transport behavior.
- Document/RAG indexing and `.codannaignore` handling.
- Embedding model download/configuration and any remote embedding opt-in paths.
- Install scripts, release packaging, and profile/integration templates.
- Language parser grammars or generated parser analysis files under `contributing/parsers/`.

## Verification

Use the existing scripts instead of inventing new checks:

```bash
cargo build --release --all-features
./contributing/scripts/quick-check.sh
./contributing/scripts/auto-fix.sh
./contributing/scripts/full-test.sh
```

For small docs-only changes, at minimum run a diff/format sanity check and state that no code tests were needed. For code changes, prefer a targeted test first, then the relevant contributor script.

## Pull Request Notes

- Keep PRs focused on one issue or behavior.
- Add or update tests for behavior changes.
- Update docs when user-facing commands, MCP contracts, configuration, or language support changes.
- Do not commit generated or cache directories such as `.codanna/` unless maintainers explicitly ask.
- If a change affects agent integration, include the MCP/CLI command you used to verify it.
