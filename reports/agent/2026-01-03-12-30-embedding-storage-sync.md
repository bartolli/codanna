# Research Report: Embedding Storage Synchronization Architecture

**Date**: 2026-01-03 12:30
**Agent**: Research-Agent-v5
**Model**: Claude Opus 4.5

## Summary

The embedding CRUD lifecycle uses three storage files (`segment_0.vec`, `languages.json`, `metadata.json`) that are synchronized through a complete-rewrite strategy during save operations. The storage is append-only at the binary level but logically replaced on each save. No atomic write guarantees exist, creating crash vulnerability.

## Key Findings

### 1. Three Storage Files Structure

The semantic directory contains three distinct files:

| File | Purpose | Format | Size Example |
|------|---------|--------|--------------|
| `segment_0.vec` | Binary vector storage | Custom binary with 16-byte header | ~18.6MB |
| `languages.json` | symbol_id -> language mapping | JSON HashMap<u32, String> | ~220KB |
| `metadata.json` | Model info, counts, timestamps | JSON SemanticMetadata | ~155 bytes |

**Evidence**: `/Users/bartolli/Projects/codanna/.codanna/index/semantic/` directory listing and file inspection

### 2. Vector Storage Structure (segment_0.vec)

The `MmapVectorStorage` uses a simple binary format:

**Header (16 bytes)**:
- Magic bytes: `CVEC` (4 bytes)
- Version: u32 (4 bytes) - currently `1`
- Dimension: u32 (4 bytes) - e.g., `384` for AllMiniLML6V2
- Vector count: u32 (4 bytes)

**Vector records (variable size)**:
- Vector ID: u32 (4 bytes)
- Vector data: dimension * f32 (dimension * 4 bytes)

**Evidence**: `/Users/bartolli/Projects/codanna/src/vector/storage.rs:1-60`

```rust
const STORAGE_VERSION: u32 = 1;
const HEADER_SIZE: usize = 16;
const MAGIC_BYTES: &[u8; 4] = b"CVEC";
const BYTES_PER_F32: usize = 4;
const BYTES_PER_ID: usize = 4;
```

### 3. Storage Write Pattern

The storage is **append-only at runtime** but uses a **complete-rewrite strategy on save**:

**During indexing**: Vectors are appended to `segment_0.vec`
- `write_batch()` appends vectors to file
- `update_header_count()` updates vector count in header
- `flush()` called after both operations

**Evidence**: `/Users/bartolli/Projects/codanna/src/vector/storage.rs:196-230`

```rust
fn append_vectors(&self, vectors: &[(VectorId, Vec<f32>)]) -> Result<(), VectorStorageError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&self.path)?;
    // ... write vectors ...
    file.flush()?;
}
```

**On save (complete rewrite)**:
1. `SemanticVectorStorage::new()` first deletes existing `segment_0.vec`
2. Creates fresh storage file
3. Writes all embeddings from memory as a batch

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/storage.rs:25-47`

```rust
pub fn new(path: &Path, dimension: VectorDimension) -> Result<Self, SemanticSearchError> {
    // First, remove any existing storage file to ensure clean state
    let storage_path = path.join("segment_0.vec");
    if storage_path.exists() {
        std::fs::remove_file(&storage_path).map_err(|e| /* ... */)?;
    }
    // ... create new storage ...
}
```

### 4. Persistence Points and Write Order

The `SimpleSemanticSearch::save()` method writes files in this order:

1. **metadata.json** (first) - via `SemanticMetadata::save(path)`
2. **segment_0.vec** (second) - via `SemanticVectorStorage::new()` + `save_batch()`
3. **languages.json** (third) - via `std::fs::write()`

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:359-420`

```rust
pub fn save(&self, path: &Path) -> Result<(), SemanticSearchError> {
    // 1. Save metadata
    let metadata = SemanticMetadata::new(model_name, self.dimensions, self.embeddings.len());
    metadata.save(path)?;

    // 2. Create storage and save batch
    let mut storage = SemanticVectorStorage::new(path, dimension)?;
    storage.save_batch(&embeddings)?;

    // 3. Save language mappings
    let languages_path = path.join("languages.json");
    std::fs::write(&languages_path, languages_json)?;
}
```

### 5. Crash Vulnerability Analysis

**No atomic writes**: Files are written sequentially with no transaction wrapper or temporary file + rename pattern.

**Crash scenarios**:

| Crash Point | State | Recovery |
|-------------|-------|----------|
| After metadata.json | metadata shows count > 0, but no vectors | Load fails: storage file not found |
| During segment_0.vec | Partial vectors, corrupted file | Load may fail or return incomplete data |
| After segment_0.vec | Vectors exist, but languages.json missing | Load succeeds with empty language map (fallback) |

**Evidence for crash resilience gap**: No `atomic` or `rename` patterns found in semantic storage code.

### 6. Loading/Initialization Flow

On startup, `load_semantic_search()` checks for `metadata.json` existence:

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/facade.rs:201-214`

```rust
pub fn load_semantic_search(&mut self, path: &Path) -> FacadeResult<bool> {
    if path.join("metadata.json").exists() {
        match SimpleSemanticSearch::load(path) {
            Ok(semantic) => {
                self.semantic_search = Some(Arc::new(Mutex::new(semantic)));
                return Ok(true);
            }
            Err(e) => {
                tracing::warn!("Failed to load semantic search: {e}");
            }
        }
    }
    Ok(false)
}
```

The `SimpleSemanticSearch::load()` sequence:
1. Load `metadata.json` and validate version
2. Parse model name and initialize embedding model
3. Open `segment_0.vec` via `SemanticVectorStorage::open()`
4. Verify dimension matches metadata
5. Load all embeddings via `storage.load_all()`
6. Check if `languages.json` exists, load if present (optional fallback)

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:429-520`

### 7. Consistency Between Files

**Metadata count verification**:
The load process compares metadata's `embedding_count` with actual loaded vectors:

```rust
if embeddings_vec.len() != metadata.embedding_count {
    eprintln!(
        "WARNING: Expected {} embeddings but found {}",
        metadata.embedding_count,
        embeddings_vec.len()
    );
}
```

This is a warning only, not a failure condition.

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:460-465`

**Languages.json consistency**:
No validation exists. If `languages.json` contains keys for embeddings that no longer exist, they are silently ignored during search operations.

### 8. In-Memory vs On-Disk State

During indexing, embeddings exist in two places:
- `SimpleSemanticSearch.embeddings`: HashMap<SymbolId, Vec<f32>>
- `SimpleSemanticSearch.symbol_languages`: HashMap<SymbolId, String>

Deletes (`remove_embeddings()`) only modify in-memory state:

```rust
pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) {
    for id in symbol_ids {
        self.embeddings.remove(id);
        self.symbol_languages.remove(id);
    }
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/semantic/simple.rs:344-348`

The `segment_0.vec` file has no delete operation. Deleted embeddings remain in the file until the next full save, which rewrites everything.

## Architecture Diagram

```
                        SimpleSemanticSearch (Memory)
                       +---------------------------+
                       | embeddings: HashMap       |
                       | symbol_languages: HashMap |
                       | metadata: SemanticMetadata|
                       +---------------------------+
                                   |
                            save() |
                                   v
            +------------------+----------------+------------------+
            |                  |                |                  |
            v                  v                v                  |
    metadata.json       segment_0.vec    languages.json           |
    (155 bytes)         (18.6 MB)        (220 KB)                  |
    Written FIRST       Written SECOND   Written THIRD             |
                                                                   |
                                   +-------------------------------+
                                   | load()
                                   v
                        SimpleSemanticSearch (Memory)
```

## Identified Gaps

1. **No atomic writes**: Crash during save leaves inconsistent state
2. **No segment_0.vec compaction**: Deleted embeddings remain until full save
3. **Weak consistency checks**: Count mismatch is warning only
4. **No languages.json validation**: Orphan keys silently ignored
5. **Delete-then-create pattern**: `SemanticVectorStorage::new()` deletes old file before writing new

## Recommendations

1. **Atomic save pattern**: Write to temporary files, then rename
2. **Add consistency validation**: Fail load if metadata counts differ from actual
3. **Consider append-only log**: Mark deleted embeddings instead of full rewrite
4. **Transaction wrapper**: Write all three files atomically

## Conclusions

The embedding storage uses a simple but non-robust synchronization strategy. Files are written sequentially during save with no atomicity guarantees. The complete-rewrite pattern for `segment_0.vec` ensures consistency after successful save but creates vulnerability during the write operation. The system gracefully handles missing `languages.json` but has no recovery mechanism for corrupted `segment_0.vec` or incomplete writes.
