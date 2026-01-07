[Documentation](../README.md) / **Reference**

---

# Reference

Quick reference documentation for Codanna.

## In This Section

- **[MCP Protocol](mcp-protocol.md)** - Complete MCP tool specifications
- **[Exit Codes](exit-codes.md)** - CLI exit codes reference

## Quick Reference

### Common Flags

- `--config`, `-c`: Path to custom settings.toml file
- `--force`, `-f`: Force operation (overwrite, re-index, etc.)
- `--dry-run`: Show what would happen without executing
- `--json`: Structured output for piping (exit code 3 when not found)

### MCP Tool Parameters

| Tool | Parameters |
|------|------------|
| `find_symbol` | `name` (required) |
| `search_symbols` | `query`, `limit`, `kind`, `module` |
| `semantic_search_docs` | `query`, `limit`, `threshold`, `lang` |
| `semantic_search_with_context` | `query`, `limit`, `threshold`, `lang` |
| `get_calls` | `function_name` OR `symbol_id` (one required) |
| `find_callers` | `function_name` OR `symbol_id` (one required) |
| `analyze_impact` | `symbol_name` OR `symbol_id` (one required), `max_depth` |
| `get_index_info` | None |

**Using symbol_id:**
- All tools return `[symbol_id:123]` for unambiguous lookup
- Use `symbol_id:ID` instead of name for precise queries
- Example: `codanna mcp get_calls symbol_id:1883`

### Language Filtering

Semantic search tools support language filtering to reduce noise in mixed-language projects:

```bash
# Search only in Rust code
codanna mcp semantic_search_docs query:"authentication" lang:rust limit:5

# Search only in TypeScript code
codanna mcp semantic_search_with_context query:"parse config" lang:typescript limit:3
```

Language filtering eliminates duplicate results when similar documentation exists across multiple languages, reducing result sets by up to 75% while maintaining identical similarity scores.

## Platform Support

**Supported Languages:**
- See [Language Support](../architecture/language-support.md) for complete list
- Tree-sitter based parsing with language-specific resolution

**Embedding Models:**
- Default: `AllMiniLML6V2` (English-optimized, 384 dimensions)
- Multilingual: `MultilingualE5Small` (100+ languages, 384 dimensions)
- Configurable via `model` setting in `.codanna/settings.toml`

**Operating Systems:**
- Linux: Fully supported
- macOS: Fully supported
- Windows: Experimental

## Requirements

- Rust 1.75+ (for development)
- ~150MB for model storage (downloaded on first use)
- A few MB for index storage (varies by codebase size)

## Next Steps

- Check the [User Guide](../user-guide/) for usage
- Explore [Advanced](../advanced/) features
- See [Architecture](../architecture/) for technical details

[Back to Documentation](../README.md)