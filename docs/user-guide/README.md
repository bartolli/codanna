[Documentation](../README.md) / **User Guide**

---

# User Guide

Complete documentation for using Codanna effectively.

## Documentation

- **[CLI Reference](cli-reference.md)** - All commands and flags
- **[MCP Tools](mcp-tools.md)** - Available tools when using the MCP server
- **[Configuration](configuration.md)** - Lives in `.codanna/settings.toml`
- **[Search Guide](search-guide.md)** - Semantic search best practices
- **[Document Search](documents.md)** - Index markdown files for RAG

## Core Commands

| Command | Description | Example |
|---------|-------------|---------|
| `codanna init` | Set up .codanna directory with default configuration | `codanna init --force` |
| `codanna index <PATH>` | Build searchable index from your codebase | `codanna index src --progress` |
| `codanna config` | Display active settings | `codanna config` |
| `codanna serve` | Start MCP server for AI assistants | `codanna serve --watch` |

## MCP Tools Preview

### Simple Tools (Positional Arguments)
| Tool | Description | Example |
|------|-------------|---------|
| `find_symbol` | Find a symbol by exact name | `codanna mcp find_symbol main` |
| `get_calls` | Show functions called by a given function | `codanna mcp get_calls process_file`<br>`codanna mcp get_calls symbol_id:1883` |
| `find_callers` | Show functions that call a given function | `codanna mcp find_callers init`<br>`codanna mcp find_callers symbol_id:1883` |
| `analyze_impact` | Analyze the impact radius of symbol changes | `codanna mcp analyze_impact Parser`<br>`codanna mcp analyze_impact symbol_id:1883` |

### Complex Tools (Key:Value Arguments)
| Tool | Description | Example |
|------|-------------|---------|
| `search_symbols` | Search symbols with full-text fuzzy matching | `codanna mcp search_symbols query:parse kind:function limit:10` |
| `semantic_search_docs` | Search using natural language queries | `codanna mcp semantic_search_docs query:"error handling" limit:5` |

**Tip:** All tools return `[symbol_id:123]` in results. Use `symbol_id:ID` for unambiguous follow-up queries instead of symbol names.

## Next Steps

- Set up [Integrations](../integrations/) with your AI assistant
- Explore [Advanced](../advanced/) Unix-native features
- Learn about the [Architecture](../architecture/) under the hood

[Back to Documentation](../README.md)