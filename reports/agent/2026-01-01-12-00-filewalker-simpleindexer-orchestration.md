# Research Report: FileWalker and SimpleIndexer Orchestration

**Date**: 2026-01-01 12:00
**Agent**: Research-Agent-v5
**Model**: Claude Opus 4.5

## Summary

The codebase implements two indexing paths: the production `SimpleIndexer` (sequential) and an experimental `Pipeline` (parallel). Both use `FileWalker` for file discovery and `DocumentIndex` (Tantivy) as the storage layer. The orchestration follows a discover-parse-index-resolve pattern with batch commits for performance.

## Key Findings

### 1. FileWalker: File Discovery

`FileWalker` discovers source files using the `ignore` crate with gitignore support.

**Location:** `/Users/bartolli/Projects/codanna/src/indexing/walker.rs:1-100`

**Core API:**
```rust
pub struct FileWalker {
    settings: Arc<Settings>,
}

impl FileWalker {
    pub fn new(settings: Arc<Settings>) -> Self;
    pub fn walk(&self, root: &Path) -> impl Iterator<Item = PathBuf>;
    pub fn count_files(&self, root: &Path) -> usize;
}
```

**Filtering Applied:**
1. Respects `.gitignore`, `.git/info/exclude`, and global gitignore
2. Supports custom `.codannaignore` files (follows gitignore syntax)
3. Filters by enabled file extensions from language registry
4. Excludes hidden files (starting with `.`)
5. Does not follow symlinks

**Evidence:** `/Users/bartolli/Projects/codanna/src/indexing/walker.rs:26-78`

### 2. SimpleIndexer Core Methods

**Location:** `/Users/bartolli/Projects/codanna/src/indexing/simple.rs`

#### `index_file()` and `index_file_with_force()`

```rust
pub fn index_file(&mut self, path: impl AsRef<Path>) -> IndexResult<IndexingResult>;
pub fn index_file_with_force(&mut self, path: impl AsRef<Path>, force: bool) -> IndexResult<IndexingResult>;
```

**Flow:**
1. Start Tantivy batch
2. Call `index_file_internal()` (handles hash checking, parsing, symbol extraction)
3. Commit batch
4. Resolve cross-file relationships

**Evidence:** `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:423-447`

#### `remove_file()`

```rust
pub fn remove_file(&mut self, path: impl AsRef<Path>) -> IndexResult<()>;
```

**Flow:**
1. Get `FileId` from Tantivy
2. Collect symbols to remove (for semantic search cleanup)
3. Start batch, delete imports for file
4. Remove all documents for file path
5. Remove embeddings from semantic search
6. Commit batch
7. Rebuild symbol cache

**Evidence:** `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:564-656`

#### `index_directory()` and `index_directory_with_options()`

```rust
pub fn index_directory(&mut self, dir: impl AsRef<Path>, progress: bool, dry_run: bool) -> IndexResult<IndexStats>;
pub fn index_directory_with_options(
    &mut self, dir: impl AsRef<Path>, progress: bool, dry_run: bool,
    force: bool, max_files: Option<usize>
) -> IndexResult<IndexStats>;
```

**Flow:**
1. Create `FileWalker`, collect all files
2. Start Tantivy batch
3. Process files one at a time via `index_file_internal()`
4. Commit batch every 100 files (`COMMIT_BATCH_SIZE`)
5. Resolve cross-file relationships after all files
6. Update stats

**Evidence:** `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2444-2573`

#### `sync_with_config()`

```rust
pub fn sync_with_config(
    &mut self, stored_paths: Option<Vec<PathBuf>>, config_paths: &[PathBuf], progress: bool
) -> IndexResult<(usize, usize, usize, usize)>;
```

**Purpose:** Synchronize indexed paths with settings.toml (source of truth)

**Flow:**
1. Compare stored paths with config paths (canonicalized)
2. Index new directories via `index_directory()`
3. Remove symbols from directories no longer in config
4. Track indexed paths

**Returns:** `(dirs_added, dirs_removed, files_indexed, symbols_found)`

**Evidence:** `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2135-2286`

### 3. Tantivy Integration (DocumentIndex)

**Location:** `/Users/bartolli/Projects/codanna/src/storage/tantivy.rs`

**Batch Operations:**
```rust
pub fn start_batch(&self) -> StorageResult<()>;
pub fn add_document(&self, ...) -> StorageResult<()>;
pub fn commit_batch(&self) -> StorageResult<()>;
```

**Batch Triggers:**
- `SimpleIndexer`: Every 100 files (`COMMIT_BATCH_SIZE`)
- `Pipeline`: Configurable via `batches_per_commit`

**Commit Processing:**
1. Commit writer to disk
2. Reload reader for new documents
3. Process pending vector embeddings
4. Build cluster cache for vector search

**Evidence:** `/Users/bartolli/Projects/codanna/src/storage/tantivy.rs:864-1040`

### 4. Relationship Resolution

**Method:** `resolve_cross_file_relationships()`
**Location:** `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2621-3156`

**Two-Pass Resolution:**
1. **Pass 1**: Process `Defines` relationships first (commits immediately)
2. **Pass 2**: Process other relationships (`Calls`, `Uses`, etc.)

**Resolution Process:**
- Build resolution context per file (imports, behaviors)
- Use symbol lookup cache to avoid duplicate Tantivy queries
- Restore incoming relationships after reindexing

### 5. Parallel Pipeline (Experimental)

**Location:** `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs`

**5-Stage Architecture:**
```
DISCOVER -> READ -> PARSE -> COLLECT -> INDEX
```

- **DISCOVER**: Parallel file walk (4 threads)
- **READ**: Multi-threaded file reading (configurable)
- **PARSE**: Parallel parsing with thread-local parser caches
- **COLLECT**: Single-threaded ID assignment and batching
- **INDEX**: Single-threaded Tantivy writes

**Returns:** `(IndexStats, Vec<UnresolvedRelationship>, SymbolLookupCache)`

**Evidence:** `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:104-245`

## Architecture Patterns

### Orchestration Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    SimpleIndexer                             │
├─────────────────────────────────────────────────────────────┤
│  sync_with_config                                            │
│    └─> index_directory                                       │
│          └─> index_directory_with_options                    │
│                ├─> FileWalker.walk()                         │
│                ├─> start_tantivy_batch()                     │
│                ├─> for each file:                            │
│                │     └─> index_file_internal()               │
│                │     └─> commit every 100 files              │
│                ├─> commit_tantivy_batch() (final)            │
│                └─> resolve_cross_file_relationships()        │
└─────────────────────────────────────────────────────────────┘
```

### Incremental Updates

Single file changes:
1. `index_file_with_force()` handles hash-based skip
2. Captures incoming relationships before deletion
3. Removes old documents, indexes new ones
4. Restores incoming relationships
5. Resolves cross-file relationships

### Batch Commit Strategy

- **Purpose**: Reduce I/O overhead, improve throughput
- **Frequency**: Every 100 files (configurable in pipeline)
- **Side Effects**: Processes pending embeddings, rebuilds symbol cache

## API Surface Dependencies

### Public APIs Used by CLI/MCP

| Method | Used By | Purpose |
|--------|---------|---------|
| `SimpleIndexer::with_settings_lazy()` | Main, MCP | Create lazy-initialized indexer |
| `index_directory()` | CLI index command | Full directory indexing |
| `index_file()` | File watcher | Incremental updates |
| `remove_file()` | File watcher | Handle deletions |
| `sync_with_config()` | CLI sync command | Settings-based sync |
| `FileWalker::walk()` | Both indexers | File discovery |
| `IndexStats` | All indexing ops | Progress/results |

### Internal APIs (Indexer-to-Storage)

| Method | Purpose |
|--------|---------|
| `DocumentIndex::start_batch()` | Begin transaction |
| `DocumentIndex::add_document()` | Add symbol to index |
| `DocumentIndex::commit_batch()` | Persist and reload |
| `DocumentIndex::remove_file_documents()` | Delete by path |
| `DocumentIndex::get_file_info()` | Hash check for skip |

## Conclusions

1. **Two Indexing Paths**: Production (`SimpleIndexer`) and experimental (`Pipeline`). Both share `FileWalker` and `DocumentIndex`.

2. **Batch Processing**: Critical for performance. Commits every 100 files to balance throughput and memory.

3. **Two-Pass Resolution**: Defines must be committed before Calls to enable instance method resolution.

4. **Relationship Preservation**: The system captures and restores incoming relationships during reindexing to maintain graph consistency.

5. **Key Dependencies**: Any changes to `FileWalker`, `DocumentIndex` batch methods, or `resolve_cross_file_relationships` affect the entire indexing flow.
