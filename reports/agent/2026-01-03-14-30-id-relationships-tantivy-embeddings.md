# Research Report: ID Relationships Between Tantivy and Embeddings

**Date**: 2026-01-03 14:30
**Agent**: Research-Agent-v5
**Model**: Claude Opus 4.5

## Summary

Symbol IDs serve as the primary key linking Tantivy storage with embedding storage. The ID lifecycle is monotonically increasing (never reused), with generation happening in the CollectStage and synchronization enforced through ordered cleanup operations. Orphan scenarios are handled gracefully with silent drops rather than errors.

## Key Findings

### 1. Symbol ID as Primary Key

The `SymbolId` is a wrapper around `u32` that serves as the universal identifier across both storage systems:

- In Tantivy: Stored as `symbol_id` field (u64 internally for Tantivy compatibility)
- In Embeddings: `HashMap<SymbolId, Vec<f32>>` in memory, converted to `VectorId` (also u32-based) for persistence

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:44-45`
```rust
/// Embeddings indexed by symbol ID
embeddings: HashMap<SymbolId, Vec<f32>>,
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/storage.rs:98-107`
```rust
// Convert SymbolId to VectorId (both are u32 internally)
let vector_id =
    VectorId::new(id.to_u32()).ok_or_else(|| SemanticSearchError::InvalidId {
        id: id.to_u32(),
        suggestion: "Symbol ID must be non-zero".to_string(),
    })?;
```

### 2. ID Generation Flow

Symbol IDs are generated in the **CollectStage** (single-threaded) to guarantee uniqueness:

```
Pipeline Flow:
DISCOVER -> READ -> PARSE -> COLLECT -> INDEX
                              ^
                              |
                    ID assignment here
```

The CollectStage:
1. Queries existing counters from Tantivy metadata before processing
2. Maintains internal counters that increment monotonically
3. Assigns IDs sequentially to ensure no collisions

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:23-29`
```rust
pub struct CollectStage {
    batch_size: usize,
    /// Starting file counter (for continuing from existing index)
    start_file_counter: u32,
    /// Starting symbol counter (for continuing from existing index)
    start_symbol_counter: u32,
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:108-113`
```rust
fn next_symbol_id(&mut self) -> SymbolId {
    self.symbol_counter += 1;
    SymbolId::new(self.symbol_counter).expect("SymbolId overflow")
}
```

### 3. ID Persistence and Counter Management

The counter is persisted in Tantivy metadata and queried at pipeline start:

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/metadata_keys.rs`
- `SymbolCounter`: Key for storing next available symbol ID
- `FileCounter`: Key for storing next available file ID

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:123-125`
```rust
// Query existing ID counters BEFORE spawning threads
let start_file_counter = index.get_next_file_id()?.saturating_sub(1);
let start_symbol_counter = index.get_next_symbol_id()?.saturating_sub(1);
```

### 4. No ID Reuse

Symbol IDs are **never reused** after deletion:

1. Counter is monotonically increasing
2. Deleted symbols leave gaps in the ID space
3. No garbage collection or ID recycling mechanism exists

This design choice ensures:
- Simpler implementation (no free-list management)
- Deterministic IDs for testing/debugging
- Safe for 4 billion symbols (u32 max) which is practically unlimited

### 5. Embedding-to-Symbol Lookup Flow

During semantic search:

```
Query -> Embed Query -> Cosine Similarity -> SymbolId list -> Symbol Lookup
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/facade.rs:706-716`
```rust
let results = sem.search_with_language(query, limit, language_filter)?;

let mut symbols = Vec::new();
for (symbol_id, score) in results {
    if let Some(symbol) = self.get_symbol(symbol_id) {
        symbols.push((symbol, score));
    }
}
```

### 6. Mismatch Handling (Integrity)

**Scenario A**: Embedding exists but symbol doesn't (orphan embedding)
- **Behavior**: Silent drop during search - the `if let Some(symbol)` simply filters it out
- **Root cause**: Possible crash between embedding creation and Tantivy commit
- **No error raised**: Results simply exclude the orphan

**Scenario B**: Symbol exists but embedding doesn't
- **Behavior**: Symbol won't appear in semantic search results
- **Root cause**: Symbol without doc comment, or embedding generation failed
- **Expected behavior**: Only documented symbols get embeddings

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/index.rs:190-198`
```rust
let embedding_items: Vec<(crate::SymbolId, &str, &str)> = if self.semantic.is_some() {
    batch
        .symbols
        .iter()
        .filter_map(|(symbol, _)| {
            symbol.doc_comment.as_ref().map(|doc| { ... })
        })
        .collect()
```

### 7. Cleanup Synchronization Order

Critical order for maintaining consistency during file re-indexing:

```
1. Get symbols for file (from Tantivy)
2. Remove embeddings for those symbols (from SimpleSemanticSearch)
3. Save embeddings to disk (prevents desync on crash)
4. Remove file documents from Tantivy
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/cleanup.rs:1-13`
```rust
//! The cleanup order is critical for embedding sync:
//! 1. Get symbols for file
//! 2. Remove embeddings for those symbols
//! 3. Save embeddings to disk (prevents desync on crash)
//! 4. Remove file documents from Tantivy
```

### 8. Language Mapping Persistence

The `languages.json` file stores symbol_id to language mappings:

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:401-407`
```rust
let languages_path = path.join("languages.json");
let languages_map: HashMap<u32, String> = self
    .symbol_languages
    .iter()
    .map(|(id, lang)| (id.to_u32(), lang.clone()))
    .collect();
```

Format: `{"1360":"rust","1361":"rust",...}`

This allows language-filtered semantic search without loading all symbols from Tantivy.

## Architecture Diagram: ID Flow

```
+------------------+     +-------------------+     +------------------+
|   CollectStage   |     |   IndexStage      |     |   CleanupStage   |
+------------------+     +-------------------+     +------------------+
         |                        |                        |
         | Generate SymbolId      | Store both:            | Remove both:
         | (monotonic counter)    | 1. Tantivy symbol      | 1. Tantivy symbol
         |                        | 2. Embedding           | 2. Embedding
         v                        v                        v
+------------------+     +-------------------+     +------------------+
|  CollectorState  |     |  DocumentIndex    |     |  DocumentIndex   |
|  .symbol_counter |     |  (Tantivy)        |     |  find_symbols_   |
+------------------+     +-------------------+     |  by_file()       |
                                  |                +------------------+
                                  |                        |
                                  v                        v
                         +-------------------+     +------------------+
                         | SimpleSemanticSearch|    | SimpleSemanticSearch|
                         | .embeddings       |     | .remove_embeddings()|
                         | HashMap<SymbolId, |     +------------------+
                         |   Vec<f32>>       |
                         +-------------------+
                                  |
                                  v (save)
                         +-------------------+
                         | SemanticVectorStorage|
                         | segment_0.vec     |
                         | languages.json    |
                         +-------------------+
```

## ID Type Conversion

```
SymbolId(u32) <---> VectorId(u32) <---> Tantivy field_u64
     ^                    ^                    ^
     |                    |                    |
  In-memory           On-disk            Tantivy index
  embeddings          vectors            (stored as u64
  (HashMap key)       (mmap file)        for compatibility)
```

## Potential Integrity Risks

1. **Crash during IndexStage**: Symbol in Tantivy, embedding missing or vice versa
   - Mitigated by atomic batch commits
   - Recovery: Re-index affected files

2. **Counter desync**: If Tantivy metadata not committed but files written
   - Mitigated by counter persisted in Tantivy metadata
   - Recovery: Counter re-read on next pipeline run

3. **Orphan embeddings after partial cleanup**: Crash between step 2-4 of cleanup
   - Mitigated by save-to-disk after embedding removal (step 3)
   - Silent drop during search (graceful degradation)

## Conclusions

1. **Symbol ID is the single source of truth** for linking Tantivy and embeddings
2. **IDs are never reused** - monotonically increasing, gaps allowed after deletion
3. **No explicit integrity checks** - relies on ordered operations and graceful handling
4. **Orphan handling is silent** - missing symbols during search are simply filtered out
5. **languages.json** provides auxiliary mapping for language-filtered search without Tantivy queries

The architecture is simple and robust, trading storage efficiency (sparse IDs) for implementation simplicity and crash safety.
