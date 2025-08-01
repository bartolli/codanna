# Semantic Search Persistence Analysis

## Current Infrastructure

### 1. Vector Storage Infrastructure (✅ EXISTS)
We have a complete vector storage system in `src/vector/storage.rs`:
- **MmapVectorStorage**: Memory-mapped vector storage with <1μs access times
- **Binary format**: Efficient storage with header + contiguous f32 arrays
- **Persistence**: Vectors are saved to `.vec` files and can be loaded back
- **Thread-safe**: ConcurrentVectorStorage wrapper for multi-threaded access

### 2. Current Semantic Search Implementation
The `SimpleSemanticSearch` in `src/semantic/simple.rs`:
- Stores embeddings in memory: `HashMap<SymbolId, Vec<f32>>`
- Not persisted when index is saved
- Lost when index is reloaded

### 3. Integration Gap
The `SimpleIndexer` has:
- `semantic_search: Option<Arc<Mutex<SimpleSemanticSearch>>>`
- No save/load methods for semantic search state
- No integration with the existing vector storage infrastructure

## Solution Path

### Option 1: Direct Integration with MmapVectorStorage
Modify `SimpleSemanticSearch` to use `MmapVectorStorage` instead of HashMap:
```rust
pub struct SimpleSemanticSearch {
    storage: MmapVectorStorage,  // Instead of HashMap
    model: Mutex<TextEmbedding>,
    dimensions: usize,
}
```

### Option 2: Add Persistence Methods
Add save/load methods to SimpleSemanticSearch:
```rust
impl SimpleSemanticSearch {
    pub fn save(&self, path: &Path) -> Result<()> {
        // Use MmapVectorStorage to save embeddings
    }
    
    pub fn load(path: &Path) -> Result<Self> {
        // Load embeddings from MmapVectorStorage
    }
}
```

### Option 3: Integrate with DocumentIndex
The `DocumentIndex` already has vector support infrastructure:
- `with_vector_support()` method
- `vector_engine` field
- Cluster cache for efficient search

## Recommendation

The infrastructure exists but needs connecting:
1. Modify `SimpleSemanticSearch` to use `MmapVectorStorage` internally
2. Add persistence hooks in `IndexPersistence::save()` and `load()`
3. Store semantic embeddings in `.codanna/index/semantic/` directory

This would:
- Reuse existing efficient storage format
- Maintain <1μs access times
- Enable semantic search to persist across sessions
- Make the MCP tool functional after index reload