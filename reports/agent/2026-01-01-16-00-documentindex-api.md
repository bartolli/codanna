# Research Report: DocumentIndex API

**Date**: 2026-01-01 16:00
**Agent**: Research-Agent
**Model**: Opus 4.5

## Summary

DocumentIndex is a Tantivy-based storage layer for code intelligence data. Located at `src/storage/tantivy.rs:393-417`, it provides 59 methods across 7 categories: constructor/configuration, batch operations, symbol queries, relationship queries, file operations, import storage, and statistics. The struct uses document types (`doc_type` field) to distinguish symbols, relationships, file_info, imports, and metadata within a single Tantivy index.

## Key Findings

### 1. Struct Definition

**Location**: `src/storage/tantivy.rs:393-417`

```rust
pub struct DocumentIndex {
    index: Index,
    reader: IndexReader,
    schema: IndexSchema,
    index_path: PathBuf,
    pub(crate) writer: Mutex<Option<IndexWriter<Document>>>,
    heap_size: usize,
    max_retry_attempts: u32,
    vector_storage_path: Option<PathBuf>,
    vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
    cluster_cache: Arc<RwLock<Option<ClusterCache>>>,
    embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
    pub(crate) pending_embeddings: Mutex<Vec<(SymbolId, String)>>,
    pending_symbol_counter: Mutex<Option<u32>>,
    pending_file_counter: Mutex<Option<u32>>,
}
```

## API Reference by Category

### Constructor and Configuration Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `new` | 444-499 | `pub fn new(index_path: impl AsRef<Path>, settings: &Settings) -> StorageResult<Self>` | `Index::open_in_dir` or `Index::create`, registers ngram tokenizer, creates `IndexReader` |
| `with_vector_support` | 543-550 | `pub fn with_vector_support(mut self, engine: Arc<Mutex<VectorSearchEngine>>, path: impl AsRef<Path>) -> Self` | None (builder pattern) |
| `with_embedding_generator` | 553-556 | `pub fn with_embedding_generator(mut self, generator: Arc<dyn EmbeddingGenerator>) -> Self` | None (builder pattern) |
| `has_vector_support` | 559-561 | `pub fn has_vector_support(&self) -> bool` | None |
| `vector_storage_path` | 564-566 | `pub fn vector_storage_path(&self) -> Option<&Path>` | None |
| `vector_engine` | 569-571 | `pub fn vector_engine(&self) -> Option<&Arc<Mutex<VectorSearchEngine>>>` | None |
| `path` | 2191-2193 | `pub fn path(&self) -> &Path` | None |

### Batch Operation Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `start_batch` | 867-886 | `pub fn start_batch(&self) -> StorageResult<()>` | `index.writer()` with retry logic |
| `add_document` | 889-975 | `pub fn add_document(&self, symbol_id, name, kind, file_id, file_path, line, column, end_line, end_column, doc_comment, signature, module_path, context, visibility, scope_context, language_id) -> StorageResult<()>` | `writer.add_document()` |
| `commit_batch` | 978-1035 | `pub fn commit_batch(&self) -> StorageResult<()>` | `writer.commit()`, `reader.reload()` |
| `remove_file_documents` | 1038-1058 | `pub fn remove_file_documents(&self, file_path: &str) -> StorageResult<()>` | `writer.delete_term()` on file_path |
| `clear` | 1286-1299 | `pub fn clear(&self) -> StorageResult<()>` | `writer.delete_all_documents()`, `writer.commit()`, `reader.reload()` |

### Symbol Query Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `search` | 1065-1277 | `pub fn search(&self, query_str, limit, kind_filter, module_filter, language_filter) -> StorageResult<Vec<SearchResult>>` | `QueryParser::parse_query`, `BooleanQuery` with fuzzy matching, `searcher.search(&query, &TopDocs)` |
| `find_symbol_by_id` | 1302-1317 | `pub fn find_symbol_by_id(&self, id: SymbolId) -> StorageResult<Option<Symbol>>` | `TermQuery` on symbol_id, `searcher.search(&query, &TopDocs::with_limit(1))` |
| `find_symbol_by_id_with_language` | 1320-1351 | `pub fn find_symbol_by_id_with_language(&self, id: SymbolId, language: &str) -> StorageResult<Option<Symbol>>` | `BooleanQuery` (symbol_id AND language), `searcher.search()` |
| `find_symbols_by_name` | 1354-1398 | `pub fn find_symbols_by_name(&self, name: &str, language_filter: Option<&str>) -> StorageResult<Vec<Symbol>>` | `TermQuery` on name field (exact match), `BooleanQuery` with doc_type filter |
| `find_symbol_by_name_and_range` | 1401-1449 | `pub fn find_symbol_by_name_and_range(&self, name, file_id, range) -> StorageResult<Option<Symbol>>` | `BooleanQuery` (name AND file_id AND line_number), verify end_line in post-processing |
| `find_symbols_by_file` | 1452-1475 | `pub fn find_symbols_by_file(&self, file_id: FileId) -> StorageResult<Vec<Symbol>>` | `BooleanQuery` (doc_type=symbol AND file_id), limit 1000 |
| `find_symbols_by_module` | 1478-1530 | `pub fn find_symbols_by_module(&self, module_path: &str) -> StorageResult<Vec<Symbol>>` | `BooleanQuery` (doc_type=symbol AND module_path), limit 1000 |
| `get_all_symbols` | 1533-1558 | `pub fn get_all_symbols(&self, limit: usize) -> StorageResult<Vec<Symbol>>` | `TermQuery` (doc_type=symbol), `TopDocs::with_limit(limit)` |
| `document_to_symbol` | 1561-1705 | `fn document_to_symbol(&self, doc: &Document) -> StorageResult<Symbol>` | None (field extraction only) |

### Relationship Query Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `get_relationships_from` | 1957-2016 | `pub fn get_relationships_from(&self, from_id: SymbolId, kind: RelationKind) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>>` | `BooleanQuery` (doc_type=relationship AND from_symbol_id AND relation_kind), limit 1000 |
| `get_relationships_to` | 2019-2081 | `pub fn get_relationships_to(&self, to_id: SymbolId, kind: RelationKind) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>>` | `BooleanQuery` (doc_type=relationship AND to_symbol_id AND relation_kind), limit 1000 |
| `get_all_relationships_by_kind` | 2084-2147 | `pub fn get_all_relationships_by_kind(&self, kind: RelationKind) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>>` | `BooleanQuery` (doc_type=relationship AND relation_kind), limit 10000 |
| `store_relationship` | 2196-2228 | `pub(crate) fn store_relationship(&self, from: SymbolId, to: SymbolId, rel: &Relationship) -> StorageResult<()>` | `writer.add_document()` with relationship fields |
| `delete_relationships_for_symbol` | 1868-1884 | `pub fn delete_relationships_for_symbol(&self, id: SymbolId) -> StorageResult<()>` | `writer.delete_term()` on from_symbol_id, then on to_symbol_id |
| `query_relationships` | 2521-2608 | `pub(crate) fn query_relationships(&self) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>>` | `TermQuery` (doc_type=relationship), limit 1,000,000 |

### File Operation Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `get_file_info` | 1758-1792 | `pub fn get_file_info(&self, path: &str) -> StorageResult<Option<(FileId, String)>>` | `BooleanQuery` (doc_type=file_info AND file_path) |
| `get_file_path` | 2150-2176 | `pub fn get_file_path(&self, file_id: FileId) -> StorageResult<Option<String>>` | `BooleanQuery` (doc_type=file_info AND file_id) |
| `get_next_file_id` | 1795-1807 | `pub fn get_next_file_id(&self) -> StorageResult<u32>` | Reads pending counter or `query_metadata(FileCounter)` |
| `get_all_indexed_paths` | 1926-1954 | `pub fn get_all_indexed_paths(&self) -> StorageResult<Vec<PathBuf>>` | `TermQuery` (doc_type=file_info), limit 100,000 |
| `store_file_info` | 2271-2293 | `pub(crate) fn store_file_info(&self, file_id, path, hash, timestamp) -> StorageResult<()>` | `writer.add_document()` |
| `store_file_registration` | 2296-2328 | `pub fn store_file_registration(&self, registration: &FileRegistration) -> StorageResult<()>` | `writer.add_document()` (pipeline API) |
| `query_file_info` | 2611-2661 | `pub(crate) fn query_file_info(&self) -> StorageResult<Vec<(FileId, String, String, u64)>>` | `TermQuery` (doc_type=file_info), limit 100,000 |

### Import Storage Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `store_import` | 2348-2382 | `pub fn store_import(&self, import: &Import) -> StorageResult<()>` | `writer.add_document()` with import fields |
| `get_imports_for_file` | 2385-2442 | `pub fn get_imports_for_file(&self, file_id: FileId) -> StorageResult<Vec<Import>>` | `BooleanQuery` (doc_type=import AND import_file_id), limit 1000 |
| `delete_imports_for_file` | 2445-2469 | `pub fn delete_imports_for_file(&self, file_id: FileId) -> StorageResult<()>` | `writer.delete_query()` with compound query |

### ID Generation and Metadata Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `get_next_symbol_id` | 1810-1821 | `pub fn get_next_symbol_id(&self) -> StorageResult<u32>` | Reads pending counter or `query_metadata(SymbolCounter)` |
| `update_pending_symbol_counter` | 1824-1831 | `pub fn update_pending_symbol_counter(&self, new_value: u32) -> StorageResult<()>` | None (mutex update only) |
| `store_metadata` | 2472-2492 | `pub(crate) fn store_metadata(&self, key: MetadataKey, value: u64) -> StorageResult<()>` | `writer.delete_term()` on meta_key, then `writer.add_document()` |
| `query_metadata` | 2699-2720 | `pub(crate) fn query_metadata(&self, key: MetadataKey) -> StorageResult<Option<u64>>` | `BooleanQuery` (doc_type=metadata AND meta_key) |

### Statistics Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `document_count` | 1280-1283 | `pub fn document_count(&self) -> StorageResult<u64>` | `searcher.num_docs()` |
| `count_symbols` | 1887-1896 | `pub fn count_symbols(&self) -> StorageResult<usize>` | `TermQuery` (doc_type=symbol), `Count` collector |
| `count_relationships` | 1899-1908 | `pub fn count_relationships(&self) -> StorageResult<usize>` | `TermQuery` (doc_type=relationship), `Count` collector |
| `count_files` | 1911-1920 | `pub fn count_files(&self) -> StorageResult<usize>` | `TermQuery` (doc_type=file_info), `Count` collector |
| `count_symbol_documents` | 2664-2673 | `pub(crate) fn count_symbol_documents(&self) -> StorageResult<u64>` | Same as count_symbols |

### Delete Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `delete_symbol` | 1834-1848 | `pub fn delete_symbol(&self, id: SymbolId) -> StorageResult<()>` | `writer.delete_term()` on symbol_id |
| `delete_relationships_for_symbol` | 1851-1884 | `pub fn delete_relationships_for_symbol(&self, id: SymbolId) -> StorageResult<()>` | `writer.delete_term()` on from_symbol_id, then to_symbol_id |
| `delete_imports_for_file` | 2445-2469 | `pub fn delete_imports_for_file(&self, file_id: FileId) -> StorageResult<()>` | `writer.delete_query()` |

### Vector/Cluster Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `build_cluster_cache` | 642-698 | `fn build_cluster_cache(&self) -> StorageResult<()>` | Iterates segment readers, reads `cluster_id` and `has_vector` fast fields |
| `get_cluster_documents` | 705-721 | `pub fn get_cluster_documents(&self, segment_ord, cluster_id) -> StorageResult<Vec<DocId>>` | Reads from cluster_cache (no Tantivy query) |
| `get_all_cluster_ids` | 724-733 | `pub fn get_all_cluster_ids(&self) -> StorageResult<Vec<ClusterId>>` | Reads from cluster_cache (no Tantivy query) |
| `warm_cluster_cache` | 736-752 | `pub fn warm_cluster_cache(&self) -> StorageResult<()>` | Clears cache and calls `build_cluster_cache()` |
| `get_cache_generation` | 755-761 | `pub fn get_cache_generation(&self) -> StorageResult<Option<u64>>` | None (reads cache) |
| `reload_and_warm` | 764-774 | `pub fn reload_and_warm(&self) -> StorageResult<()>` | `reader.reload()`, then `warm_cluster_cache()` |
| `update_cluster_assignments` | 777-861 | `pub fn update_cluster_assignments(&self) -> StorageResult<()>` | Searches for symbols by id, deletes and re-adds with updated cluster_id |
| `post_commit_vector_processing` | 574-639 | `fn post_commit_vector_processing(&self) -> StorageResult<()>` | None (vector engine operations only) |

### Utility Methods

| Method | Line | Signature | Tantivy Operations |
|--------|------|-----------|-------------------|
| `index_symbol` | 2231-2268 | `pub fn index_symbol(&self, symbol: &Symbol, file_path: &str) -> StorageResult<()>` | Calls `add_document()` |
| `create_writer_with_retry` | 501-540 | `fn create_writer_with_retry(&self) -> Result<IndexWriter<Document>, TantivyError>` | `index.writer()` with exponential backoff |
| `rebuild_index_data` | 2676-2696 | `pub(crate) fn rebuild_index_data(&self) -> StorageResult<()>` | DEPRECATED - returns Ok(()) |

## Architecture Patterns

### Document Type Discrimination

All documents share the same Tantivy index but are distinguished by the `doc_type` field:
- `"symbol"` - Code symbols (functions, structs, methods, etc.)
- `"relationship"` - Edges between symbols (Calls, Implements, etc.)
- `"file_info"` - File metadata (path, hash, timestamp)
- `"import"` - Import statements
- `"metadata"` - Counters and configuration values

### Batch Write Pattern

1. `start_batch()` - Creates writer, initializes pending counters
2. Multiple `add_document()` / `store_*()` calls
3. `commit_batch()` - Commits writer, reloads reader, processes vectors

### Query Pattern

Most queries use `BooleanQuery` with:
1. `doc_type` filter (Occur::Must)
2. Field-specific filter (Occur::Must)
3. Optional additional filters

### ID Generation During Batch

During batch operations, `get_next_symbol_id()` and `get_next_file_id()` use pending counters instead of querying committed metadata, ensuring unique IDs across concurrent file processing.

## Delegation Recommendations for SimpleIndexer

### Direct Delegation (pass-through to DocumentIndex)

These methods can be directly delegated without facade-level handling:
- All query methods (`find_symbol_*`, `get_*`, `search`)
- All statistics methods (`count_*`, `document_count`)
- Batch control (`start_batch`, `commit_batch`)
- Path accessor (`path`)

### Facade-Level Handling Required

These require additional logic in SimpleIndexer:
- `store_relationship` - SimpleIndexer may need to resolve symbol references
- `add_document` / `index_symbol` - May need coordinate with symbol resolution
- Vector operations - May need orchestration with embedding pipeline
- File operations - May need coordination with change detection

## Conclusions

DocumentIndex provides a complete storage API for code intelligence. The API is well-organized into logical categories with consistent patterns. For SimpleIndexer facade:

1. **Query methods**: Direct delegation works for all read operations
2. **Write methods**: Most can delegate, but relationship storage may need resolution
3. **Batch operations**: SimpleIndexer should control batch lifecycle
4. **Vector operations**: Require careful coordination with embedding pipeline
5. **Statistics**: All can delegate directly

The pending counter pattern (`pending_symbol_counter`, `pending_file_counter`) is critical for parallel indexing - SimpleIndexer should ensure batch boundaries are respected when delegating write operations.
