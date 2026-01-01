# Research Report: Embedding Sync Lifecycle in SimpleIndexer

**Date**: 2026-01-01 12:00
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The embedding sync lifecycle in SimpleIndexer involves two parallel embedding systems: `SimpleSemanticSearch` (doc comment embeddings) and `VectorSearchEngine` (symbol embeddings via pipeline). Embeddings are created during `store_symbol()` for doc comments and during `commit_tantivy_batch()` for vector embeddings. Deletion occurs through `remove_embeddings()` when files are re-indexed or removed.

## Key Findings

### 1. Two Embedding Systems

SimpleIndexer manages two distinct embedding systems:

1. **SimpleSemanticSearch** (`semantic_search` field) - indexes doc comments
2. **VectorSearchEngine** (`vector_engine` field) - indexes symbol text via pending batches

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:82-89`

```rust
/// Optional vector search engine
vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
/// Optional embedding generator
embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
/// Symbols pending vector processing (SymbolId, symbol_text)
pending_embeddings: Vec<(SymbolId, String)>,
/// Optional semantic search for documentation
semantic_search: Option<Arc<Mutex<SimpleSemanticSearch>>>,
```

### 2. Symbol Creation to Embedding Creation

**Flow in `store_symbol()`** (`/Users/bartolli/Projects/codanna/src/indexing/simple.rs:1029-1079`):

1. If semantic search is enabled AND symbol has doc comment:
   - Get language from `file_languages` HashMap
   - Call `semantic_search.index_doc_comment_with_language(symbol_id, doc, language)`
   - Embedding is generated and stored immediately in memory

2. If vector engine is enabled:
   - Create symbol text using `create_symbol_text()`
   - Add to `pending_embeddings` Vec (not indexed yet)
   - Vector embeddings are batched for later processing

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:1029-1079`

```rust
fn store_symbol(&mut self, symbol: crate::Symbol, path_str: &str) -> IndexResult<()> {
    // Index doc comment for semantic search if enabled
    if let (Some(semantic), Some(doc)) = (&self.semantic_search, &symbol.doc_comment) {
        let language = self.file_languages.get(&symbol.file_id)...;
        semantic.lock().unwrap().index_doc_comment_with_language(symbol.id, doc, language)?;
    }

    // Store the symbol in Tantivy
    self.document_index.index_symbol(&symbol, path_str)?;

    // If vector support is enabled, prepare for embedding
    if self.vector_engine.is_some() && self.embedding_generator.is_some() {
        let symbol_text = create_symbol_text(&symbol.name, symbol.kind, symbol.signature.as_deref());
        self.pending_embeddings.push((symbol.id, symbol_text));
    }
    Ok(())
}
```

### 3. Symbol Deletion to Embedding Deletion

**Removal triggers**:

1. **`remove_file()`** - explicit file removal
2. **`index_file_internal()`** - re-indexing existing file
3. **`sync_with_config()`** - directory cleanup (calls `remove_file()`)

**The `remove_embeddings()` method** (`/Users/bartolli/Projects/codanna/src/semantic/simple.rs:329-334`):

```rust
pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) {
    for id in symbol_ids {
        self.embeddings.remove(id);
        self.symbol_languages.remove(id);
    }
}
```

Removes both the embedding vector AND the language mapping for each symbol.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:564-656`

In `remove_file()`:
```rust
// Get all symbols for this file before removing
let symbols = self.document_index.find_symbols_by_file(file_id)?;

// ... remove from Tantivy ...

// Remove embeddings for the symbols if semantic search is enabled
if !symbols_to_remove.is_empty() {
    if let Some(semantic) = &self.semantic_search {
        let symbol_ids: Vec<SymbolId> = symbols_to_remove.iter().map(|s| s.id).collect();
        semantic.lock().unwrap().remove_embeddings(&symbol_ids);
    }
}
```

### 4. Incremental Update Flow (Re-indexing)

**Flow in `index_file_internal()`** (`/Users/bartolli/Projects/codanna/src/indexing/simple.rs:469-561`):

When a file already exists and has changed:

1. **Capture** - Get symbols before removal: `find_symbols_by_file(file_id)`
2. **Preserve relationships** - `capture_incoming_relationships(&symbols_before)`
3. **Remove Tantivy docs** - `remove_file_documents(path_str)`
4. **Remove embeddings** - `semantic_search.remove_embeddings(&symbol_ids)`
5. **CRITICAL** - Save semantic state to disk to prevent cache desync
6. **Register file** - `register_file(path_str, content_hash)`
7. **Re-index content** - `reindex_file_content()` which calls `extract_and_store_symbols()`
8. Each new symbol calls `store_symbol()` which creates new embeddings

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:515-538`

```rust
// Remove embeddings for the old symbols if semantic search is enabled
if let Some(symbol_ids) = symbols_to_remove {
    if let Some(semantic) = &self.semantic_search {
        semantic.lock().unwrap().remove_embeddings(&symbol_ids);

        // CRITICAL: Save embeddings to disk after removal to prevent cache desync
        let semantic_path = self.settings.index_path.join("semantic");
        if let Err(e) = semantic.lock().unwrap().save(&semantic_path) {
            eprintln!("Warning: Failed to save semantic search after embedding removal: {e}");
        }
    }
}
```

### 5. The Orchestration Order

**Complete sequence for file re-indexing:**

```
File Change Detected
    |
    v
index_file_internal(path, force)
    |
    +-- Read file with hash
    |
    +-- Check if exists: get_file_info(path_str)
    |
    +-- If exists AND (force OR hash changed):
    |       |
    |       +-- 1. find_symbols_by_file(file_id)          # Get old symbols
    |       +-- 2. capture_incoming_relationships()        # Preserve call graph
    |       +-- 3. remove_file_documents(path_str)         # Delete from Tantivy
    |       +-- 4. remove_embeddings(&symbol_ids)          # Delete embeddings
    |       +-- 5. save() semantic state to disk           # Persist removal
    |
    +-- register_file(path_str, hash)                      # Register new/updated
    |
    +-- reindex_file_content(path, path_str, file_id, content)
            |
            +-- detect_language()
            +-- create_parser_with_behavior()
            +-- extract_and_store_symbols()  -->  store_symbol()
            |       |
            |       +-- index_doc_comment_with_language()  # Create embedding
            |       +-- index_symbol() to Tantivy
            |       +-- pending_embeddings.push()          # Queue vector
            |
            +-- extract_and_store_relationships()

    [Later, on commit_tantivy_batch()]
        |
        +-- commit_batch() to Tantivy
        +-- process_pending_embeddings()  -->  Generate vector embeddings
        +-- build_symbol_cache()
```

### 6. Language Separation for Scoring

**Structure in `SimpleSemanticSearch`** (`/Users/bartolli/Projects/codanna/src/semantic/simple.rs:40-47`):

```rust
pub struct SimpleSemanticSearch {
    /// Embeddings indexed by symbol ID
    embeddings: HashMap<SymbolId, Vec<f32>>,

    /// Language mapping for each symbol (for language-filtered search)
    symbol_languages: HashMap<SymbolId, String>,
    // ...
}
```

**Population during indexing** (`/Users/bartolli/Projects/codanna/src/semantic/simple.rs:185-201`):

```rust
pub fn index_doc_comment_with_language(
    &mut self,
    symbol_id: SymbolId,
    doc: &str,
    language: &str,
) -> Result<(), SemanticSearchError> {
    // First index the doc comment normally
    self.index_doc_comment(symbol_id, doc)?;

    // Then store the language mapping
    if self.embeddings.contains_key(&symbol_id) {
        self.symbol_languages.insert(symbol_id, language.to_string());
    }
    Ok(())
}
```

**Usage during search** (`/Users/bartolli/Projects/codanna/src/semantic/simple.rs:250-298`):

```rust
pub fn search_with_language(
    &self,
    query: &str,
    limit: usize,
    language: Option<&str>,
) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
    // Filter embeddings by language BEFORE computing similarity
    let filtered_embeddings: Vec<(&SymbolId, &Vec<f32>)> = if let Some(lang) = language {
        self.embeddings
            .iter()
            .filter(|(id, _)| {
                self.symbol_languages
                    .get(id)
                    .is_some_and(|symbol_lang| symbol_lang == lang)
            })
            .collect()
    } else {
        self.embeddings.iter().collect()
    };
    // ... compute similarity only on filtered set
}
```

### 7. Pending Embeddings and Batch Processing

**Vector embeddings use a deferred batch model:**

1. During `store_symbol()`: Add to `pending_embeddings: Vec<(SymbolId, String)>`
2. During `commit_tantivy_batch()`: Call `process_pending_embeddings()`
3. `process_pending_embeddings()` generates embeddings in batch, indexes to vector engine, clears pending

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:3262-3312`

```rust
fn process_pending_embeddings(
    &mut self,
    vector_engine: &Arc<Mutex<VectorSearchEngine>>,
    embedding_generator: &Arc<dyn EmbeddingGenerator>,
) -> IndexResult<()> {
    if self.pending_embeddings.is_empty() {
        return Ok(());
    }

    let texts: Vec<&str> = self.pending_embeddings.iter().map(|(_, text)| text.as_str()).collect();
    let embeddings = embedding_generator.generate_embeddings(&texts)?;

    // ... create vector pairs ...

    vector_engine.lock()?.index_vectors(&vectors)?;
    self.pending_embeddings.clear();
    Ok(())
}
```

## Architecture Patterns

### Dual-System Design
- `SimpleSemanticSearch`: Immediate embedding for doc comments (in-memory, persisted on save)
- `VectorSearchEngine`: Batched embedding for symbol text (deferred to commit)

### Consistency Model
- Semantic embeddings saved immediately after removal to prevent desync
- Vector embeddings processed atomically with Tantivy commit
- Symbol cache rebuilt after each commit for consistency

### Language Filtering Strategy
- Language stored per-symbol at index time
- Filtering happens BEFORE similarity computation (efficient)
- No cross-language results unless explicitly requested

## Conclusions

1. **Two parallel systems** - Doc comment embeddings (SimpleSemanticSearch) and symbol text embeddings (VectorSearchEngine) have different lifecycles.

2. **Immediate vs Deferred** - Doc embeddings are created immediately in `store_symbol()`, vector embeddings are batched until `commit_tantivy_batch()`.

3. **Cleanup is symmetric** - Both `embeddings` and `symbol_languages` are cleaned up together in `remove_embeddings()`.

4. **Re-index is atomic** - Old embeddings are removed, state is saved to disk, then new embeddings are created.

5. **Language filtering is pre-computation** - Filter by language before computing similarity, not after.
