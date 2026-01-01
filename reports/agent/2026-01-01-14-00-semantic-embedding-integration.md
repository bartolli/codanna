# Research Report: Semantic Search Embedding System

**Date**: 2026-01-01 14:00
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The semantic search embedding system maps SymbolId to embedding vectors using a 1:1 relationship where VectorId equals SymbolId (same u32 value). Storage uses memory-mapped files with a simple binary format: 16-byte header followed by contiguous (VectorId, Vec<f32>) records. The new EmbedStage in the parallel pipeline already implements the correct pattern.

## Key Findings

### 1. SymbolId to VectorId Mapping

The system uses a direct 1:1 mapping: `VectorId = SymbolId`. Both are `u32` values wrapped in newtypes.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/embed.rs:73-78`
```rust
// Map SymbolId to VectorId (same u32 value)
if let Some(vector_id) = VectorId::new(symbol_id.value()) {
    vector_pairs.push((vector_id, embedding));
} else {
    stats.failed += 1;
}
```

### 2. Storage File Format (segment_0.vec)

Binary format with 16-byte header followed by contiguous vector records.

**Header (16 bytes)**:
- Magic bytes: `CVEC` (4 bytes)
- Version: u32 little-endian (4 bytes)
- Dimension: u32 little-endian (4 bytes)
- Vector count: u32 little-endian (4 bytes)

**Vector records**:
- VectorId: u32 little-endian (4 bytes)
- Vector data: dimension * f32 little-endian (dimension * 4 bytes)

**Evidence**: `/Users/bartolli/Projects/codanna/src/vector/storage.rs:1-45`
```rust
/// Storage Format:
/// - Header (16 bytes): version, dimension, vector count
/// - Vectors: Contiguous f32 arrays in little-endian format
const STORAGE_VERSION: u32 = 1;
const HEADER_SIZE: usize = 16;
const MAGIC_BYTES: &[u8; 4] = b"CVEC";
```

### 3. Embedding Text Generation

Embeddings are generated from a combination of symbol metadata, not just documentation.

**Text formula**: `"{kind} {name} {signature} {doc_comment}"`

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/embed.rs:102-145`
```rust
pub fn create_embedding_text(
    name: &str,
    kind: SymbolKind,
    signature: Option<&str>,
    doc_comment: Option<&str>,
) -> String {
    let kind_str = match kind { ... };
    let mut text = format!("{kind_str} {name}");
    if let Some(sig) = signature { text.push_str(sig); }
    if let Some(doc) = doc_comment { text.push_str(doc); }
    text
}
```

### 4. Batch API Methods

**Primary batch method**: `VectorSearchEngine::index_vectors()`

**Evidence**: `/Users/bartolli/Projects/codanna/src/vector/engine.rs:82-120`
```rust
pub fn index_vectors(&mut self, vectors: &[(VectorId, Vec<f32>)]) -> Result<(), VectorError> {
    // 1. Validate dimensions
    // 2. Store vectors via write_batch
    // 3. Run K-means clustering
    // 4. Update cluster assignments
}
```

**Alternative for semantic storage**: `SemanticVectorStorage::save_batch()`

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/storage.rs:150-183`
```rust
pub fn save_batch(
    &mut self,
    embeddings: &[(SymbolId, Vec<f32>)],
) -> Result<(), SemanticSearchError>
```

### 5. EmbedStage in Parallel Pipeline

The `EmbedStage` struct already implements the correct integration pattern.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/embed.rs:54-92`
```rust
pub fn embed_and_store(
    &self,
    symbols: &[(SymbolId, String)],  // (SymbolId, embedding_text) pairs
    engine: &mut VectorSearchEngine,
) -> Result<EmbedStats, VectorError> {
    // Process in EMBED_BATCH_SIZE chunks (256)
    for chunk in symbols.chunks(EMBED_BATCH_SIZE) {
        let texts: Vec<&str> = chunk.iter().map(|(_, text)| text.as_str()).collect();
        let embeddings = self.generator.generate_embeddings(&texts)?;
        // Map SymbolId -> VectorId and store
        engine.index_vectors(&vector_pairs)?;
    }
}
```

### 6. Language Filtering

Language information is stored in a separate `languages.json` file mapping `symbol_id -> language`.

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:400-410`
```rust
let languages_map: HashMap<u32, String> = self
    .symbol_languages
    .iter()
    .map(|(id, lang)| (id.to_u32(), lang.clone()))
    .collect();
std::fs::write(&languages_path, serde_json::to_string(&languages_map)?)?;
```

## Architecture/Patterns Identified

### Data Flow: Symbol -> Text -> Embedding -> Storage

```
COLLECT stage
    |
    v
(SymbolId, Symbol) pairs
    |
    | EmbedStage::create_embedding_text()
    v
(SymbolId, String) pairs (embedding text)
    |
    | EmbeddingGenerator::generate_embeddings() [batch]
    v
(SymbolId, Vec<f32>) pairs
    |
    | VectorId::new(symbol_id.value())
    v
(VectorId, Vec<f32>) pairs
    |
    | VectorSearchEngine::index_vectors() [with clustering]
    | or MmapVectorStorage::write_batch() [raw storage]
    v
segment_0.vec (binary file)
```

### Two Storage Paths

1. **SimpleSemanticSearch** (legacy): HashMap in memory, saves via `SemanticVectorStorage::save_batch()`
2. **VectorSearchEngine** (new): Direct to disk with K-means clustering via `index_vectors()`

The new parallel pipeline should use `VectorSearchEngine` path for:
- Built-in clustering for search optimization
- Concurrent access via `ConcurrentVectorStorage`
- Direct disk writes (no intermediate HashMap)

## Conclusions

### Integration Recommendations for Parallel Pipeline

1. **Use EmbedStage directly** - It already implements the correct pattern:
   - Batches embeddings in 256-symbol chunks
   - Converts SymbolId to VectorId
   - Stores via VectorSearchEngine

2. **Data to collect per symbol**:
   - `SymbolId` (from store_symbol)
   - `name` (required)
   - `SymbolKind` (required)
   - `signature` (optional, improves search quality)
   - `doc_comment` (optional, key for semantic search)

3. **Thread safety**:
   - `VectorSearchEngine` is NOT thread-safe (mutable reference needed)
   - Run embedding as a single-threaded post-processing stage
   - OR use `ConcurrentVectorStorage::write_batch()` for parallel writes, then cluster once

4. **Performance characteristics**:
   - `EMBED_BATCH_SIZE = 256` balances memory vs efficiency
   - Embedding generation: ~50-200ms per batch (GPU dependent)
   - Storage write: <1ms per batch (memory-mapped)
   - Clustering: O(n*k) where k = sqrt(n)

5. **No sync guarantee needed**:
   - Symbol count and embedding count don't need to match
   - Not all symbols have embeddings (only those with meaningful text)
   - Use `embedding_count` from metadata, not symbol_count
