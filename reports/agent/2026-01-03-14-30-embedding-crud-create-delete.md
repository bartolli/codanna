# Research Report: Embedding CRUD Lifecycle - CREATE and DELETE Operations

**Date**: 2026-01-03 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The embedding lifecycle is tightly coupled with the pipeline stages. CREATE operations occur in the INDEX stage via `EmbeddingPool.embed_parallel()` and `SimpleSemanticSearch.store_embeddings()`. DELETE operations occur in the CLEANUP stage via `SimpleSemanticSearch.remove_embeddings()`. The symbol_id serves as the direct key for embedding storage (1:1 mapping).

## Key Findings

### 1. CREATE Flow: Symbol to Embedding Storage

The CREATE flow follows this call chain:

```
IndexStage.process_batch()
  -> filters symbols with doc_comments
  -> EmbeddingPool.embed_parallel([(SymbolId, doc_text, language)])
     -> EmbeddingPool.embed_one() for each symbol (parallel via rayon)
     -> returns Vec<(SymbolId, Vec<f32>, String)>
  -> SimpleSemanticSearch.store_embeddings(items)
     -> HashMap<SymbolId, Vec<f32>>.insert(symbol_id, embedding)
     -> HashMap<SymbolId, String>.insert(symbol_id, language)
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/index.rs:200-225`

```rust
if let Some(pool) = &self.embedding_pool {
    // Parallel embedding generation using pool
    let embeddings = pool.embed_parallel(&embedding_items);
    let count = embeddings.len();

    // Store in semantic search
    let mut semantic_guard = semantic.lock().map_err(|_| PipelineError::Parse {
        path: PathBuf::new(),
        reason: "Failed to lock semantic search".to_string(),
    })?;
    semantic_guard.store_embeddings(embeddings);
}
```

The symbol_id mapping is established at embedding generation time - the same SymbolId that was assigned during symbol creation is passed directly to the embedding pool.

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:206-216`

```rust
pub fn store_embeddings(&mut self, items: Vec<(SymbolId, Vec<f32>, String)>) -> usize {
    let mut count = 0;
    for (symbol_id, embedding, language) in items {
        if embedding.len() == self.dimensions {
            self.embeddings.insert(symbol_id, embedding);
            self.symbol_languages.insert(symbol_id, language);
            count += 1;
        }
    }
    count
}
```

### 2. DELETE Flow: Symbol Removal to Embedding Cleanup

The DELETE flow follows this call chain:

```
CleanupStage.cleanup_files([PathBuf])
  -> CleanupStage.cleanup_single_file(path)
     -> DocumentIndex.get_file_info(path) -> file_id
     -> DocumentIndex.find_symbols_by_file(file_id) -> [Symbol]
     -> extract symbol_ids from symbols
     -> SimpleSemanticSearch.remove_embeddings(&[SymbolId])
        -> HashMap.remove(symbol_id) for embeddings
        -> HashMap.remove(symbol_id) for symbol_languages
     -> DocumentIndex.remove_file_documents(path)
  -> SimpleSemanticSearch.save(semantic_path)  // Persists changes immediately
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/cleanup.rs:92-121`

```rust
fn cleanup_single_file(&self, path: &Path) -> PipelineResult<(usize, usize)> {
    // Step 1: Get file_id from path
    let file_info = self.index.get_file_info(&path_str)?;
    let Some((file_id, _hash)) = file_info else {
        return Ok((0, 0));
    };

    // Step 2: Get all symbols for this file
    let symbols = self.index.find_symbols_by_file(file_id)?;
    let symbol_ids: Vec<SymbolId> = symbols.iter().map(|s| s.id).collect();

    // Step 3: Remove embeddings (if semantic search is enabled)
    if let Some(ref semantic) = self.semantic {
        let mut semantic_guard = semantic.lock()?;
        semantic_guard.remove_embeddings(&symbol_ids);
    }

    // Step 4: Remove file documents from Tantivy
    self.index.remove_file_documents(&path_str)?;
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:344-349`

```rust
pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) {
    for id in symbol_ids {
        self.embeddings.remove(id);
        self.symbol_languages.remove(id);
    }
}
```

### 3. Timing: When Operations Occur in Pipeline

**CREATE timing:**
- Embeddings are generated during the INDEX stage (Phase 1)
- Happens AFTER file parsing, DURING symbol indexing to Tantivy
- Batched: symbols are collected per file, then batch-embedded
- Embeddings are stored in-memory (HashMap) during indexing
- Persistence to disk (`segment_0.vec`) happens AFTER all indexing completes

**DELETE timing:**
- Cleanup occurs BEFORE re-indexing (for modified files)
- Cleanup occurs SEPARATELY for deleted files (detected in discover stage)
- Embeddings are saved to disk IMMEDIATELY after cleanup to prevent desync
- Critical order (from cleanup.rs:13-18):
  1. Get symbols for file
  2. Remove embeddings for those symbols
  3. Save embeddings to disk (prevents desync on crash)
  4. Remove file documents from Tantivy

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:938-960`

```rust
// Cleanup deleted files
if !discover_result.deleted_files.is_empty() {
    let stats = cleanup_stage.cleanup_files(&discover_result.deleted_files)?;
}
// Cleanup modified files (before re-indexing)
if !discover_result.modified_files.is_empty() {
    let stats = cleanup_stage.cleanup_files(&discover_result.modified_files)?;
}
```

### 4. Embedding Pool Architecture

The `EmbeddingPool` manages multiple model instances for parallel embedding:

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/pool.rs:22-40`

- Uses `crossbeam_channel` for model acquisition/release
- Each model instance is ~86MB (AllMiniLML6V2)
- `embed_parallel()` uses rayon for true parallel processing
- Model instances are acquired, used, then returned to pool

```rust
pub fn embed_parallel(&self, items: &[(SymbolId, &str, &str)]) -> Vec<(SymbolId, Vec<f32>, String)> {
    items.par_iter()
        .filter_map(|(symbol_id, doc, language)| {
            match self.embed_one(doc) {
                Ok(embedding) => Some((*symbol_id, embedding, (*language).to_string())),
                Err(e) => None,
            }
        })
        .collect()
}
```

### 5. Storage Files

The embedding storage uses:
- `segment_0.vec` - Binary vector storage via `MmapVectorStorage`
- `languages.json` - Maps symbol_id (u32) to language string
- `metadata.json` - Model name, dimension, embedding count

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:360-425`

The save operation:
1. Creates/ensures directory exists
2. Writes metadata.json with model info
3. Creates `SemanticVectorStorage` (wraps `MmapVectorStorage`)
4. Batch-writes all embeddings to segment_0.vec
5. Writes languages.json

## Architecture/Patterns Identified

### Symbol-Embedding ID Mapping
The mapping is direct: `SymbolId` = embedding key. No separate ID space.
- `VectorId::new(symbol_id.value())` converts for storage
- `SymbolId::new(vector_id.get())` converts back on load

### Transactional Guarantees
- Cleanup saves embeddings IMMEDIATELY after removal (crash safety)
- Indexing saves embeddings AFTER all files indexed (batch efficiency)
- Tantivy commits are batched (configurable via `batches_per_commit`)

### Memory vs Disk
- During indexing: embeddings live in `HashMap<SymbolId, Vec<f32>>`
- On disk: embeddings in `segment_0.vec` via memory-mapped storage
- Load operation reads all vectors into HashMap at startup

## Conclusions

1. **CREATE**: Embeddings generated in INDEX stage, stored in-memory, persisted after full index
2. **DELETE**: Embeddings removed in CLEANUP stage, persisted immediately for crash safety
3. **Mapping**: Direct 1:1 mapping via SymbolId - no translation layer
4. **Triggers**: File deletion triggers cleanup via discover stage; file modification triggers cleanup before re-index
5. **Batching**: CREATE is batched per-file (256 symbols max); DELETE is per-file

The system maintains consistency by:
- Always removing embeddings BEFORE removing Tantivy documents
- Always saving embeddings after cleanup (but after full index for CREATE)
- Using the same SymbolId throughout the lifecycle
