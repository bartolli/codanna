
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

> [!NOTE]
> **New Feature: Profiles!**
> Package hooks, commands, skills, and agents for different project types and custom workflows. More control than plugins, not tied to Claude Code manifest.
>
> The `claude` profile includes Research-Agent (renamed from codanna-navigator), `/codanna:x-ray` and `/codanna:symbol` commands, and hooks for read size limits and skill suggestions.

**Quick Profile Install (0.6.8+):**
```bash
# Initialize with latest settings
codanna init --force

# Add provider and install profile
codanna profile provider add bartolli/codanna-profiles
codanna profile install claude@codanna-profiles

# Install hook system
npm --prefix .claude/hooks/codanna install
```

**Local customization**: Clone [codanna-profiles](https://github.com/bartolli/codanna-profiles) and create profiles for your project types.

See [Profile Documentation](docs/profiles/) for details.

**Also available: Plugins** - Claude Code manifest format for CC-specific workflows. See [Plugin Documentation](docs/plugins/).

## What It Solves

Your AI assistant knows your code:

- "Where's this function called?" → instant call graph
- "Show me all authentication functions" → finds functions with auth-related doc comments
- "Find config file parsers" → matches functions that parse configuration
- "What breaks if I change this interface?" → full-project impact analysis

## Why Bother

**Context is everything.**

Codanna cuts the noise:

- Less grep-and-hope loops.
- Less explaining the same thing twice.
- Less blind code generation.

**Instead**: tight context, smarter engineering, flow that doesn't stall.

## Quick Start

```bash
# Install
cargo install codanna --all-features

# Setup
codanna init

# Index your code
codanna index . --progress                  # Index current directory
codanna index src tests --progress          # Index specific directories
codanna add-dir src --progress              # Add directory to settings for indexing

# Ask real questions
codanna mcp semantic_search_docs query:"where do we resolve symbol references" limit:3
```

**Result**: 3 relevant functions in 0.16s with exact file locations and signatures.

## Features

- **Fast parsing** - Tree-sitter AST (same as GitHub code navigator)
- **Semantic search** - Natural language queries that understand your code
- **Relationship tracking** - Call graphs, implementations, dependencies
- **Multi-language** - Rust, Python, JavaScript, TypeScript, Java, Kotlin, Go, PHP, C, C++, C#, Swift, GDScript
- **MCP protocol** - Native integration with Claude and other AI assistants
- **Profiles** - Package configs for different project types and custom workflows
- **Plugins** - Claude Code manifest format for project-scoped commands and agents
- **<10ms lookups** - Memory-mapped caches for instant responses


## Documentation

### Learn
- **[Getting Started](docs/getting-started/)** - Installation and first steps
- **[User Guide](docs/user-guide/)** - CLI commands, tools, configuration
- **[Integrations](docs/integrations/)** - Claude, Codex, HTTP/HTTPS servers
- **[Profiles](docs/profiles/)** - Package hooks, commands, skills, and agents

### Master
- **[Advanced](docs/advanced/)** - Unix piping, slash commands, performance
- **[Architecture](docs/architecture/)** - How it works under the hood
- **[Contributing](docs/contributing/)** - Development setup and guidelines

### Reference
- **[CLI Reference](docs/user-guide/cli-reference.md)** - All commands and options
- **[MCP Tools](docs/user-guide/mcp-tools.md)** - Available MCP tools
- **[Configuration](docs/user-guide/configuration.md)** - Settings and customization

[View all documentation →](docs/)

## Integration Examples

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

### Agent Workflow
```markdown
1. semantic_search_with_context - Find relevant code
2. analyze_impact - Map dependencies
3. find_symbol, get_calls - Get specifics
```

### Unix Native
```bash
# Build call graphs with pipes
codanna mcp find_callers index_file --json | \
jq -r '.data[]?[0] | "\(.name) - \(.file_path)"'
```

## Requirements

- Rust 1.75+ (for development)
- ~150MB for model storage (downloaded on first use)
- A few MB for index storage

### System Dependencies

**Linux**: `sudo apt install pkg-config libssl-dev`
**macOS**: No additional dependencies

## Current Status

- 12 supported languages with tree-sitter parsing
- 75,000+ symbols/second parsing speed
- <10ms symbol lookups
- Windows support is experimental

## Releases

See [CHANGELOG.md](CHANGELOG.md) for detailed release notes.

## Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under the Apache License, Version 2.0 - See [LICENSE](LICENSE) file.

Attribution required when using Codanna in your project. See [NOTICE](NOTICE) file.

---

Built with 🦀 by devs throttled by tools that "understand" code only in theory.