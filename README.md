<div align="center">

<h1 align="center">Codanna</h1>

[![Claude](https://img.shields.io/badge/Claude-✓%20Copmatible-grey?logo=claude&logoColor=fff&labelColor=D97757)](#)
[![Google Gemini](https://img.shields.io/badge/Gemini-✓%20Compatible-grey?logo=googlegemini&logoColor=fff&labelColor=8E75B2)](#)
[![OpenAI Codex](https://img.shields.io/badge/Codex-✓%20Compatible-grey?logo=openai&logoColor=fff&labelColor=10A37F)](#)
[![Rust](https://img.shields.io/badge/Rust-CE412B?logo=rust&logoColor=white)](#)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/codanna?logo=rust&labelColor=CE412B&color=grey)](#)

<p align="center">
  <a href="https://github.com/bartolli/codanna/tree/main/docs">Documentation</a>
  ·
  <a href="https://github.com/bartolli/codanna/issues">Report Bug</a>
  ·
  <a href="https://github.com/bartolli/codanna/discussions">Discussions</a>
</p>

<h2></h2>

**X-ray vision for your agent.**

Give your code assistant the ability to see through your codebase—understanding functions, tracing relationships, and finding implementations with surgical precision. Context-first coding. No grep-and-hope loops. No endless back-and-forth. Just smarter engineering in fewer keystrokes.
</div>

<h3 align="left"></h3>

## Quick Start

### Install (macOS, Linux, WSL)

```bash
curl -fsSL --proto '=https' --tlsv1.2 https://install.codanna.sh | sh
```

### Or via Homebrew

```bash
brew install bartolli/codanna/codanna
```

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
| **[Semantic Search](docs/user-guide/search-guide.md)** | Natural language queries against code and documentation. Finds functions by what they do, not just their names. |
| **[Relationship Tracking](docs/user-guide/mcp-tools.md)** | Call graphs, implementations, and dependencies. Trace how code connects across files. |
| **[Document Search](docs/user-guide/documents.md)** | Index markdown and text files for RAG workflows. Query project docs alongside code. |
| **[MCP Protocol](docs/integrations/)** | Native integration with Claude, Gemini, Codex, and other AI assistants. |
| **[Profiles](docs/profiles/)** | Package hooks, commands, and agents for different project types. |
| **[Plugins](docs/plugins/)** | Claude Code manifest format for project-scoped workflows. |

**Performance:** Sub-10ms symbol lookups with memory-mapped caches.

**Languages:** Rust, Python, JavaScript, TypeScript, Java, Kotlin, Go, PHP, C, C++, C#, Swift, GDScript.

## Integration

Standard CLI and MCP protocol. Works with Claude, Codex, and any MCP-compatible client.
HTTP/HTTPS servers available for network access.

### Claude Code

```json
{
  "mcpServers": {
    "codanna": {
      "command": "codanna",
      "args": ["serve", "--watch"]
    }
  }
}
```

### HTTP Server

```bash
codanna serve --http --watch
codanna serve --https --watch  # With TLS
```

### Unix Pipes

```bash
codanna mcp find_callers index_file --json | \
jq -r '.data[]?[0] | "\(.name) - \(.file_path)"'
```

See [Integrations](docs/integrations/) for detailed setup guides.

## Documentation

- **[Getting Started](docs/getting-started/)** - Installation and first steps
- **[User Guide](docs/user-guide/)** - CLI commands, tools, configuration
- **[CLI Reference](docs/user-guide/cli-reference.md)** - All commands and options
- **[MCP Tools](docs/user-guide/mcp-tools.md)** - Available tools for AI assistants
- **[Architecture](docs/architecture/)** - How it works under the hood

[View all documentation](docs/)

## Advanced Features

<details>
<summary><strong>Profiles</strong> - Package reusable configurations</summary>

```bash
codanna init --force
codanna profile provider add bartolli/codanna-profiles
codanna profile install claude@codanna-profiles
npm --prefix .claude/hooks/codanna install
```

The `claude` profile includes Research-Agent, `/codanna:x-ray` and `/codanna:symbol` commands, and hooks for skill suggestions.

See [Profile Documentation](docs/profiles/).

</details>

<details>
<summary><strong>Document Collections</strong> - RAG-ready documentation search</summary>

```bash
codanna documents add-collection docs docs/
codanna documents add-collection guides examples/
codanna documents index
codanna documents search "error handling" --collection docs
```

Chunks documents, generates embeddings, and provides semantic search over your markdown files.

See [Document Search](docs/user-guide/documents.md).

</details>

## Requirements

- ~150MB for embedding model (downloaded on first use)
- **Build from source:** Rust 1.85+, Linux needs `pkg-config libssl-dev`

## Status

- Sub-10ms symbol lookups
- 75,000+ symbols/second parsing
- Windows support is experimental

## Contributing

Contributions welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache License 2.0 - See [LICENSE](LICENSE).

Attribution required. See [NOTICE](NOTICE).

---

Built with Rust.
