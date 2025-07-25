# Codebase Intelligence CLI Documentation

A high-performance code intelligence system for understanding codebases.

## Installation

```bash
# Build from source
cargo build --release

# The binary will be available at:
./target/release/codanna
```

## Quick Start with Target Binary

```bash
# Initialize configuration
./target/release/codanna init

# Index current directory with progress
./target/release/codanna index . --progress

# Find a symbol
./target/release/codanna retrieve symbol main

# Show function calls
./target/release/codanna retrieve calls main

# Search for symbols
./target/release/codanna retrieve search "parse" --limit 5
```

## Commands Overview

```
codanna <COMMAND>

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
codanna init [OPTIONS]
```

**Options:**
- `-f, --force` - Force overwrite existing configuration

**Example:**
```bash
# Create initial configuration
codanna init

# Overwrite existing configuration
codanna init --force
```

**Local Development:**
```bash
# Create initial configuration
./target/release/codanna init

# Overwrite existing configuration
./target/release/codanna init --force
```

Creates `.codanna/settings.toml` with default configuration.

### `index` - Index Source Files

Index source files or entire directories for code intelligence analysis.

```bash
codanna index [OPTIONS] <PATH>
```

**Arguments:**
- `<PATH>` - Path to file or directory to index

**Options:**
- `-t, --threads <THREADS>` - Number of threads to use (overrides config)
- `-f, --force` - Force re-indexing even if index exists
- `-p, --progress` - Show progress during indexing
- `--dry-run` - Show what would be indexed without actually indexing
- `--max-files <MAX_FILES>` - Maximum number of files to index

**Incremental Indexing:**
The indexer uses SHA256 content hashing to track file changes:
- Unchanged files are automatically skipped (100x faster)
- Only modified files are re-parsed and re-indexed
- File hashes and UTC timestamps are stored for each indexed file
- Use `--force` to ignore hashes and re-index all files

**Examples:**
```bash
# Index a single file
codanna index src/main.rs

# Index entire directory with progress
codanna index src --progress

# Dry run to see what would be indexed
codanna index . --dry-run

# Index with limited files
codanna index src --max-files 100 --progress

# Force re-index with custom thread count
codanna index . --force --threads 8
```

**Local Development:**
```bash
# Index a single file
./target/release/codanna index src/main.rs

# Index entire directory with progress
./target/release/codanna index src --progress

# Dry run to see what would be indexed
./target/release/codanna index . --dry-run

# Index with limited files
./target/release/codanna index src --max-files 100 --progress

# Force re-index with custom thread count
./target/release/codanna index . --force --threads 8
```

### `retrieve` - Query the Index

Retrieve various information from the indexed codebase.

```bash
codanna retrieve <SUBCOMMAND>
```

#### Subcommands:

##### `symbol` - Find symbols by name
```bash
codanna retrieve symbol <NAME>
```

**Example:**
```bash
codanna retrieve symbol SimpleIndexer
```

**Local Development:**
```bash
./target/release/codanna retrieve symbol SimpleIndexer
```

##### `calls` - Show what functions a given function calls
```bash
codanna retrieve calls <FUNCTION>
```

**Example:**
```bash
codanna retrieve calls process_file
```

##### `callers` - Show what functions call a given function
```bash
codanna retrieve callers <FUNCTION>
```

**Example:**
```bash
codanna retrieve callers helper_function
```

##### `implementations` - Show what types implement a given trait
```bash
codanna retrieve implementations <TRAIT_NAME>
```

**Example:**
```bash
codanna retrieve implementations LanguageParser
```

##### `uses` - Show what types a given symbol uses
```bash
codanna retrieve uses <SYMBOL>
```

**Example:**
```bash
codanna retrieve uses SimpleIndexer
```

##### `impact` - Show the impact radius of changing a symbol
```bash
codanna retrieve impact [OPTIONS] <SYMBOL>
```

**Options:**
- `-d, --depth <DEPTH>` - Maximum depth to search (default: 5)

**Example:**
```bash
codanna retrieve impact parse_function --depth 3
```

##### `search` - Search for symbols using full-text search
```bash
codanna retrieve search [OPTIONS] <QUERY>
```

**Arguments:**
- `<QUERY>` - Search query (supports fuzzy matching for typos)

**Options:**
- `-l, --limit <LIMIT>` - Maximum number of results (default: 10)
- `-k, --kind <KIND>` - Filter by symbol kind (e.g., Function, Struct, Trait)
- `-m, --module <MODULE>` - Filter by module path

**Features:**
- Full-text search across symbol names, documentation, and signatures
- Fuzzy search with typo tolerance (e.g., "symbl" finds "symbol")
- Rich metadata in results (file path, line number, module path, documentation)
- Relevance-based scoring

**Examples:**
```bash
# Search for symbols containing "hash"
codanna retrieve search hash

# Search for functions with "test" in the name
codanna retrieve search test --kind function

# Search with fuzzy matching (finds "symbol" even with typo)
codanna retrieve search symbl

# Limit results and filter by module
codanna retrieve search parser --limit 5 --module "crate::parsing"
```

##### `defines` - Show what methods a type or trait defines
```bash
codanna retrieve defines <SYMBOL>
```

**Example:**
```bash
codanna retrieve defines LanguageParser
```

##### `dependencies` - Show comprehensive dependency analysis
```bash
codanna retrieve dependencies <SYMBOL>
```

**Example:**
```bash
codanna retrieve dependencies SimpleIndexer
```

### `config` - Show Configuration

Display the current configuration settings.

```bash
codanna config
```

**Example:**
```bash
codanna config
```

**Local Development:**
```bash
./target/release/codanna config
```

### `serve` - Start MCP Server

Start the Model Context Protocol (MCP) server for AI assistants.

```bash
codanna serve [OPTIONS]
```

**Options:**
- `-p, --port <PORT>` - Port to listen on (overrides config)

**Example:**
```bash
# Start with default settings
codanna serve

# Start on specific port
codanna serve --port 8080

# Test with MCP inspector
npx @modelcontextprotocol/inspector cargo run -- serve
```

### `mcp-test` - Test MCP Client

Test MCP client functionality by connecting to a server.

```bash
codanna mcp-test [OPTIONS]
```

**Options:**
- `--server-binary <PATH>` - Path to server binary (defaults to current binary)
- `--tool <TOOL_NAME>` - Tool to call (if not specified, lists available tools)
- `--args <JSON>` - Tool arguments as JSON

**Example:**
```bash
# List available tools
codanna mcp-test

# Test specific tool
codanna mcp-test --tool find_symbol --args '{"name": "main"}'
```

### `mcp` - Direct MCP Tool Calls

Call MCP tools directly without spawning a server (embedded mode).

```bash
codanna mcp <TOOL> [OPTIONS]
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
- `search_symbols` - Search for symbols using full-text search with fuzzy matching

**Examples:**
```bash
# Find a symbol
codanna mcp find_symbol --args '{"name": "parse"}'

# Get function calls
codanna mcp get_calls --args '{"function_name": "index_file"}'

# Analyze impact with custom depth
codanna mcp analyze_impact --args '{"symbol_name": "Symbol", "max_depth": 3}'

# Get index information
codanna mcp get_index_info

# Search for symbols with fuzzy matching
codanna mcp search_symbols --args '{"query": "parse", "limit": 5}'

# Search for functions only
codanna mcp search_symbols --args '{"query": "test", "limit": 3, "kind": "function"}'

# Search within a specific module
codanna mcp search_symbols --args '{"query": "new", "module": "crate::types", "limit": 5}'
```

**Local Development:**
```bash
# Find a symbol
./target/release/codanna mcp find_symbol --args '{"name": "parse"}'

# Get function calls
./target/release/codanna mcp get_calls --args '{"function_name": "index_file"}'

# Find callers
./target/release/codanna mcp find_callers --args '{"function_name": "main"}'

# Analyze impact with custom depth
./target/release/codanna mcp analyze_impact --args '{"symbol_name": "Symbol", "max_depth": 3}'

# Get index information
./target/release/codanna mcp get_index_info

# Search for symbols with fuzzy matching
./target/release/codanna mcp search_symbols --args '{"query": "parse", "limit": 5}'

# Search for functions only
./target/release/codanna mcp search_symbols --args '{"query": "test", "limit": 3, "kind": "function"}'

# Search within a specific module
./target/release/codanna mcp search_symbols --args '{"query": "new", "module": "crate::types", "limit": 5}'

# Test with actual symbols from your codebase
./target/release/codanna mcp find_symbol --args '{"name": "SimpleIndexer"}'
./target/release/codanna mcp get_calls --args '{"function_name": "reindex_file_content"}'
./target/release/codanna mcp find_callers --args '{"function_name": "parse_file"}'
./target/release/codanna mcp search_symbols --args '{"query": "tantivy", "limit": 10}'
```

## Configuration File

The configuration file is located at `.codanna/settings.toml`:

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
path = ".codanna/index"
```

## Typical Workflow

1. **Initialize configuration**
   ```bash
   codanna init
   ```

2. **Edit configuration** (optional)
   ```bash
   # Edit .codanna/settings.toml to customize settings
   ```

3. **Index your codebase**
   ```bash
   codanna index . --progress
   ```

4. **Query the index**
   ```bash
   # Find a symbol
   codanna retrieve symbol MyStruct
   
   # See what calls a function
   codanna retrieve callers important_function
   
   # Analyze impact of changes
   codanna retrieve impact core_function --depth 3
   ```

5. **Use with AI assistants** (optional)
   ```bash
   # Start MCP server
   codanna serve
   ```

## Local Development Workflow

For testing during development, use the target binary:

1. **Initialize configuration**
   ```bash
   ./target/release/codanna init
   ```

2. **Index your codebase**
   ```bash
   ./target/release/codanna index . --progress
   ```

3. **Query the index**
   ```bash
   # Find a symbol
   ./target/release/codanna retrieve symbol MyStruct
   
   # See what calls a function
   ./target/release/codanna retrieve callers important_function
   
   # Test search functionality
   ./target/release/codanna retrieve search "parse" --limit 5
   ```

4. **Test MCP functionality**
   ```bash
   # Start MCP server
   ./target/release/codanna serve
   
   # Test MCP client
   ./target/release/codanna mcp-test
   
   # Test specific MCP tools directly
   ./target/release/codanna mcp find_symbol --args '{"name": "main"}'
   ./target/release/codanna mcp get_calls --args '{"function_name": "main"}'
   ./target/release/codanna mcp search_symbols --args '{"query": "index", "limit": 5}'
   ```

## Testing Scenarios

These examples demonstrate various indexing options and progress reporting:

### Basic Progress Reporting
```bash
# Index with real-time progress updates
codanna index src --progress

# Output shows:
# Indexing: 15/22 files (68%) - 21 files/s - ETA: 1s
```

### Dry Run Testing
```bash
# Preview what would be indexed without actually indexing
codanna index . --dry-run

# Shows list of files that would be indexed:
# Would index 31 files:
#   ./demo/example.rs
#   ./tests/cli_config_test.rs
#   ... and 29 more files
```

### Limited File Indexing
```bash
# Index only first 5 files (useful for testing)
codanna index src --max-files 5 --progress

# Index limited files with dry run
codanna index src --dry-run --max-files 10
```

### Force Re-indexing
```bash
# Force complete re-index with progress
codanna index . --force --progress

# Combine multiple options
codanna index src --force --max-files 20 --progress
```

### Performance Testing
```bash
# Index large directory to see performance metrics
codanna index /path/to/large/project --progress

# Output includes performance stats:
# Indexing Complete:
#   Files indexed: 1000
#   Performance: 25 files/second
#   Average symbols/file: 12.3
```

### Error Handling
```bash
# Test with non-existent directory
codanna index nonexistent --progress
# Error: Path does not exist: nonexistent

# Test with file instead of directory
codanna index src/main.rs --progress
# Successfully indexes single file
```

## Performance Tips

- Use `--threads` to control parallelism based on your CPU
- Use `--dry-run` first on large codebases to estimate indexing time
- Use `--max-files` to index incrementally
- The index is persisted, so subsequent runs are faster unless `--force` is used
- Incremental indexing automatically skips unchanged files (100x faster)
- Only modified files are re-parsed based on SHA256 content hashes

## Troubleshooting

- If indexing fails, check file permissions and ensure files are valid UTF-8
- Use `--dry-run` to debug which files would be indexed
- Check `.gitignore` rules if expected files aren't being indexed
- Run `codanna config` to verify your settings