# MCP Tools Reference

Available tools when using the MCP server. All tools support `--json` flag for structured output.

## Tool Categories

### Discovery Tools
- **find_symbol** - Find symbol by exact name
- **search_symbols** - Full-text search with fuzzy matching
- **semantic_search_docs** - Natural language search
- **semantic_search_with_context** - Natural language search with relationships

### Relationship Tools
- **get_calls** - Functions called by a function
- **find_callers** - Functions that call a function
- **analyze_impact** - Impact radius of symbol changes

### Document Tools
- **search_documents** - Search indexed markdown/text files

### Information Tools
- **get_index_info** - Index statistics

## Tool Details

### `find_symbol`

Find a symbol by exact name.

**Parameters:**
- `name` (required) - Exact symbol name to find

**Example:**
```bash
codanna mcp find_symbol main
codanna mcp find_symbol Parser --json
```

**Returns:** Symbol information including file path, line number, kind, and signature.

### `search_symbols`

Search symbols with full-text fuzzy matching.

**Parameters:**
- `query` (required) - Search query (supports fuzzy matching)
- `limit` - Maximum number of results (default: 10)
- `kind` - Filter by symbol kind (e.g., "Function", "Struct", "Trait")
- `module` - Filter by module path

**Example:**
```bash
codanna mcp search_symbols query:parse kind:function limit:10
codanna mcp search_symbols query:Parser --json
```

**Returns:** List of matching symbols with relevance ranking.

### `semantic_search_docs`

Search using natural language queries.

**Parameters:**
- `query` (required) - Natural language search query
- `limit` - Maximum number of results (default: 10)
- `threshold` - Minimum similarity score (0-1)
- `lang` - Filter by programming language (e.g., "rust", "typescript")

**Example:**
```bash
codanna mcp semantic_search_docs query:"error handling" limit:5
codanna mcp semantic_search_docs query:"authentication" lang:rust limit:5
```

**Returns:** Semantically similar symbols based on documentation.

### `semantic_search_with_context`

Natural language search with enhanced context including relationships.

**Parameters:**
- `query` (required) - Natural language search query
- `limit` - Maximum number of results (default: 5, as each includes full context)
- `threshold` - Minimum similarity score (0-1)
- `lang` - Filter by programming language

**Example:**
```bash
codanna mcp semantic_search_with_context query:"parse files" threshold:0.7
codanna mcp semantic_search_with_context query:"parse config" lang:typescript limit:3
```

**Returns:** Symbols with:
- Their documentation
- What calls them
- What they call
- Complete impact graph (includes ALL relationships: calls, type usage, composition)

### `get_calls`

Show functions called by a given function.

**Parameters:**
- `function_name` OR `symbol_id` (one required) - Function name or symbol ID

**Example:**
```bash
codanna mcp get_calls process_file
codanna mcp get_calls symbol_id:1883
codanna mcp get_calls main --json
```

**Returns:** List of functions that the specified function calls. Each result includes `[symbol_id:123]` for follow-up queries.

### `find_callers`

Show functions that call a given function.

**Parameters:**
- `function_name` OR `symbol_id` (one required) - Function name or symbol ID

**Example:**
```bash
codanna mcp find_callers init
codanna mcp find_callers symbol_id:1883
codanna mcp find_callers parse_file --json
```

**Returns:** List of functions that call the specified function. Each result includes `[symbol_id:123]` for follow-up queries.

### `analyze_impact`

Analyze the impact radius of symbol changes.

**Parameters:**
- `symbol_name` OR `symbol_id` (one required) - Symbol name or symbol ID
- `max_depth` - Maximum depth to search (default: 3)

**Example:**
```bash
codanna mcp analyze_impact Parser
codanna mcp analyze_impact symbol_id:1883
codanna mcp analyze_impact SimpleIndexer --json
```

**Returns:** Complete dependency graph showing:
- What CALLS this function
- What USES this as a type (fields, parameters, returns)
- What RENDERS/COMPOSES this (JSX: `<Component>`, Rust: struct fields, etc.)
- Full dependency graph across files
- Each result includes `[symbol_id:123]` for unambiguous follow-up

### `search_documents`

Search indexed documents (markdown, text files) using natural language queries.

**Parameters:**
- `query` (required) - Natural language search query
- `collection` - Filter by collection name (optional)
- `limit` - Maximum number of results (default: 5)

**Example:**
```bash
codanna mcp search_documents query:"authentication flow"
codanna mcp search_documents query:"error handling" collection:docs limit:3
codanna mcp search_documents query:"getting started" --json
```

**Returns:** Matching document chunks with:
- Source file path and similarity score
- Heading context (document structure)
- KWIC preview centered on keywords with highlighting

**Note:** Requires document collections to be indexed first. See [Document Search](documents.md).

### `get_index_info`

Get index statistics and metadata.

**Parameters:** None

**Example:**
```bash
codanna mcp get_index_info
codanna mcp get_index_info --json
```

**Returns:**
- Total symbols indexed
- Symbols by language
- Symbols by kind
- Index creation/update timestamps
- File count

## Understanding Relationship Types

**Calls:** Function invocation (`functionA()` invokes `functionB()`) - shown by `get_calls`, `find_callers`

**Uses:** Type dependencies, composition, rendering (parameters, JSX components, struct fields) - shown by `analyze_impact`

## Language Filtering

Add `lang:rust` to any search tool to filter by language. Reduces search space by up to 75% in mixed codebases.

See [Language Filtering](../reference/concepts.md#language-filtering) for supported languages.

## JSON Output

All tools support `--json` flag for structured output. Pipe to `jq` for filtering and extraction.

See [JSON Output](../reference/concepts.md#json-output) for examples.

## Using symbol_id

All tools return `[symbol_id:123]` for unambiguous follow-up queries. Use IDs instead of names to avoid disambiguation and enable direct lookups.

See [symbol_id](../reference/concepts.md#symbol_id) for workflow patterns.

## Tool Workflow

### Recommended Approach

Start with `semantic_search_with_context` or `analyze_impact` for complete context. Use `get_calls`/`find_callers` for specific invocations. Chain queries using `symbol_id` from results.

See [Agent Workflows](../reference/concepts.md#agent-workflows) for detailed tool priority and patterns.

## System Messages

Each tool response includes hidden guidance messages for AI assistants. See [Agent Guidance](../integrations/agent-guidance.md) for configuration.

## See Also

- [CLI Reference](cli-reference.md#codanna-mcp-tool-positional) - Command-line usage
- [Unix Piping](../advanced/unix-piping.md) - Advanced piping workflows
- [Agent Guidance](../integrations/agent-guidance.md) - Configuring system messages