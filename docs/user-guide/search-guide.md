# Search Guide

How to write effective queries and get the best results from Codanna's search capabilities.

## Search Types

### Exact Match: `find_symbol`
For when you know the exact name:
```bash
codanna mcp find_symbol main
codanna mcp find_symbol SimpleIndexer
```

### Fuzzy Search: `search_symbols`
For partial matches and typos:
```bash
codanna mcp search_symbols query:parse
codanna mcp search_symbols query:indx  # Will find "index" functions
```

### Semantic Search: `semantic_search_docs`
For natural language queries:
```bash
codanna mcp semantic_search_docs query:"where do we handle errors"
codanna mcp semantic_search_docs query:"authentication logic"
```

### Context Search: `semantic_search_with_context`
For understanding relationships:
```bash
codanna mcp semantic_search_with_context query:"file processing pipeline"
```

## Writing Better Documentation Comments

Semantic search requires meaningful documentation. "Parse configuration from TOML file and validate required fields" enables search, while "Load config" does not.

## Query Writing Tips

### Be Specific
- **Bad:** "error"
- **Good:** "error handling in file operations"

### Use Domain Terms
- **Bad:** "make things fast"
- **Good:** "performance optimization for indexing"

### Include Context
- **Bad:** "parse"
- **Good:** "parse TypeScript import statements"

## Language Filtering

In mixed-language codebases, use language filters:

```bash
# Search only Rust code
codanna mcp semantic_search_docs query:"memory management" lang:rust

# Search only TypeScript
codanna mcp semantic_search_docs query:"React components" lang:typescript
```

Supported languages: rust, python, typescript, go, php, c, cpp

## Understanding Scores

Similarity scores range from 0 to 1:
- **0.7+** - Very relevant
- **0.5-0.7** - Relevant
- **0.3-0.5** - Somewhat relevant
- **<0.3** - Probably not what you're looking for

Use threshold to filter:
```bash
codanna mcp semantic_search_docs query:"authentication" threshold:0.5
```

## Search Workflows

### Common Patterns

**Find Implementation:** Semantic search → extract `symbol_id` → trace relationships

**Understand Flow:** Find entry point → `get_calls symbol_id:X` → `analyze_impact`

**Debug Issues:** Search error code → `find_callers symbol_id:X` → trace source

## Advanced Techniques

### Combining Tools
```bash
# Find all parsers and their callers
codanna mcp search_symbols query:parse kind:function --json | \
jq -r '.data[].name' | \
xargs -I {} codanna mcp find_callers {} --json | \
jq -r '.data[][0].name' | sort -u
```

### Building Context
```bash
# Get complete context for a concept
codanna mcp semantic_search_with_context query:"dependency injection" limit:1 --json | \
jq '.data[0]'
```

This returns:
- The symbol itself with `[symbol_id:123]`
- What calls it (each with symbol_id)
- What it calls (each with symbol_id)
- Full impact analysis

Use the returned symbol_ids for precise follow-up queries.

## Common Issues

### No Results

**Problem:** Semantic search returns nothing
**Solution:**
- Check documentation exists
- Try broader terms
- Remove technical jargon

### Too Many Results

**Problem:** Search returns too much
**Solution:**
- Add language filter: `lang:rust`
- Increase threshold: `threshold:0.6`
- Reduce limit: `limit:3`
- Be more specific in query

### Wrong Language Results

**Problem:** Getting Python results when wanting TypeScript
**Solution:** Always use language filter in mixed codebases:
```bash
codanna mcp semantic_search_docs query:"components" lang:typescript
```

## Best Practices

1. **Start with semantic_search_with_context** - It provides the most complete picture
2. **Use symbol_id for follow-ups** - Eliminates ambiguity and saves queries
3. **Use language filters** - Reduces noise by up to 75% in mixed codebases
4. **Write good documentation** - Better docs = better search results
5. **Chain searches** - Use symbol_ids from one search in the next
6. **Use JSON output** - Enables powerful piping and filtering

**Example workflow with symbol_id:**
```bash
# Step 1: Find with semantic search
codanna mcp semantic_search_with_context query:"config parser" limit:1 --json
# Extract: parse_config [symbol_id:567]

# Step 2: Direct follow-up (no ambiguity)
codanna mcp get_calls symbol_id:567
codanna mcp find_callers symbol_id:567
codanna mcp analyze_impact symbol_id:567
```

## Performance Tips

- First search after startup may be slower (cache warming)
- Subsequent searches are typically <10ms
- Use `--json` and `jq` for complex filtering instead of multiple searches

## Document Search

Beyond code symbols, Codanna can index markdown and text files for semantic search:

```bash
# Add and index documentation
codanna documents add-collection docs docs/
codanna documents index

# Search documentation
codanna documents search "authentication flow"
```

Document search supports:
- **KWIC previews** - Results centered on keyword matches
- **Keyword highlighting** - Matching terms wrapped with `**markers**`
- **Collection filtering** - Search within specific document groups

See [Document Search](documents.md) for complete documentation.

## See Also

- [MCP Tools Reference](mcp-tools.md) - Complete tool documentation
- [Document Search](documents.md) - Index markdown files for RAG
- [Unix Piping](../advanced/unix-piping.md) - Advanced search workflows
- [Configuration](configuration.md) - Semantic model configuration