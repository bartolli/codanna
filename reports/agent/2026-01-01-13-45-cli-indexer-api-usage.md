# Research Report: CLI Commands and Indexer API Usage

**Date**: 2026-01-01 13:45
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

This report documents which CLI commands use `SimpleIndexer`, `DocumentIndex`, or `Pipeline`, what methods they call, and how main.rs orchestrates indexer initialization and distribution.

## Command Architecture Overview

### Indexer Initialization Flow (main.rs)

The `needs_indexer` flag in `main.rs:258-272` determines which commands receive a `SimpleIndexer` instance:

**Commands that DO NOT need SimpleIndexer:**
- `Init`
- `Config`
- `Parse`
- `McpTest`
- `Benchmark`
- `AddDir`
- `RemoveDir`
- `ListDirs`
- `Plugin`
- `Documents`
- `Profile`
- `IndexParallel` (uses Pipeline instead)

**Commands that REQUIRE SimpleIndexer:**
- `Index`
- `Retrieve`
- `Mcp`
- `Serve`

**Evidence**: `src/main.rs:258-272`

```rust
let needs_indexer = !matches!(
    &cli.command,
    Commands::Init { .. }
        | Commands::Config
        | Commands::Parse { .. }
        | Commands::McpTest { .. }
        | Commands::Benchmark { .. }
        | Commands::AddDir { .. }
        | Commands::RemoveDir { .. }
        | Commands::ListDirs
        | Commands::Plugin { .. }
        | Commands::Documents { .. }
        | Commands::Profile { .. }
        | Commands::IndexParallel { .. }
);
```

## Detailed Command Analysis

### 1. index.rs - Main Index Command

**Uses**: `SimpleIndexer` (passed from main.rs)

**API Methods Called**:
- `index_file_with_force(path, force)` - Index single file
- `index_directory_with_options(path, ...)` - Index directory with progress/limits
- `sync_with_config(stored_paths, config_paths, progress)` - Sync indexed paths

**Signature**: `pub fn run(args: IndexArgs, config: &mut Settings, indexer: &mut SimpleIndexer, persistence: &IndexPersistence, sync_made_changes: Option<bool>)`

**Evidence**: `src/cli/commands/index.rs:24-31`

### 2. index_parallel.rs - Parallel Pipeline Command

**Uses**: `Pipeline` + `DocumentIndex` (creates its own)

**Does NOT use SimpleIndexer** - this is the new parallel architecture.

**API Methods Called**:
- `DocumentIndex::new(&index_path, settings)` - Create Tantivy index
- `Pipeline::new(settings, config)` - Create pipeline
- `pipeline.index_incremental(path, index, semantic, force)` - Incremental indexing

**Key Difference**: Creates `Arc<DocumentIndex>` directly, bypassing SimpleIndexer entirely.

**Evidence**: `src/cli/commands/index_parallel.rs:64-79`

```rust
let index = match DocumentIndex::new(&index_path, settings) {
    Ok(idx) => Arc::new(idx),
    Err(e) => { ... }
};
// ...
let pipeline = Pipeline::new(Arc::clone(&settings_arc), config);
// ...
pipeline.index_incremental(path, Arc::clone(&index), semantic.clone(), force)
```

### 3. retrieve.rs - Symbol Retrieval Commands

**Uses**: `SimpleIndexer` (passed from main.rs, immutable reference)

**Signature**: `pub fn run(query: RetrieveQuery, indexer: &SimpleIndexer) -> ExitCode`

**API Methods Called (per subcommand)**:

| Subcommand | SimpleIndexer Methods |
|------------|----------------------|
| `symbol` | `find_symbols_by_name()`, `get_symbol()`, `get_symbol_context()` |
| `callers` | `find_symbols_by_name()`, `get_symbol()`, `get_calling_functions_with_metadata()` |
| `calls` | `find_symbols_by_name()`, `get_symbol()`, `get_called_functions_with_metadata()` |
| `implementations` | `find_symbols_by_name()`, `get_implementations()` |
| `search` | `search()` (Tantivy text search) |
| `describe` | `find_symbols_by_name()`, `get_symbol()`, `get_symbol_context()` |

**Evidence**: `src/cli/commands/retrieve.rs:8-11` and `src/retrieve.rs:12-150`

### 4. mcp.rs - MCP Tool Invocation

**Uses**: `SimpleIndexer` (passed from main.rs, owned)

**Signature**: `pub async fn run(tool: String, ..., indexer: SimpleIndexer, config: &Settings)`

**API Methods Called (per tool)**:

| Tool | SimpleIndexer Methods |
|------|----------------------|
| `find_symbol` | `find_symbols_by_name()`, `get_symbol_context()` |
| `get_calls` | `get_symbol()`, `get_symbol_context()` (with CALLS) |
| `find_callers` | `get_symbol()`, `get_calling_functions_with_metadata()` |
| `analyze_impact` | `get_symbol()`, `find_symbols_by_name()`, `get_impact_radius()` |
| `semantic_search_*` | `semantic_search_docs_with_threshold_and_language()` |
| `search_symbols` | `search()` |
| `index_info` | `symbol_count()`, `file_count()`, `relationship_count()`, `get_semantic_metadata()` |

**Evidence**: `src/cli/commands/mcp.rs:40-350`

### 5. serve.rs - MCP Server

**Uses**: `SimpleIndexer` (passed from main.rs, owned)

**Signature**: `pub async fn run(args: ServeArgs, config: Settings, settings: Arc<Settings>, indexer: SimpleIndexer, index_path: PathBuf)`

**Note**: Server modes (stdio, HTTP, HTTPS) use the indexer for handling MCP tool calls internally.

**Evidence**: `src/cli/commands/serve.rs:22-30`

### 6. documents.rs - Document Management

**Uses**: `DocumentStore` (creates its own, NOT SimpleIndexer)

**API Methods Called**:
- `DocumentStore::new(&doc_path, dimension)` - Create store
- `store.with_embeddings(Box::new(generator))` - Enable embeddings
- `store.search(query)` - Search documents
- `store.list_collections()` - List indexed collections
- `store.collection_stats(name)` - Get collection statistics

**Evidence**: `src/cli/commands/documents.rs:1-50`

## API Surface Summary

### SimpleIndexer Methods Used by CLI

| Method | Commands Using It |
|--------|------------------|
| `index_file_with_force()` | index |
| `index_directory_with_options()` | index |
| `sync_with_config()` | index (via main.rs) |
| `find_symbols_by_name()` | retrieve, mcp |
| `get_symbol()` | retrieve, mcp |
| `get_symbol_context()` | retrieve, mcp |
| `get_calling_functions_with_metadata()` | retrieve, mcp |
| `get_called_functions_with_metadata()` | retrieve, mcp |
| `get_implementations()` | retrieve |
| `get_impact_radius()` | mcp |
| `search()` | retrieve, mcp |
| `semantic_search_docs_with_threshold_and_language()` | mcp |
| `symbol_count()` | mcp (index_info) |
| `file_count()` | mcp (index_info) |
| `relationship_count()` | mcp (index_info) |
| `get_semantic_metadata()` | mcp (index_info) |

### Pipeline Methods Used by CLI

| Method | Commands Using It |
|--------|------------------|
| `Pipeline::new()` | index-parallel |
| `index_incremental()` | index-parallel |

### DocumentIndex Methods Used by CLI

| Method | Commands Using It |
|--------|------------------|
| `DocumentIndex::new()` | index-parallel |
| (passed to Pipeline) | index-parallel |

## Architectural Observations

1. **Dual Indexing Paths**: The codebase has two distinct indexing approaches:
   - `index` command: Uses `SimpleIndexer` with its internal indexing logic
   - `index-parallel` command: Uses `Pipeline` + `DocumentIndex` directly

2. **Ownership Patterns**:
   - `index`: Takes `&mut SimpleIndexer` (mutable borrow)
   - `retrieve`: Takes `&SimpleIndexer` (immutable borrow)
   - `mcp`/`serve`: Takes `SimpleIndexer` (owned)
   - `index-parallel`: Creates `Arc<DocumentIndex>` internally

3. **SimpleIndexer Independence**: Commands that don't need symbol relationships (documents, parse, benchmark) bypass SimpleIndexer entirely.

4. **Pipeline as Alternative**: The `index-parallel` command demonstrates an alternative architecture that works directly with `DocumentIndex`, suggesting a potential migration path away from `SimpleIndexer` for indexing operations.

## Conclusions

1. The `index` command is the primary consumer of `SimpleIndexer`'s write APIs.
2. The `retrieve` and `mcp` commands are the primary consumers of query APIs.
3. The `index-parallel` command provides an alternative path using `Pipeline` + `DocumentIndex`.
4. Commands that need only document storage (not symbol relationships) use `DocumentStore` directly.
5. The CLI architecture cleanly separates concerns: indexing (`index`, `index-parallel`), querying (`retrieve`, `mcp`), and serving (`serve`).
