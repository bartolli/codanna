<div align="center">

<h1 align="center">Codanna</h1>

[![Claude](https://img.shields.io/badge/Claude-✓%20Compatible-grey?logo=claude&logoColor=fff&labelColor=D97757)](#)
[![Google Gemini](https://img.shields.io/badge/Gemini-✓%20Compatible-grey?logo=googlegemini&logoColor=fff&labelColor=8E75B2)](#)
[![OpenAI Codex](https://img.shields.io/badge/Codex-✓%20Compatible-grey?logo=openai&logoColor=fff&labelColor=10A37F)](#)
[![Rust](https://img.shields.io/badge/Rust-CE412B?logo=rust&logoColor=white)](#)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/codanna?logo=rust&labelColor=CE412B&color=grey)](#)

<p align="center">
  <a href="https://docs.codanna.sh/" target="_blank">Documentation</a>
  ·
  <a href="https://github.com/bartolli/codanna/issues">Report Bug</a>
  ·
  <a href="https://github.com/bartolli/codanna/discussions">Discussions</a>
</p>

<h2></h2>

**X-ray vision for your agent.**

Give your code assistant the ability to see through your codebase—understanding functions, tracing relationships, and finding implementations with surgical precision. Context-first coding. No grep-and-hope loops. No endless back-and-forth. Just smarter engineering in fewer keystrokes.

Built for rapid R&D and pair programming—instant answers when LSP is too slow. [Learn more](https://docs.codanna.sh/)
</div>

<h3 align="left"></h3>

## Quick Start

### Install (macOS, Linux, WSL)

```bash
curl -fsSL --proto '=https' --tlsv1.2 https://install.codanna.sh | sh
```

### Or via Homebrew

```bash
brew install codanna
```

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/bartolli/codanna/main/scripts/install.ps1 | iex
```

See [Installation Guide](https://docs.codanna.sh/installation) for Cargo and other options.

### Initialize and index

```bash
codanna init
codanna index src
```

### Search code

```bash
codanna mcp semantic_search_with_context query:"where do we handle errors" limit:3
```

### Search documentation (RAG)

```bash
codanna documents add-collection docs ./docs
codanna documents index
codanna mcp search_documents query:"authentication flow"
```

## What It Does

Your AI assistant gains structured knowledge of your code:

- **"Where's this function called?"** - Instant call graph, not grep results
- **"Find authentication logic"** - Semantic search matches intent, not just keywords
- **"What breaks if I change this?"** - Full dependency analysis across files

The difference: Codanna understands code structure. It knows `parseConfig` is a function that calls `validateSchema`, not just a string match.

## Features

| Feature | Description |
|---------|-------------|
| **[Semantic Search](https://docs.codanna.sh/features/semantic-search)** | Natural language queries against code and documentation. Finds functions by what they do, not just their names. |
| **[Relationship Tracking](https://docs.codanna.sh/features/relationships)** | Call graphs, implementations, and dependencies. Trace how code connects across files. |
| **[Document Search](https://docs.codanna.sh/features/document-search)** | Index markdown and text files for RAG workflows. Query project docs alongside code. |
| **[MCP Protocol](https://docs.codanna.sh/reference/mcp-quick)** | Native integration with Claude, Gemini, Codex, and other AI assistants. |
| **[Profiles](https://docs.codanna.sh/features/collaboration)** | Package hooks, commands, and agents for different project types. |

**Performance:** Sub-10ms lookups, 75,000+ symbols/second parsing.

**Languages:** Rust, Python, JavaScript, TypeScript, Java, Kotlin, Go, PHP, C, C++, C#, Lua, Swift, GDScript.

## Integration

MCP protocol for AI assistants. Works with Claude Code, Cursor, Windsurf, and any MCP-compatible client. Supports stdio, HTTP, and HTTPS transports.

See [Integration Guides](https://docs.codanna.sh/reference/mcp-quick) for setup instructions.

## Requirements

- ~150MB for embedding model (downloaded on first use)
- **Build from source:** Rust 1.85+, Linux needs `pkg-config libssl-dev`
- Windows support is experimental

## Contributing

Contributions welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache License 2.0 - See [LICENSE](LICENSE).

Attribution required. See [NOTICE](NOTICE).

---

Built with Rust.
