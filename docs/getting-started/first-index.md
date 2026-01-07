# Your First Index

Learn how to create and use your first Codanna index.

## Initialize Codanna

```bash
codanna init
```

This creates `.codanna/` directory with:
- `settings.toml` - Configuration file
- `index/` - Where your code index will be stored

## Understanding .codannaignore

Codanna respects `.gitignore` and adds its own `.codannaignore`:

```bash
# Created automatically by codanna init
.codanna/       # Don't index own data
target/         # Skip build artifacts
node_modules/   # Skip dependencies
*_test.rs       # Optionally skip tests
```

## Index Your Code

### Dry Run (Preview)

See what will be indexed without actually indexing:

```bash
codanna index src --dry-run
```

### Build the Index

```bash
# Index entire project (respects .gitignore and .codannaignore)
codanna index . 

# Index specific directory
codanna index src 

# Index a single file
codanna index src/main.rs

# Force re-index
codanna index src --force
```

## Verify Your Index

Check that indexing worked:

```bash
# Get index statistics
codanna mcp get_index_info

# Search for a known function
codanna mcp find_symbol main

# Try semantic search
codanna mcp semantic_search_docs query:"error handling" limit:5
```

## How Indexing Works

1. **Parse fast** - Tree-sitter AST parsing (same as GitHub code navigator) for [11 languages](../architecture/language-support.md)
2. **Extract real stuff** - functions, traits, type relationships, call graphs
3. **Embed** - semantic vectors built from your doc comments
4. **Index** - Tantivy + memory-mapped symbol cache for <10ms lookups

## Tips for Better Indexing

### Documentation Comments

Semantic search works by understanding your documentation comments:

```rust
/// Parse configuration from a TOML file and validate required fields
/// This handles missing files gracefully and provides helpful error messages
fn load_config(path: &Path) -> Result<Config, Error> {
    // implementation...
}
```

With good comments, semantic search can find this function when prompted for:
- "configuration validation"
- "handle missing config files"
- "TOML parsing with error handling"

### Mixed-Language Codebases

When identical documentation exists across multiple languages (e.g., Python backend and TypeScript frontend with similar auth functions), use language filtering to get language-specific results: `lang:python` or `lang:typescript`.

## Troubleshooting

### Index Takes Too Long

- Use `--threads` to control parallelism
- Consider using `.codannaignore` to skip large directories
- Skip test files if not needed

### No Results in Search

- Ensure files have documentation comments
- Check that the language is [supported](../architecture/language-support.md)
- Verify files aren't excluded by `.gitignore` or `.codannaignore`

## Next Steps

- Learn [MCP Tools](../user-guide/mcp-tools.md) for searching your index
- Set up [Integrations](../integrations/) with AI assistants
- Configure [settings.toml](../user-guide/configuration.md) for your project