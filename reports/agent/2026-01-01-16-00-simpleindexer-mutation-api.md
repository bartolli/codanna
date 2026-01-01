# Research Report: SimpleIndexer Mutation/Write API

**Date**: 2026-01-01 16:00
**Agent**: Research-Agent-v5
**Model**: Opus 4.5
**File**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs` (5888 lines)

## Summary

SimpleIndexer is a 5888-line monolithic indexer that manages file indexing, symbol storage, relationship resolution, semantic search, and caching. All mutation operations coordinate multiple internal state components: DocumentIndex (Tantivy), symbol_cache, semantic_search, indexed_paths, and various in-memory resolvers. The API is designed for sequential single-threaded operation with batch commits.

## Internal State Components

The SimpleIndexer struct manages these mutable fields:

| Field | Type | Purpose |
|-------|------|---------|
| `document_index` | `DocumentIndex` | Tantivy storage (symbols, files, relationships) |
| `symbol_cache` | `Option<Arc<ConcurrentSymbolCache>>` | O(1) symbol lookups by name |
| `semantic_search` | `Option<Arc<Mutex<SimpleSemanticSearch>>>` | Embedding-based doc search |
| `indexed_paths` | `HashSet<PathBuf>` | Tracked indexed directories |
| `unresolved_relationships` | `Vec<UnresolvedRelationship>` | Pending cross-file refs |
| `method_call_resolvers` | `HashMap<FileId, MethodCallResolver>` | Variable types per file |
| `trait_symbols_by_file` | `HashMap<FileId, HashMap<String, SymbolKind>>` | Trait tracking |
| `file_languages` | `HashMap<FileId, LanguageId>` | Language per file |
| `file_behaviors` | `HashMap<FileId, Box<dyn LanguageBehavior>>` | Import state |
| `pending_embeddings` | `Vec<(SymbolId, String)>` | Symbols awaiting vector processing |
| `pending_incoming_relationships` | `Option<(String, Vec<CapturedIncomingRelationship>)>` | Preserved refs during reindex |

## Key Findings

### 1. File Indexing Methods

#### `index_file` (Line 418)
```rust
pub fn index_file(&mut self, path: impl AsRef<Path>) -> IndexResult<crate::IndexingResult>
```
**Modifies**: document_index, symbol_cache, semantic_search, unresolved_relationships, method_call_resolvers, trait_symbols_by_file, file_languages, file_behaviors, pending_embeddings
**Flow**: start_tantivy_batch -> index_file_internal -> commit_tantivy_batch -> resolve_cross_file_relationships
**Pipeline Replacement**: Yes - Pipeline Phase 1 handles parsing/collection, but resolution still needed

#### `index_file_with_force` (Lines 421-438)
```rust
pub fn index_file_with_force(&mut self, path: impl AsRef<Path>, force: bool) -> IndexResult<crate::IndexingResult>
```
**Modifies**: Same as index_file
**Flow**: Same as index_file with force flag to skip hash check
**Pipeline Replacement**: Yes - force flag maps to Pipeline clearing existing data

#### `index_file_no_resolve` (Lines 441-455)
```rust
pub fn index_file_no_resolve(&mut self, path: impl AsRef<Path>) -> IndexResult<crate::IndexingResult>
```
**Modifies**: document_index, symbol_cache, semantic_search (but NOT unresolved_relationships resolution)
**Flow**: Batched writes without relationship resolution
**Pipeline Replacement**: Yes - designed for batch operations, aligns with Pipeline design

#### `index_file_internal` (Lines 457-569)
```rust
fn index_file_internal(&mut self, path: impl AsRef<Path>, force: bool) -> IndexResult<crate::IndexingResult>
```
**Private method** - core logic shared by public methods
**Flow**:
1. Normalize path relative to workspace_root
2. Read file content and hash
3. Check if file exists (skip if unchanged unless force)
4. Capture incoming relationships before deletion (for reindex)
5. Remove old symbols and embeddings
6. Register new file
7. Call reindex_file_content

---

### 2. File Removal

#### `remove_file` (Lines 571-647)
```rust
pub fn remove_file(&mut self, path: impl AsRef<Path>) -> IndexResult<()>
```
**Modifies**: document_index, semantic_search, symbol_cache
**Flow**:
1. Get file_id from path
2. Collect symbols for cleanup
3. Delete imports for file (`delete_imports_for_file`)
4. Remove file documents from Tantivy
5. Remove embeddings from semantic search
6. Commit batch
7. Rebuild symbol cache

**Pipeline Replacement**: Partial - file removal could be a Pipeline stage, but requires coordination with semantic search cleanup

---

### 3. Directory Indexing Methods

#### `index_directory` (Lines 2449-2455)
```rust
pub fn index_directory(&mut self, dir: impl AsRef<Path>, progress: bool, dry_run: bool) -> IndexResult<IndexStats>
```
**Delegates to**: `index_directory_with_options`

#### `index_directory_with_force` (Lines 2458-2466)
```rust
pub fn index_directory_with_force(&mut self, dir: impl AsRef<Path>, progress: bool, dry_run: bool, force: bool) -> IndexResult<IndexStats>
```
**Delegates to**: `index_directory_with_options`

#### `index_directory_with_options` (Lines 2469-2572)
```rust
pub fn index_directory_with_options(
    &mut self, dir: impl AsRef<Path>, progress: bool, dry_run: bool, force: bool, max_files: Option<usize>
) -> IndexResult<IndexStats>
```
**Modifies**: All state components
**Flow**:
1. Walk directory with FileWalker
2. Start Tantivy batch
3. Process files in chunks of 100 (`COMMIT_BATCH_SIZE`)
4. Commit periodically to avoid memory pressure
5. Resolve cross-file relationships at end
6. Build symbol cache

**Pipeline Replacement**: **Primary target** - Pipeline::index_directory handles Steps 1-3 in parallel. Steps 4-5 need Pipeline Phase 2.

---

### 4. Sync Operations

#### `sync_with_config` (Lines 2149-2265)
```rust
pub fn sync_with_config(
    &mut self, stored_paths: Option<Vec<PathBuf>>, config_paths: &[PathBuf], progress: bool
) -> IndexResult<(usize, usize, usize, usize)>
```
**Modifies**: document_index, indexed_paths, symbol_cache, semantic_search
**Flow**:
1. Compare stored paths vs config paths
2. Index new directories
3. Remove files from removed directories
4. Update indexed_paths tracking
**Returns**: (added_count, removed_count, files_indexed, symbols_found)

**Pipeline Replacement**: Partial - could use Pipeline for indexing new directories, removal logic stays

---

### 5. Semantic Search Management

#### `enable_semantic_search` (Line 242-257)
```rust
pub fn enable_semantic_search(&mut self) -> IndexResult<()>
```
**Modifies**: semantic_search
**Action**: Initializes SimpleSemanticSearch from settings.semantic_search.model

#### `save_semantic_search` (Lines 288-308)
```rust
pub fn save_semantic_search(&self, path: &Path) -> Result<(), SemanticSearchError>
```
**Modifies**: Nothing (read-only save)
**Action**: Persists embeddings to disk

#### `load_semantic_search` (Lines 313-348)
```rust
pub fn load_semantic_search(&mut self, path: &Path) -> IndexResult<bool>
```
**Modifies**: semantic_search
**Action**: Loads embeddings from disk, returns true if successful

---

### 6. Cache Management

#### `build_symbol_cache` (Lines 3339-3378)
```rust
pub fn build_symbol_cache(&mut self) -> IndexResult<()>
```
**Modifies**: symbol_cache
**Flow**:
1. Clear existing cache (release mmap)
2. Get all symbols from index
3. Build SymbolHashCache file
4. Load cache for immediate use
**Dependencies**: Requires committed Tantivy data

**Pipeline Replacement**: Yes - should be called after Pipeline indexing completes

#### `clear_symbol_cache` (Lines 3381-3397)
```rust
pub fn clear_symbol_cache(&mut self, delete_file: bool) -> IndexResult<()>
```
**Modifies**: symbol_cache
**Action**: Drops cache reference, optionally deletes file

#### `load_symbol_cache` (Lines 3400-3426)
```rust
pub fn load_symbol_cache(&mut self) -> IndexResult<()>
```
**Modifies**: symbol_cache
**Action**: Opens existing cache file if present

---

### 7. Transaction Support

#### `begin_transaction` (Lines 397-400)
```rust
pub fn begin_transaction(&self) -> IndexTransaction
```
**Modifies**: Nothing (returns dummy transaction)
**Note**: Compatibility method - Tantivy handles transactions internally

#### `commit_transaction` (Lines 403-407)
```rust
pub fn commit_transaction(&mut self, mut transaction: IndexTransaction) -> IndexResult<()>
```
**Modifies**: Delegates to commit_tantivy_batch
**Action**: Marks transaction complete, commits Tantivy batch

#### `rollback_transaction` (Lines 410-413)
```rust
pub fn rollback_transaction(&mut self, _transaction: IndexTransaction)
```
**Modifies**: Nothing
**Note**: No-op - Tantivy automatically discards uncommitted changes

---

### 8. Tantivy Batch Operations

#### `start_tantivy_batch` (Lines 351-358)
```rust
pub fn start_tantivy_batch(&self) -> IndexResult<()>
```
**Modifies**: document_index (internal batch state)
**Action**: Signals start of batch writes

#### `commit_tantivy_batch` (Lines 361-391)
```rust
pub fn commit_tantivy_batch(&mut self) -> IndexResult<()>
```
**Modifies**: document_index, pending_embeddings, symbol_cache
**Flow**:
1. Commit Tantivy batch
2. Process pending vector embeddings
3. Build/update symbol cache
**Critical**: This is the sync point for all pending writes

#### `clear_tantivy_index` (Lines 2405-2421)
```rust
pub fn clear_tantivy_index(&mut self) -> IndexResult<()>
```
**Modifies**: document_index, trait_symbols_by_file, method_call_resolvers, semantic_search
**Action**: Clears all indexed data, resets resolvers

---

## Architecture/Patterns Identified

### Batch Processing Pattern
```
start_tantivy_batch()
  -> index_file_internal() x N
  -> commit_tantivy_batch() [every 100 files]
  -> resolve_cross_file_relationships()
```

### State Synchronization Points
1. `commit_tantivy_batch()` - Persists symbols/files/relationships
2. `resolve_cross_file_relationships()` - Two-pass resolution (Defines first, then Calls)
3. `build_symbol_cache()` - Rebuilds O(1) lookup cache

### Two-Pass Relationship Resolution (Lines 2627-3076)
1. **Pass 1**: Process Defines relationships, commit
2. **Pass 2**: Process Calls/other relationships (can query Defines)
This ordering ensures instance methods resolve correctly.

## Pipeline Replacement Analysis

| Method | Pipeline Replacement | Notes |
|--------|---------------------|-------|
| `index_file` | Partial | Single-file not ideal for Pipeline |
| `index_file_with_force` | Partial | Same |
| `index_file_no_resolve` | Yes | Designed for batching |
| `index_directory_*` | **Yes** | Primary Pipeline target |
| `remove_file` | Partial | Needs semantic cleanup coordination |
| `sync_with_config` | Partial | Uses index_directory internally |
| `enable_semantic_search` | No | Initialization, not indexing |
| `save/load_semantic_search` | No | Persistence, not indexing |
| `build_symbol_cache` | Post-Pipeline | Call after Pipeline completes |
| `clear_symbol_cache` | No | Management operation |
| `*_transaction` | Deprecated | Tantivy handles internally |
| `start/commit_tantivy_batch` | Absorbed | Pipeline manages batching |
| `clear_tantivy_index` | No | Management operation |

## Conclusions

1. **Primary Pipeline Target**: `index_directory_with_options` is the main entry point that Pipeline should replace. It processes files sequentially with periodic commits.

2. **State Coupling**: SimpleIndexer tightly couples parsing, storage, caching, and semantic search. Pipeline must coordinate these or provide hooks.

3. **Resolution Complexity**: Cross-file relationship resolution (Lines 2627-3076) uses a two-pass algorithm. Pipeline Phase 2 needs to replicate this.

4. **Cache Rebuild**: `build_symbol_cache()` must be called after Pipeline completes to maintain O(1) lookups.

5. **Semantic Search Integration**: If semantic search is enabled, symbols need embedding during indexing. Pipeline should provide a hook for this.

6. **Transaction Compatibility**: The transaction API is vestigial. Pipeline can ignore it and use direct batch operations.
