# Research Report: Pipeline Embedding Architecture

**Date**: 2026-01-03 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The pipeline has two parallel embedding systems: `EmbedStage` (unused, targets `VectorSearchEngine`) and the active path through `IndexStage` using `EmbeddingPool` + `SimpleSemanticSearch`. The active flow embeds during INDEX stage, not as a separate stage. `EmbedStage` is fully implemented but never instantiated in production.

## Key Findings

### 1. Current Pipeline Stage Order

The documented architecture in `mod.rs`:

```text
DISCOVER -> READ -> PARSE -> COLLECT -> INDEX
   |          |        |        |         |
   v          v        v        v         v
[paths]  [content] [parsed] [batch]   Tantivy + Embeddings
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:6-19`

There is NO separate EMBED stage in the active flow. Embeddings are generated WITHIN the INDEX stage.

### 2. EmbedStage Status: Implemented but Unused

`EmbedStage` exists in `stages/embed.rs` and is:
- Fully implemented with `embed_and_store()` method
- Exported via `mod.rs` line 41: `pub use stages::embed::{EmbedStage, EmbedStats};`
- Used only in unit tests (lines 207-349)
- **Never instantiated in production code**

The grep for `EmbedStage::new` shows only test usage:
- `src/indexing/pipeline/stages/embed.rs:207` - test
- `src/indexing/pipeline/stages/embed.rs:219` - test
- `src/indexing/pipeline/stages/embed.rs:238` - test
- `src/indexing/pipeline/stages/embed.rs:263` - test
- `src/indexing/pipeline/stages/embed.rs:331` - test

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/embed.rs:20-92`

`EmbedStage` targets `VectorSearchEngine` (IVFFlat engine), while the active path uses `SimpleSemanticSearch`.

### 3. Active Embedding Flow

Embeddings are generated in `IndexStage.process_batch()`:

```rust
// From IndexStage
if let Some(pool) = &self.embedding_pool {
    // Parallel embedding generation using pool
    let embeddings = pool.embed_parallel(&embedding_items);

    // Store in semantic search
    let mut semantic_guard = semantic.lock()...;
    semantic_guard.store_embeddings(embeddings);
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/index.rs:200-230`

The flow is:
1. `IndexStage` receives `IndexBatch` from COLLECT
2. Writes symbols to Tantivy
3. Collects embedding candidates (symbols with `doc_comment`)
4. If `embedding_pool` is set, calls `pool.embed_parallel()`
5. Stores via `semantic_guard.store_embeddings()`

### 4. IndexBatch Structure

`IndexBatch` does NOT carry embedding data, only symbol data:

```rust
pub struct IndexBatch {
    pub symbols: Vec<(Symbol, PathBuf)>,
    pub imports: Vec<Import>,
    pub unresolved_relationships: Vec<UnresolvedRelationship>,
    pub file_registrations: Vec<FileRegistration>,
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:242-254`

Embedding text is derived from `Symbol.doc_comment` during INDEX stage, not passed through the batch.

### 5. EmbeddingPool Usage

`EmbeddingPool` is passed into the pipeline via `index_incremental()`:

```rust
pub fn index_incremental(
    &self,
    root: &Path,
    index: Arc<DocumentIndex>,
    semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
    embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
    force: bool,
) -> PipelineResult<IncrementalStats>
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:860-876`

Pool is wired through:
1. `index_incremental()` -> `index_incremental_with_progress()`
2. `index_files()` or `index_full()` -> `index_directory_with_semantic()`
3. `IndexStage::with_embedding_pool(pool)`

**Evidence for wiring**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/index.rs:67-74`

```rust
pub fn with_embedding_pool(mut self, pool: Arc<EmbeddingPool>) -> Self {
    self.embedding_pool = Some(pool);
    self
}
```

### 6. Two Embedding Systems

| System | Target Storage | Used In Production | Purpose |
|--------|---------------|-------------------|---------|
| `EmbedStage` | `VectorSearchEngine` (IVFFlat) | NO | Future vector search |
| `IndexStage` + `EmbeddingPool` | `SimpleSemanticSearch` | YES | Current semantic search |

`EmbedStage` uses:
- `EmbeddingGenerator` trait
- `VectorSearchEngine.index_vectors()`
- `VectorId` (mapped from SymbolId)

`IndexStage` uses:
- `EmbeddingPool` (fastembed TextEmbedding instances)
- `SimpleSemanticSearch.store_embeddings()`
- Direct `Vec<f32>` storage

## Architecture Diagram

```
Current Production Flow:
========================

DISCOVER -> READ -> PARSE -> COLLECT -> INDEX
                                           |
                                           +-> Tantivy writes (symbols, imports, files)
                                           |
                                           +-> if semantic && embedding_pool:
                                                 |
                                                 +-> pool.embed_parallel(symbols with doc_comments)
                                                 |
                                                 +-> semantic.store_embeddings(embeddings)


Unused EmbedStage Design:
=========================

COLLECT -> [EMBED] -> INDEX
              |
              +-> EmbedStage.embed_and_store()
              |       |
              |       +-> generator.generate_embeddings()
              |       +-> engine.index_vectors()
              |
              +-> VectorSearchEngine (IVFFlat clustering)
```

## Gap Analysis

### What is Missing for COLLECT -> EMBED -> INDEX

1. **IndexBatch lacks embedding data**: Would need `embedding_texts: Vec<(SymbolId, String)>` field
2. **No channel between COLLECT and EMBED**: Pipeline only has `batch_tx/rx` going to INDEX
3. **EmbedStage not instantiated**: Would need to create in `index_directory_with_semantic()`
4. **VectorSearchEngine not integrated**: Active path uses `SimpleSemanticSearch` instead

### Why EmbedStage was Not Used

Looking at the code history and design:
- `EmbedStage` targets `VectorSearchEngine` which uses IVFFlat clustering
- `SimpleSemanticSearch` is simpler (HashMap-based, no clustering)
- `EmbeddingPool` provides parallel embedding more directly
- The embedding flow was embedded (pun intended) into `IndexStage` for simplicity

## Recommendations

### Option A: Minimal Integration (Keep Current Structure)

The current flow works. `EmbedStage` remains as a future option for IVFFlat-based vector search.

**No changes needed** - the pipeline already supports parallel embeddings via `EmbeddingPool`.

### Option B: Separate EMBED Stage (If Needed)

If you want a separate EMBED stage:

1. Add channel between COLLECT and EMBED:
   ```rust
   let (embed_tx, embed_rx) = bounded(config.batch_channel_size);
   ```

2. Modify COLLECT to send to EMBED instead of INDEX

3. Create EMBED thread:
   ```rust
   let embed_handle = thread::spawn(move || {
       let stage = EmbedStage::new(generator);
       for batch in embed_rx {
           stage.embed_and_store(&batch.embedding_candidates, &mut engine)?;
           batch_tx.send(batch)?;
       }
   });
   ```

4. Pass embedding results through to INDEX stage

**Cost**: ~100-150 lines, adds complexity without clear benefit.

### Option C: Switch to VectorSearchEngine

If you want to use IVFFlat clustering for better large-scale search:

1. Replace `SimpleSemanticSearch` with `VectorSearchEngine` in `IndexFacade`
2. Wire `EmbedStage` into the pipeline
3. Migrate existing embeddings from HashMap to mmap storage

**Cost**: Significant refactor, unclear performance benefit at current scale.

## Conclusion

The embedding architecture is functional but has dual implementations. The active path (`EmbeddingPool` + `SimpleSemanticSearch` in `IndexStage`) works correctly. The `EmbedStage` module is complete but unused, designed for a different storage backend (`VectorSearchEngine`).

For the `feature/parallel-indexing-pipeline` branch, no changes are needed - embeddings are already generated in parallel via `EmbeddingPool.embed_parallel()` with batched 64-document calls.
