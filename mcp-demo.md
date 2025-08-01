# MCP Server Demo

## Setup

1. First, index some code:
```bash
cargo run -- index src/main.rs
```

**Note**: If you want to use semantic search features, ensure semantic search is enabled in your settings.toml before indexing (see Semantic Search Setup below).

2. Start the MCP server:
```bash
cargo run -- serve
```

## Available Tools

The MCP server exposes these tools:

1. **find_symbol** - Find a symbol by name in the indexed codebase
   - Parameters: `name` (string)

2. **get_calls** - Get all functions that a given function calls
   - Parameters: `function_name` (string)

3. **find_callers** - Find all functions that call a given function
   - Parameters: `function_name` (string)

4. **analyze_impact** - Analyze the impact radius of changing a symbol
   - Parameters: `symbol_name` (string), `max_depth` (number, default: 3)

5. **get_index_info** - Get information about the indexed codebase
   - No parameters

6. **search_symbols** - Search for symbols using full-text search with fuzzy matching
   - Parameters: `query` (string), `limit` (number, default: 10), `kind` (string, optional), `module` (string, optional)

7. **semantic_search_docs** - Search documentation using natural language
   - Parameters: `query` (string), `limit` (number, default: 10), `threshold` (number, optional: 0.0-1.0)
   - Requires semantic search to be enabled

8. **semantic_search_with_context** - Enhanced semantic search with full context (dependencies, callers, impact)
   - Parameters: `query` (string), `limit` (number, default: 5), `threshold` (number, optional: 0.0-1.0)
   - Requires semantic search to be enabled
   - Returns comprehensive information including:
     - Symbol location with file:line
     - Documentation
     - Dependencies (what it calls)
     - Callers (what calls it)
     - Impact analysis (what would be affected)

## Semantic Search Setup

To enable semantic search features, you need to configure it in your `.codanna/settings.toml`:

```toml
[semantic_search]
enabled = true
model = "AllMiniLML6V2"  # Currently the only supported model
threshold = 0.6          # Optional: default similarity threshold (0.0-1.0)
```

If you've already indexed your code without semantic search enabled, you'll need to re-index:

```bash
# Re-index with semantic search enabled
cargo run -- index src --force

# Or for a full project re-index
cargo run -- index . --force
```

**Note**: Semantic search adds ~1.5KB per symbol for embeddings. Initial indexing will be slower as embeddings are generated.

## Testing with MCP Inspector

### Option 1: Use the installed binary (recommended)

First install the binary:
```bash
cargo install --path .
```

Then run the inspector:
```bash
npx @modelcontextprotocol/inspector codanna serve
```

### Option 2: Use the wrapper script

Due to argument parsing issues with cargo run, use the wrapper script:
```bash
npx @modelcontextprotocol/inspector ./mcp-server.sh
```

## Integration with Claude Desktop

Add to your Claude Desktop config:

### Basic Configuration
```json
{
  "mcpServers": {
    "codanna": {
      "command": "/path/to/codanna",
      "args": ["serve"]
    }
  }
}
```

### Advanced Configuration with Custom Settings
```json
{
  "mcpServers": {
    "codanna": {
      "command": "/Users/yourusername/.cargo/bin/codanna",
      "args": ["serve", "--config", "/path/to/custom-settings.toml"],
      "env": {
        "RUST_LOG": "warn",
        "CODANNA_INDEX_PATH": "/path/to/index"
      }
    }
  }
}
```

### Configuration with Semantic Search
For projects using semantic search, ensure your settings.toml has semantic search enabled:

```json
{
  "mcpServers": {
    "codanna-myproject": {
      "command": "/Users/yourusername/.cargo/bin/codanna",
      "args": ["serve"],
      "cwd": "/path/to/myproject",
      "env": {
        "RUST_LOG": "warn"
      }
    }
  }
}
```

The server will use the `.codanna/settings.toml` in your project directory, which should include:
```toml
[semantic_search]
enabled = true
model = "AllMiniLML6V2"
threshold = 0.6
```

## Advanced Examples

### Using Semantic Search Tools

#### Basic Semantic Search
Find documentation using natural language:
```bash
# Search for JSON parsing functionality
codanna mcp semantic_search_docs --args '{"query": "parse JSON data", "limit": 5}'

# Search with higher precision threshold
codanna mcp semantic_search_docs --args '{"query": "database connection", "threshold": 0.7, "limit": 10}'
```

#### Enhanced Context Search (The Powerhorse)
Get comprehensive information in a single query:
```bash
# Find authentication code with full context
codanna mcp semantic_search_with_context --args '{"query": "handle user authentication", "limit": 3}'

# Search for error handling with dependencies and impact
codanna mcp semantic_search_with_context --args '{"query": "error handling and recovery", "threshold": 0.6}'
```

### Comparing Tool Outputs

Traditional approach (multiple calls):
```bash
# 1. Find the symbol
codanna mcp find_symbol --args '{"name": "parse_config"}'
# 2. Get what it calls
codanna mcp get_calls --args '{"function_name": "parse_config"}'
# 3. Get what calls it
codanna mcp find_callers --args '{"function_name": "parse_config"}'
# 4. Analyze impact
codanna mcp analyze_impact --args '{"symbol_name": "parse_config", "max_depth": 2}'
```

New approach (single call with context):
```bash
# Get everything at once!
codanna mcp semantic_search_with_context --args '{"query": "configuration parsing", "limit": 1}'
```

## Testing Semantic Search

### Expected Output Format

#### semantic_search_docs output:
```
Found 2 semantically similar result(s) for 'parse JSON':

1. parse_json (Function) - Similarity: 0.892
   File: src/parser/json.rs:45
   Doc: Parse JSON data from a string and return structured data...
   
2. validate_json_schema (Function) - Similarity: 0.743
   File: src/validator.rs:12
   Doc: Validate JSON data against a schema...
```

#### semantic_search_with_context output:
```
Found 1 results for query: 'parse JSON data'

1. parse_json - Function at src/parser/json.rs:45
   Similarity Score: 0.892
   Documentation:
     Parse JSON data from a string and return structured data
     
   parse_json calls 3 function(s):
     -> Function tokenize_json at src/lexer.rs:23
     -> Function build_ast at src/ast.rs:167
     -> Function validate_schema at src/validator.rs:89

   2 function(s) call parse_json:
     <- Function handle_api_request at src/api/handler.rs:34
     <- Function load_config at src/config.rs:12

   Changing parse_json would impact 5 symbol(s) (max depth: 2):
   
     functions (3):
       - handle_api_request
       - load_config
       - test_json_parser
       
     methods (2):
       - ApiServer::process_request
       - ConfigManager::reload
```

### Testing with MCP Inspector

When using MCP Inspector with semantic search tools:

```bash
# Make sure semantic search is enabled first
cat .codanna/settings.toml | grep -A3 semantic_search

# Run inspector
npx @modelcontextprotocol/inspector codanna serve

# In the inspector, try these queries:
# 1. Tool: semantic_search_docs
#    Args: {"query": "parse configuration", "limit": 5}
#
# 2. Tool: semantic_search_with_context  
#    Args: {"query": "database operations", "limit": 2, "threshold": 0.6}
```