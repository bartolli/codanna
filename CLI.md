# Codebase Intelligence CLI Documentation

A high-performance code intelligence system for understanding codebases.

## Installation

```bash
# Build from source
cargo build --release

# The binary will be available at:
./target/release/codebase-intelligence
```

## Commands Overview

```
codebase-intelligence <COMMAND>

Commands:
  init      Initialize configuration file
  index     Index source files or directories
  retrieve  Retrieve information from the index
  config    Show current configuration
  serve     Start MCP server for AI assistants
  mcp-test  Test MCP client functionality
  mcp       Call MCP tools directly without spawning a server
  help      Print this message or the help of the given subcommand(s)
```

## Detailed Command Reference

### `init` - Initialize Configuration

Creates a configuration file with default settings.

```bash
codebase-intelligence init [OPTIONS]
```

**Options:**
- `-f, --force` - Force overwrite existing configuration

**Example:**
```bash
# Create initial configuration
codebase-intelligence init

# Overwrite existing configuration
codebase-intelligence init --force
```

Creates `.code-intelligence/settings.toml` with default configuration.

### `index` - Index Source Files

Index source files or entire directories for code intelligence analysis.

```bash
codebase-intelligence index [OPTIONS] <PATH>
```

**Arguments:**
- `<PATH>` - Path to file or directory to index

**Options:**
- `-t, --threads <THREADS>` - Number of threads to use (overrides config)
- `-f, --force` - Force re-indexing even if index exists
- `-p, --progress` - Show progress during indexing
- `--dry-run` - Show what would be indexed without actually indexing
- `--max-files <MAX_FILES>` - Maximum number of files to index

**Examples:**
```bash
# Index a single file
codebase-intelligence index src/main.rs

# Index entire directory with progress
codebase-intelligence index src --progress

# Dry run to see what would be indexed
codebase-intelligence index . --dry-run

# Index with limited files
codebase-intelligence index src --max-files 100 --progress

# Force re-index with custom thread count
codebase-intelligence index . --force --threads 8
```

### `retrieve` - Query the Index

Retrieve various information from the indexed codebase.

```bash
codebase-intelligence retrieve <SUBCOMMAND>
```

#### Subcommands:

##### `symbol` - Find symbols by name
```bash
codebase-intelligence retrieve symbol <NAME>
```

**Example:**
```bash
codebase-intelligence retrieve symbol SimpleIndexer
```

##### `calls` - Show what functions a given function calls
```bash
codebase-intelligence retrieve calls <FUNCTION>
```

**Example:**
```bash
codebase-intelligence retrieve calls process_file
```

##### `callers` - Show what functions call a given function
```bash
codebase-intelligence retrieve callers <FUNCTION>
```

**Example:**
```bash
codebase-intelligence retrieve callers helper_function
```

##### `implementations` - Show what types implement a given trait
```bash
codebase-intelligence retrieve implementations <TRAIT_NAME>
```

**Example:**
```bash
codebase-intelligence retrieve implementations LanguageParser
```

##### `uses` - Show what types a given symbol uses
```bash
codebase-intelligence retrieve uses <SYMBOL>
```

**Example:**
```bash
codebase-intelligence retrieve uses SimpleIndexer
```

##### `impact` - Show the impact radius of changing a symbol
```bash
codebase-intelligence retrieve impact [OPTIONS] <SYMBOL>
```

**Options:**
- `-d, --depth <DEPTH>` - Maximum depth to search (default: 5)

**Example:**
```bash
codebase-intelligence retrieve impact parse_function --depth 3
```

##### `defines` - Show what methods a type or trait defines
```bash
codebase-intelligence retrieve defines <SYMBOL>
```

**Example:**
```bash
codebase-intelligence retrieve defines LanguageParser
```

##### `dependencies` - Show comprehensive dependency analysis
```bash
codebase-intelligence retrieve dependencies <SYMBOL>
```

**Example:**
```bash
codebase-intelligence retrieve dependencies SimpleIndexer
```

### `config` - Show Configuration

Display the current configuration settings.

```bash
codebase-intelligence config
```

**Example:**
```bash
codebase-intelligence config
```

### `serve` - Start MCP Server

Start the Model Context Protocol (MCP) server for AI assistants.

```bash
codebase-intelligence serve [OPTIONS]
```

**Options:**
- `-p, --port <PORT>` - Port to listen on (overrides config)

**Example:**
```bash
# Start with default settings
codebase-intelligence serve

# Start on specific port
codebase-intelligence serve --port 8080

# Test with MCP inspector
npx @modelcontextprotocol/inspector cargo run -- serve
```

### `mcp-test` - Test MCP Client

Test MCP client functionality by connecting to a server.

```bash
codebase-intelligence mcp-test [OPTIONS]
```

**Options:**
- `--server-binary <PATH>` - Path to server binary (defaults to current binary)
- `--tool <TOOL_NAME>` - Tool to call (if not specified, lists available tools)
- `--args <JSON>` - Tool arguments as JSON

**Example:**
```bash
# List available tools
codebase-intelligence mcp-test

# Test specific tool
codebase-intelligence mcp-test --tool find_symbol --args '{"name": "main"}'
```

### `mcp` - Direct MCP Tool Calls

Call MCP tools directly without spawning a server (embedded mode).

```bash
codebase-intelligence mcp <TOOL> [OPTIONS]
```

**Arguments:**
- `<TOOL>` - Tool to call

**Options:**
- `--args <JSON>` - Tool arguments as JSON object

**Available Tools:**
- `find_symbol` - Find a symbol by name
- `get_calls` - Get functions called by a function
- `find_callers` - Find functions that call a given function
- `analyze_impact` - Analyze impact of changing a symbol
- `get_index_info` - Get information about the index

**Examples:**
```bash
# Find a symbol
codebase-intelligence mcp find_symbol --args '{"name": "parse"}'

# Get function calls
codebase-intelligence mcp get_calls --args '{"function_name": "index_file"}'

# Analyze impact with custom depth
codebase-intelligence mcp analyze_impact --args '{"symbol_name": "Symbol", "max_depth": 3}'

# Get index information
codebase-intelligence mcp get_index_info
```

## Configuration File

The configuration file is located at `.code-intelligence/settings.toml`:

```toml
[indexing]
parallel_threads = 8
ignore_patterns = ["target/", "*.tmp"]

[languages.rust]
enabled = true

[languages.python]
enabled = false

[languages.javascript]
enabled = false

[languages.typescript]
enabled = false

[mcp]
enabled = true
port = 7777

[index]
type = "sqlite"
path = ".code-intelligence/index"
```

## Typical Workflow

1. **Initialize configuration**
   ```bash
   codebase-intelligence init
   ```

2. **Edit configuration** (optional)
   ```bash
   # Edit .code-intelligence/settings.toml to customize settings
   ```

3. **Index your codebase**
   ```bash
   codebase-intelligence index . --progress
   ```

4. **Query the index**
   ```bash
   # Find a symbol
   codebase-intelligence retrieve symbol MyStruct
   
   # See what calls a function
   codebase-intelligence retrieve callers important_function
   
   # Analyze impact of changes
   codebase-intelligence retrieve impact core_function --depth 3
   ```

5. **Use with AI assistants** (optional)
   ```bash
   # Start MCP server
   codebase-intelligence serve
   ```

## Performance Tips

- Use `--threads` to control parallelism based on your CPU
- Use `--dry-run` first on large codebases to estimate indexing time
- Use `--max-files` to index incrementally
- The index is persisted, so subsequent runs are faster unless `--force` is used

## Troubleshooting

- If indexing fails, check file permissions and ensure files are valid UTF-8
- Use `--dry-run` to debug which files would be indexed
- Check `.gitignore` rules if expected files aren't being indexed
- Run `codebase-intelligence config` to verify your settings