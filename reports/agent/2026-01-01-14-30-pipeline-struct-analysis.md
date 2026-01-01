# Research Report: Pipeline Struct Analysis

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The `Pipeline` struct in `src/indexing/pipeline/mod.rs` is a parallel indexing system that orchestrates 5-stage Phase 1 indexing and 3-stage Phase 2 resolution. It provides `index_incremental` as the production entry point, handling new/modified/deleted files with semantic search integration.

## Key Findings

### 1. Pipeline Struct Definition

The struct is minimal, holding only configuration and settings:

```rust
pub struct Pipeline {
    settings: Arc<Settings>,
    config: PipelineConfig,
}
```

**Evidence**: `src/indexing/pipeline/mod.rs:64-67`

### 2. Public Methods

The Pipeline exposes 8 public methods:

| Method | Purpose |
|--------|---------|
| `new(settings, config)` | Create pipeline with explicit config |
| `with_settings(settings)` | Create pipeline with config derived from settings |
| `config()` | Get PipelineConfig reference |
| `settings()` | Get Settings reference |
| `index_directory(root, index)` | Phase 1: parallel indexing (internal) |
| `index_directory_with_progress(root, index, progress)` | Phase 1 with progress callback |
| `run_phase2(unresolved, symbol_cache, index)` | Phase 2: resolve relationships |
| `index_and_resolve(root, index)` | Run Phase 1 + Phase 2 in sequence |
| `index_incremental(root, index, semantic, force)` | Production entry point |

**Evidence**: `src/indexing/pipeline/mod.rs:70-103` (constructors), `104-245` (index_directory), `272-373` (run_phase2), `377-394` (index_and_resolve), `396-510` (index_incremental)

### 3. Pipeline Stages

**Phase 1 Stages** (parallel):
```
DISCOVER -> READ -> PARSE -> COLLECT -> INDEX
   |          |       |        |         |
   v          v       v        v         v
[paths]   [content] [parsed] [batch]   Tantivy
```

- **DISCOVER**: Parallel file system walk (4 walker threads)
- **READ**: Multi-threaded file reading (configurable, default 2 threads)
- **PARSE**: Parallel parsing with thread-local parser pools (N-2 CPU threads)
- **COLLECT**: Single-threaded ID assignment and batching
- **INDEX**: Single-threaded Tantivy writes with periodic commits

**Phase 2 Stages** (sequential):
- **CONTEXT**: Builds resolution contexts from unresolved relationships
- **RESOLVE**: Two-pass resolution (Pass 1: Defines, commit barrier, Pass 2: Calls)
- **WRITE**: Writes resolved relationships to Tantivy

**Pre-Phase Stage**:
- **CLEANUP**: For incremental mode, removes deleted/modified file data

**Evidence**: `src/indexing/pipeline/stages/mod.rs:1-40`, `src/indexing/pipeline/mod.rs:1-30`

### 4. DocumentIndex and SimpleSemanticSearch Handling

The Pipeline accepts `DocumentIndex` and `SimpleSemanticSearch` as parameters:

```rust
pub fn index_incremental(
    &self,
    root: &Path,
    index: Arc<DocumentIndex>,
    semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
    force: bool,
) -> PipelineResult<IncrementalStats>
```

- `DocumentIndex` is wrapped in `Arc` for thread-safe sharing across stages
- `SimpleSemanticSearch` is wrapped in `Arc<Mutex<>>` for synchronized access
- IndexStage can optionally embed symbols during indexing via `.with_semantic()`
- After indexing, embeddings are saved to `settings.index_path/semantic`

**Evidence**: `src/indexing/pipeline/mod.rs:396-410` (signature), `450-456` (CleanupStage with semantic), `604-609` (IndexStage with semantic)

### 5. PipelineConfig Options

```rust
pub struct PipelineConfig {
    pub parse_threads: usize,       // Default: CPU count - 2
    pub read_threads: usize,        // Default: 2
    pub batch_size: usize,          // Default: 5000 symbols
    pub path_channel_size: usize,   // Default: 1000
    pub content_channel_size: usize,// Default: 100
    pub parsed_channel_size: usize, // Default: 1000
    pub batch_channel_size: usize,  // Default: 20
    pub batches_per_commit: usize,  // Default: 10
}
```

Configuration methods:
- `PipelineConfig::default()` - Balanced defaults
- `PipelineConfig::from_settings()` - Read from `.codanna/settings.toml`
- `PipelineConfig::small()` - Optimized for <1000 files
- `PipelineConfig::large()` - Optimized for >10000 files
- Builder methods: `with_parse_threads()`, `with_batch_size()`, `with_batches_per_commit()`

**Evidence**: `src/indexing/pipeline/config.rs:1-100`

## Architecture/Patterns

### Channel-Based Pipeline

Uses `crossbeam-channel` bounded channels for backpressure:
- `(path_tx, path_rx)` - DISCOVER to READ
- `(content_tx, content_rx)` - READ to PARSE
- `(parsed_tx, parsed_rx)` - PARSE to COLLECT
- `(batch_tx, batch_rx)` - COLLECT to INDEX

### SymbolLookupCache

In-memory cache built during Phase 1 for O(1) Phase 2 resolution:
- `by_id: DashMap<SymbolId, Symbol>` - Direct lookup
- `by_name: DashMap<Box<str>, Vec<SymbolId>>` - Candidate resolution
- `by_file_id: DashMap<FileId, Vec<SymbolId>>` - Local symbols

**Evidence**: `src/indexing/pipeline/types.rs:328-420`

### Statistics Types

- `IndexStats` - Phase 1 results (files_indexed, symbols_found, elapsed)
- `Phase2Stats` - Resolution results (defines_resolved, calls_resolved, unresolved)
- `IncrementalStats` - Full run results (new/modified/deleted counts, nested stats)
- `CleanupStats` - Deletion results (files_cleaned, symbols_removed, embeddings_removed)

**Evidence**: `src/indexing/pipeline/mod.rs:774-830`

## Capabilities Comparison: Pipeline vs SimpleIndexer

| Capability | Pipeline | SimpleIndexer |
|------------|----------|---------------|
| Parallel file walking | Yes (4 threads) | No |
| Parallel parsing | Yes (N-2 threads) | No |
| Incremental indexing | Yes (`index_incremental`) | Yes (`index_directory_with_force`) |
| Change detection | Yes (hash-based) | Yes (hash-based) |
| Semantic search | Yes (optional) | Yes (optional) |
| Two-phase resolution | Yes (Defines then Calls) | Single phase |
| Progress callback | Stub only | Yes |
| File cleanup | Yes (CleanupStage) | Yes (`remove_file`) |
| Transaction support | Via batched commits | Yes (`begin/commit/rollback`) |
| Symbol caching | SymbolLookupCache (DashMap) | Internal HashMap |

### Missing in Pipeline (present in SimpleIndexer)

1. `search()` - Full-text search (use DocumentIndex directly)
2. Relationship query methods (`get_called_functions`, `get_calling_functions`, etc.)
3. Symbol query methods (`get_symbol`, `find_symbol`, `get_symbols_by_file`)
4. `sync_with_config()` - Config file synchronization
5. `reindex_file_content()` - Re-index from string content

These are query operations that should use `DocumentIndex` directly after Pipeline indexing.

## Conclusions

The Pipeline struct is production-ready for replacing SimpleIndexer's indexing workflow:

1. **Use `Pipeline::index_incremental()`** as the main entry point for production indexing
2. **Pass `force: true`** for initial/full re-index, `force: false` for incremental
3. **Query operations** should use `DocumentIndex` methods directly (not through Pipeline)
4. **PipelineConfig** can be tuned via settings.toml or preset profiles
5. **Progress reporting** needs implementation (currently a stub)

The Pipeline does not replace SimpleIndexer's query API - it only handles the indexing workflow. After indexing, use DocumentIndex for searches and relationship queries.
