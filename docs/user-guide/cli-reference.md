# CLI Reference

Complete listing of all Codanna commands and options.

## Global Options

Available for all commands:
- `-c, --config <CONFIG>` - Path to custom settings.toml file
- `--info` - Show detailed loading information
- `-h, --help` - Print help
- `-V, --version` - Print version

## Top-Level Commands

| Command | Description |
|---------|-------------|
| `codanna init` | Set up .codanna directory with default configuration |
| `codanna index` | Build searchable index from codebase |
| `codanna add-dir` | Add a folder to be indexed |
| `codanna remove-dir` | Remove a folder from indexed paths |
| `codanna list-dirs` | List all folders that are being indexed |
| `codanna retrieve` | Query symbols, relationships, and dependencies |
| `codanna serve` | Start MCP server |
| `codanna config` | Display active settings |
| `codanna mcp-test` | Test MCP connection |
| `codanna mcp` | Execute MCP tools directly |
| `codanna benchmark` | Benchmark parser performance |
| `codanna parse` | Output AST nodes in JSONL format |
| `codanna plugin` | Manage Claude Code plugins |

## Command Details

`codanna init`
Set up .codanna directory with default configuration

**Options:**
- `-f, --force` - Force overwrite existing configuration

`codanna index [PATHS...]`
Build searchable index from codebase

**Arguments:**
- `[PATHS...]` - Paths to files or directories to index (multiple paths allowed)
- If no paths provided, uses `indexed_paths` from configuration (must be configured via `add-dir`)

**Options:**
- `-t, --threads <THREADS>` - Number of threads to use (overrides config)
- `-f, --force` - Force re-indexing even if index exists
- `-p, --progress` - Show progress during indexing
- `--dry-run` - Dry run - show what would be indexed without indexing
- `--max-files <MAX_FILES>` - Maximum number of files to index

**Examples:**
```bash
# Index a single directory
codanna index src --progress

# Index multiple directories at once
codanna index src lib tests --progress

# Use configured indexed paths
codanna index --progress
```

**Behavior:**
- Accepts multiple paths for indexing in a single operation
- When run without arguments, uses folders from `indexed_paths` configuration
- Automatically cleans up symbols from removed folders when using configuration
- Backward compatible with single-path usage

`codanna add-dir <PATH>`
Add a folder to indexed paths in settings.toml

**Arguments:**
- `<PATH>` - Path to folder (canonicalized to absolute)

**Examples:**
```bash
codanna add-dir /path/to/project
codanna add-dir src
```

**Behavior:**
- Updates settings.toml (source of truth)
- Prevents duplicate entries
- Next command automatically indexes the folder

`codanna remove-dir <PATH>`
Remove a folder from indexed paths in settings.toml

**Arguments:**
- `<PATH>` - Path to folder (must exist in configuration)

**Examples:**
```bash
codanna remove-dir /path/to/old-project
codanna remove-dir tests
```

**Behavior:**
- Updates settings.toml (source of truth)
- Next command automatically removes symbols, embeddings, and metadata

`codanna list-dirs`
List configured indexed directories from settings.toml

**Example:**
```bash
codanna list-dirs
```

## Automatic Sync Mechanism

Every command compares settings.toml (source of truth) with index metadata:
- New paths in config → automatically indexed
- Removed paths → symbols, embeddings, and metadata cleaned

**Example:**
```bash
codanna add-dir examples/typescript
codanna retrieve symbol Button
# ✓ Added 1 new directories (5 files, 127 symbols)

codanna remove-dir examples/typescript
codanna retrieve symbol Button
# ✓ Removed 1 directories from index
```

Settings.toml can be edited manually - changes detected on next command.

`codanna retrieve <SUBCOMMAND>`
Query indexed symbols, relationships, and dependencies

**Subcommands:**
| Subcommand | Description |
|------------|-------------|
| `retrieve symbol` | Find a symbol by name |
| `retrieve calls` | Show what functions a given function calls (accepts `symbol_id:ID`) |
| `retrieve callers` | Show what functions call a given function (accepts `symbol_id:ID`) |
| `retrieve implementations` | Show what types implement a given trait |
| `retrieve uses` | Show what types a given symbol uses |
| `retrieve search` | Search for symbols using full-text search |
| `retrieve defines` | Show what methods a type or trait defines |
| `retrieve dependencies` | Show dependency analysis for a symbol |
| `retrieve describe` | Show information about a symbol (accepts `symbol_id:ID`) |

**All retrieve subcommands support:**
- `--json` - Output in JSON format

**Using symbol_id:**
```bash
# By name (may be ambiguous)
codanna retrieve calls process_file

# By ID (always unambiguous)
codanna retrieve calls symbol_id:1883

# Works with: calls, callers, describe
```

`codanna serve`
Start MCP server with optional HTTP/HTTPS modes

**Options:**
- `--watch` - Enable hot-reload when index changes
- `--watch-interval <WATCH_INTERVAL>` - How often to check for index changes (default: 5)
- `--http` - Run as HTTP server instead of stdio transport
- `--https` - Run as HTTPS server with TLS support
- `--bind <BIND>` - Address to bind HTTP/HTTPS server to (default: 127.0.0.1:8080)

`codanna config`
Display active settings

`codanna mcp-test`
Test MCP connection - verify connectivity and list available tools

`codanna mcp <TOOL> [POSITIONAL]...`
Execute MCP tools directly without spawning server

**Arguments:**
- `<TOOL>` - Tool to call
- `[POSITIONAL]...` - Positional arguments (can be simple values or key:value pairs)

**Options:**
- `--args <ARGS>` - Tool arguments as JSON (for backward compatibility and complex cases)
- `--json` - Output in JSON format

**Available Tools:**
| Tool | Description |
|------|-------------|
| `find_symbol` | Find symbol by exact name |
| `search_symbols` | Full-text search with fuzzy matching |
| `semantic_search_docs` | Natural language search |
| `semantic_search_with_context` | Natural language search with relationships |
| `get_calls` | Functions called by a function |
| `find_callers` | Functions that call a function |
| `analyze_impact` | Impact radius of symbol changes |
| `get_index_info` | Index statistics |

`codanna benchmark [LANGUAGE]`
Benchmark parser performance

**Arguments:**
- `[LANGUAGE]` - Language to benchmark (rust, python, typescript, go, php, c, cpp, all) [default: all]

**Options:**
- `-f, --file <FILE>` - Custom file to benchmark

`codanna parse <FILE>`
Parse file and output AST as JSON Lines

**Arguments:**
- `<FILE>` - File to parse

**Options:**
- `-o, --output <OUTPUT>` - Output file (defaults to stdout)
- `-d, --max-depth <MAX_DEPTH>` - Maximum depth to traverse
- `-a, --all-nodes` - Include all nodes (by default only named nodes are shown)

`codanna plugin <SUBCOMMAND>`
Manage Claude Code plugins by installing from Git-based marketplaces

> **Full Documentation:** See [Plugin System Documentation](../plugins/) for detailed usage, creating plugins, and marketplace structure.

**Subcommands:**
| Subcommand | Description |
|------------|-------------|
| `plugin add` | Install a plugin from a marketplace repository |
| `plugin remove` | Remove an installed plugin and clean up its files |
| `plugin update` | Update a plugin to a newer version |
| `plugin list` | List all installed plugins with their versions |
| `plugin verify` | Verify that a plugin's files match their expected checksums |

`plugin add <MARKETPLACE> <PLUGIN_NAME>`
Install a plugin from a marketplace repository

**Arguments:**
- `<MARKETPLACE>` - Marketplace repository URL or local path
- `<PLUGIN_NAME>` - Plugin name to install

**Options:**
- `--ref <REF>` - Git reference (branch, tag, or commit SHA)
- `-f, --force` - Force installation even if conflicts exist
- `--dry-run` - Perform a dry run without making changes

#`plugin remove <PLUGIN_NAME>`
Remove an installed plugin and clean up its files

**Arguments:**
- `<PLUGIN_NAME>` - Plugin name to remove

**Options:**
- `-f, --force` - Force removal even if other plugins depend on it
- `--dry-run` - Perform a dry run without making changes

`plugin update <PLUGIN_NAME>`
Update a plugin to a newer version

**Arguments:**
- `<PLUGIN_NAME>` - Plugin name to update

**Options:**
- `--ref <REF>` - Update to specific Git reference
- `--dry-run` - Perform a dry run without making changes

`plugin list`
List all installed plugins with their versions

`plugin verify <PLUGIN_NAME>`
Verify that a plugin's files match their expected checksums

**Arguments:**
- `<PLUGIN_NAME>` - Plugin name to verify

## Getting Help

To get detailed help for any command or subcommand:

```bash
# Top-level command help
codanna help <command>
codanna <command> --help

# Subcommand help
codanna help retrieve <subcommand>
codanna retrieve <subcommand> --help
codanna help plugin <subcommand>
codanna plugin <subcommand> --help
```

## Exit Codes

- `0` - Success
- `1` - General error
- `3` - Not found (used by retrieve commands)

## Notes

- All retrieve commands support `--json` flag for structured output
- MCP tools support both positional and key:value arguments
- Plugin command manages codanna extensions
- Use `--dry-run` with index, plugin add, and plugin remove to preview without making changes
- Language filtering available in semantic search: `lang:rust`, `lang:typescript`, etc.