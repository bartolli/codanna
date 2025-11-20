# Core Concepts

Canonical definitions for concepts used throughout Codanna.

## symbol_id

Every symbol in the index has a unique identifier. Use `symbol_id:123` to reference symbols unambiguously.

### Why Use symbol_id?

- **Eliminates ambiguity** - Multiple functions can have the same name
- **Faster queries** - Direct ID lookup vs name resolution
- **Chain operations** - Use IDs from one query in the next

### Where to Find symbol_id

All MCP tools return symbol IDs in results:
```bash
codanna mcp semantic_search_with_context query:"config parser" limit:1
# Returns: parse_config [symbol_id:567]
```

### How to Use symbol_id

Pass to any tool that accepts `function_name`:
```bash
# Find calls using ID instead of name
codanna mcp get_calls symbol_id:567

# Find callers
codanna mcp find_callers symbol_id:567

# Analyze impact
codanna mcp analyze_impact symbol_id:567
```

### Workflow Pattern

```bash
# 1. Search returns symbol_id
codanna mcp semantic_search_with_context query:"authentication" --json
# Extract: authenticate_user [symbol_id:1234]

# 2. Use symbol_id for follow-up queries
codanna mcp find_callers symbol_id:1234
codanna mcp get_calls symbol_id:1234
```

## Language Filtering

Filter search and query results by programming language.

### Syntax

Use `lang:` parameter with language name:
```bash
codanna mcp semantic_search_with_context query:"parser" lang:rust
codanna mcp search_symbols query:"Config" lang:typescript
```

### Supported Languages

See [Language Support](../architecture/language-support.md) for the complete list.

### Benefits

- Reduces noise in mixed-language codebases
- Narrows results by up to 75%
- Faster queries on large indexes

### Examples

```bash
# Search only Python files
codanna mcp semantic_search_with_context query:"test fixtures" lang:python

# Find TypeScript symbols only
codanna mcp search_symbols query:"Component" lang:typescript
```

## Result Streaming

MCP tools stream results for immediate feedback on large result sets.

### How It Works

Tools return results as they're found rather than waiting for completion:
```bash
# Results appear progressively
codanna mcp search_symbols query:"test_"
# Output streams: test_parser, test_validator, test_handler...
```

### Benefits

- See results immediately
- Cancel early if you find what you need
- Better UX for large codebases

## JSON Output

All MCP tools support `--json` flag for structured output.

### Usage

```bash
codanna mcp semantic_search_with_context query:"parser" --json
```

### Benefits

- Pipe to `jq` for filtering
- Parse in scripts
- Extract specific fields
- Chain with other tools

### Examples

```bash
# Extract symbol_id from results
codanna mcp semantic_search_with_context query:"config" --json | jq '.[0].symbol_id'

# Get just file paths
codanna mcp search_symbols query:"test" --json | jq -r '.[].file_path'

# Filter by score threshold
codanna mcp semantic_search_with_context query:"auth" --json | jq '.[] | select(.score > 0.7)'
```

## Agent Workflows

Recommended tool usage patterns for AI assistants.

### Tool Priority

**Tier 1 (Start Here):**
- `semantic_search_with_context` - Find relevant code with context
- `analyze_impact` - Map dependencies and change radius

**Tier 2 (Get Details):**
- `find_symbol` - Look up specific symbols
- `get_calls` - What does this function call?
- `find_callers` - Who calls this function?

**Tier 3 (Browse/Explore):**
- `search_symbols` - Find by name pattern
- `semantic_search_docs` - Search documentation only
- `get_index_info` - Index statistics

### Standard Workflow

```bash
# 1. Find relevant code
codanna mcp semantic_search_with_context query:"authentication logic"

# 2. Map impact radius
codanna mcp analyze_impact authenticate_user

# 3. Get specific details
codanna mcp find_callers symbol_id:1234
codanna mcp get_calls symbol_id:1234
```

### When to Use Each Tool

- **Start refactor/fix** → `semantic_search_with_context` + `analyze_impact`
- **Understand function** → `get_calls` + `find_callers`
- **Find by name** → `search_symbols`
- **Check index status** → `get_index_info`

## Why Start with Semantic Search?

### Context Over Names

`semantic_search_with_context` returns symbols WITH their relationships, dependencies, and impact radius. `search_symbols` returns just names. Starting with context means fewer round trips and better understanding.

### Conceptual Links

Semantic search finds code that's conceptually related but not directly connected through calls or imports. Two functions handling authentication in different ways, error recovery patterns across modules, or validation logic scattered through layers - semantic search connects them based on what they do, not how they're structured.

This is particularly valuable when:
- Refactoring cross-cutting concerns
- Finding similar patterns for consistency
- Understanding distributed functionality
- Discovering implicit dependencies

### Working with Undocumented Code

**The Reality:** Many projects have minimal or no documentation. Semantic search needs documentation comments to work effectively.

**The Investment:** Spending time to document public APIs and critical paths pays immediate dividends:

1. **Use an AI agent to document systematically:**
   ```bash
   # Find undocumented public functions
   codanna mcp search_symbols query:"pub fn" kind:function

   # Agent adds doc comments following your standards
   # (Clean Code principles, API documentation guidelines)
   ```

2. **Focus on:**
   - Public APIs and exported symbols
   - Entry points and critical paths
   - Complex business logic
   - Error handling patterns

3. **Standards to follow:**
   - What the function does (not how)
   - Parameters and return values
   - Error conditions
   - Usage examples for complex APIs

**The Payoff:** Once documented, semantic search becomes exponentially more useful. "authentication flow" finds all auth-related code. "retry logic" discovers resilience patterns. "validation" connects input checking across layers.

**Practical Tip:** Don't document everything at once. Start with the area you're currently working in. Each documented module makes the next search more effective.

## See Also

- [MCP Tools Reference](../user-guide/mcp-tools.md) - Complete tool documentation
- [Search Guide](../user-guide/search-guide.md) - Search strategies and examples
- [Agent Guidance](../integrations/agent-guidance.md) - AI assistant integration
