# MCP Server Demo

## Setup

1. First, index some code:
```bash
cargo run -- index src/main.rs
```

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