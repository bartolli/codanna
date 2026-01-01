# Research Report: SimpleIndexer Internal State Management

**Date**: 2026-01-01 16:00
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

SimpleIndexer maintains a complex internal state that splits responsibilities between a persistent Tantivy-based DocumentIndex and in-memory transient state for parsing sessions. The struct contains 15 fields spanning document storage, semantic search, symbol caching, and resolution tracking. Understanding this state is critical for any IndexFacade implementation.

## Key Findings

### 1. Struct Definition (All Fields)

**Location**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:66-98`

```rust
pub struct SimpleIndexer {
    // Core infrastructure
    parser_factory: ParserFactory,
    settings: Arc<Settings>,
    document_index: DocumentIndex,

    // Caching layer
    symbol_cache: Option<Arc<ConcurrentSymbolCache>>,

    // Resolution state (transient, rebuilt per session)
    unresolved_relationships: Vec<UnresolvedRelationship>,
    method_call_resolvers: HashMap<FileId, MethodCallResolver>,
    trait_symbols_by_file: HashMap<FileId, HashMap<String, SymbolKind>>,
    file_languages: HashMap<FileId, LanguageId>,
    file_behaviors: HashMap<FileId, Box<dyn LanguageBehavior>>,
    pending_incoming_relationships: Option<(String, Vec<CapturedIncomingRelationship>)>,

    // Vector/Semantic search
    vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
    embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
    pending_embeddings: Vec<(SymbolId, String)>,
    semantic_search: Option<Arc<Mutex<SimpleSemanticSearch>>>,

    // Directory tracking
    indexed_paths: HashSet<PathBuf>,
}
```

### 2. indexed_paths Management

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:95, 2100-2280`

**Purpose**: Tracks which directories have been indexed (canonicalized paths).

**Access patterns**:
- `add_indexed_path(dir_path: &Path)` - Canonicalizes path, deduplicates against parent directories
- `get_indexed_paths()` - Returns reference to the HashSet
- `sync_with_config()` - Compares stored paths against config, indexes new directories, removes old ones

**State behavior**:
- Initialized as empty HashSet in constructors (lines 145, 186)
- Paths are deduplicated: child paths are removed when parent is added
- Used in sync_with_config to determine which directories to add/remove
- Removed paths have their files deleted from the index

**Facade implications**: This is transient state that could be tracked at facade level OR derived from DocumentIndex.get_all_indexed_paths() which queries Tantivy for all file paths and extracts unique directories.

### 3. semantic_search Initialization and Access

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:89, 244-340`

**Field type**: `Option<Arc<Mutex<SimpleSemanticSearch>>>`

**Initialization**:
- Starts as `None` in both constructors
- `enable_semantic_search()` - Creates from model name in settings
- `load_semantic_search(path)` - Loads persisted data from disk

**Access patterns**:
- `has_semantic_search()` - Check if enabled
- `semantic_search_embedding_count()` - Get count of embeddings
- `get_semantic_metadata()` - Get metadata struct
- `save_semantic_search(path)` - Persist to disk
- `semantic_search_docs()` / `semantic_search_docs_with_language()` - Search queries

**Mutation during indexing**:
- Embeddings added during `reindex_file_content()` via the mutex lock
- Embeddings removed during file removal/reindex via `remove_embeddings()`
- Save called after embedding removal to maintain cache consistency

**Facade implications**: This is independent state that should be managed at facade level. The semantic search is completely separate from DocumentIndex.

### 4. document_index Query/Mutation Flows

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs` (multiple locations)

**DocumentIndex struct** (`/Users/bartolli/Projects/codanna/src/storage/tantivy.rs:393-417`):
```rust
pub struct DocumentIndex {
    index: Index,
    reader: IndexReader,
    schema: IndexSchema,
    index_path: PathBuf,
    writer: Mutex<Option<IndexWriter<Document>>>,
    heap_size: usize,
    max_retry_attempts: u32,
    vector_storage_path: Option<PathBuf>,
    vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
    cluster_cache: Arc<RwLock<Option<ClusterCache>>>,
    embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
    pending_embeddings: Mutex<Vec<(SymbolId, String)>>,
    pending_symbol_counter: Mutex<Option<u32>>,
    pending_file_counter: Mutex<Option<u32>>,
}
```

**Query methods** (read operations):
- `find_symbol_by_id(id)` - Get symbol by ID
- `find_symbols_by_name(name, language_filter)` - Find by name
- `find_symbols_by_file(file_id)` - Get all symbols in file
- `get_file_info(path)` - Get FileId and hash for path
- `get_file_path(file_id)` - Get path for FileId
- `get_all_indexed_paths()` - Get all indexed file paths
- `count_symbols()`, `count_files()`, `count_relationships()` - Statistics
- `get_relationships_from(id, kind)`, `get_relationships_to(id, kind)` - Relationship queries

**Mutation methods** (write operations):
- `start_batch()` - Begin write transaction
- `commit_batch()` - Commit pending changes
- `store_file_info()` - Register file
- `index_symbol()` - Add symbol document
- `store_relationship()` - Add relationship document
- `store_import()` - Add import document
- `remove_file_documents(path)` - Delete all documents for a file
- `delete_imports_for_file(file_id)` - Remove import documents
- `clear()` - Clear entire index

**Facade implications**: All persistent data flows through DocumentIndex. The facade must delegate these operations.

### 5. symbol_cache Build and Usage

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:74, 1644-1700, 3315-3410`

**Field type**: `Option<Arc<ConcurrentSymbolCache>>`

**Build trigger**:
- Built after `commit_tantivy_batch()` (line 382)
- Built after file removal (line 651)
- Called via `build_symbol_cache()` method

**Build process**:
1. Clear existing cache to release memory-mapped views
2. Fetch all symbols from DocumentIndex.get_all_symbols()
3. Build hash-based cache file on disk
4. Load cache for immediate use

**Usage (O(1) lookup)**:
```rust
pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
    if let Some(ref cache) = self.symbol_cache {
        if let Some(id) = cache.lookup_by_name(name) {
            return Some(id);  // Fast path
        }
    }
    // Fallback to Tantivy query
    self.document_index.find_symbols_by_name(name, None)...
}
```

**Storage location**: `{index_base}/symbol_cache.bin`

**Facade implications**: Cache is derived state that can be rebuilt from DocumentIndex. The facade could manage this independently or delegate to SimpleIndexer.

### 6. settings Impact on Behavior

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:114-246`

**Uses of settings**:
- `workspace_root` - Base path for relative paths and module path calculation
- `index_path` - Location for Tantivy data and caches
- `semantic_search.model` - Model name for semantic search initialization
- Passed to `ParserFactory::new(settings.clone())`
- Passed to `DocumentIndex::new(tantivy_path, &settings)`
- Used to construct `FileWalker::new(self.settings.clone())`

**Facade implications**: Settings must be available at construction and affects path resolution throughout. Should be passed to facade.

### 7. State Synchronization Patterns

**Transient resolution state** (rebuilt per indexing session):
- `unresolved_relationships` - Cleared via `std::mem::take()` in `resolve_cross_file_relationships()`
- `method_call_resolvers` - Populated during parsing, used during resolution
- `trait_symbols_by_file` - Populated during parsing, used during relationship extraction
- `file_languages` - Maps FileId to LanguageId for enriching Symbol results
- `file_behaviors` - Stores language behavior instances with state
- `pending_incoming_relationships` - Temporary storage during file reindex

**Persistent state** (survives sessions):
- `document_index` - Tantivy data (symbols, relationships, files, imports)
- `symbol_cache` - Derived from DocumentIndex, persisted to disk
- `semantic_search` - Separate persistence to `{index_path}/semantic/`
- `indexed_paths` - Currently transient, but could be persisted

**Synchronization flow during indexing**:
1. `index_file_internal()` - Register file, extract symbols, store to DocumentIndex
2. `commit_tantivy_batch()` - Commit Tantivy, process pending embeddings, rebuild symbol cache
3. `resolve_cross_file_relationships()` - Process unresolved relationships, update Tantivy

## Architecture Summary

### State That Lives in SimpleIndexer

| Field | Persistent | Purpose |
|-------|------------|---------|
| `parser_factory` | No | Creates language-specific parsers |
| `settings` | Config | Shared configuration |
| `document_index` | Yes (Tantivy) | All persistent data |
| `symbol_cache` | Yes (file) | O(1) symbol lookup |
| `unresolved_relationships` | No | Batch relationship resolution |
| `method_call_resolvers` | No | Per-file method resolution |
| `trait_symbols_by_file` | No | Trait tracking during parsing |
| `file_languages` | No | Language enrichment |
| `file_behaviors` | No | Stateful parsing behaviors |
| `pending_incoming_relationships` | No | Reindex relationship preservation |
| `vector_engine` | Yes (file) | Vector search storage |
| `embedding_generator` | No | Generates embeddings |
| `pending_embeddings` | No | Batch embedding processing |
| `semantic_search` | Yes (file) | Semantic search data |
| `indexed_paths` | No* | Directory tracking |

### State That Must Be Preserved in IndexFacade

**Must delegate to SimpleIndexer/DocumentIndex**:
- All Tantivy operations (symbols, relationships, files, imports)
- Symbol cache management (or derive independently)
- Semantic search operations

**Can be managed at facade level**:
- `indexed_paths` - Could be tracked independently
- Settings reference
- Parser factory

**Transient state (not preserved)**:
- `unresolved_relationships` - Created and consumed within indexing
- `method_call_resolvers` - Per-session state
- `trait_symbols_by_file` - Per-session state
- `file_languages` / `file_behaviors` - Per-session state
- `pending_*` fields - Batch processing buffers

### What Can Be Delegated vs Facade-Level Tracking

**Delegate to SimpleIndexer**:
- All indexing operations (`index_file`, `index_directory`)
- All query operations (symbol lookup, relationships)
- Semantic search operations
- Symbol cache operations

**Facade-level tracking candidates**:
- `indexed_paths` - Simple set that facade could maintain
- Statistics aggregation across multiple indexes
- Cross-index search coordination

## Conclusions

1. **DocumentIndex is the single source of truth** for persistent data. All symbol, relationship, and file data flows through it.

2. **Transient state is session-scoped** and is used during indexing/resolution phases. It does not need to be preserved between indexing sessions.

3. **Semantic search is independent** - It has its own persistence path and can be managed separately from DocumentIndex.

4. **Symbol cache is derived state** - It can be rebuilt from DocumentIndex at any time, making it safe to invalidate.

5. **indexed_paths is the main candidate for facade-level tracking** - It is currently transient but represents logical state about what directories are indexed.

6. **Settings must be passed through** - Many operations depend on settings for path resolution and configuration.

For an IndexFacade implementation, the recommended approach is to wrap SimpleIndexer and delegate most operations, while potentially tracking indexed_paths and providing a unified interface for multi-index scenarios.
