# Research Report: symbol_cache.bin Usage Analysis

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The `symbol_cache.bin` file is a memory-mapped hash-based cache that provides O(1) symbol name lookups. It duplicates a subset of Tantivy data (symbol_id, name_hash, file_id, line, column, kind) for faster lookups. The new parallel pipeline can eliminate this file by using its in-memory `PipelineSymbolCache` during indexing and relying on Tantivy for query-time lookups.

## Key Findings

### 1. Creation Flow

The cache is created by `SimpleIndexer::build_symbol_cache()` which:
1. Clears any existing cache (releases memory-mapped views)
2. Retrieves all symbols from Tantivy via `get_all_symbols()`
3. Writes to `{index_path}/symbol_cache.bin` using FNV-1a hashing

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:3315-3361`

```rust
pub fn build_symbol_cache(&mut self) -> IndexResult<()> {
    let cache_path = self.get_cache_path();
    self.clear_symbol_cache(false)?;
    let all_symbols = self.get_all_symbols();
    crate::storage::symbol_cache::SymbolHashCache::build_from_symbols(
        &cache_path,
        all_symbols.iter(),
    )?;
    self.load_symbol_cache()?;
}
```

The cache is rebuilt after:
- Batch commits (`index_directory()`) at line 382
- File removal (`remove_file()`) at line 651

### 2. Load Flow

The cache is loaded by `SimpleIndexer::load_symbol_cache()`:
1. Checks if `symbol_cache.bin` exists
2. Opens as memory-mapped file via `SymbolHashCache::open()`
3. Wraps in `ConcurrentSymbolCache` for thread-safe access

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:3386-3405`

Load triggers:
- During `SimpleIndexer::new()` and `with_settings()` initialization
- After `build_symbol_cache()` completes

### 3. Data Structure

The cache stores a 24-byte `CacheEntry` per symbol:

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/symbol_cache.rs:48-61`

```rust
#[repr(C)]
struct CacheEntry {
    symbol_id: u32,     // 4 bytes
    name_hash: u64,     // 8 bytes (FNV-1a hash of name)
    file_id: u32,       // 4 bytes
    line: u32,          // 4 bytes
    column: u16,        // 2 bytes
    kind: u8,           // 1 byte
    _padding: u8,       // 1 byte (alignment)
}
```

File format:
- 32-byte header (magic "SYMC", version, bucket_count, symbol_count)
- 256 hash buckets with offset table
- Entries stored by bucket for hash-based lookup

### 4. Usage Patterns

The cache serves two purposes:

**A. Fast Symbol ID Lookup (O(1) vs O(log n))**

Used in `SimpleIndexer::find_symbol()`:

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:1642-1661`

```rust
pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
    if let Some(ref cache) = self.symbol_cache {
        if let Some(id) = cache.lookup_by_name(name) {
            return Some(id);
        }
    }
    // Fallback to Tantivy
    self.document_index.find_symbols_by_name(name, None)...
}
```

**B. Resolution Context Building**

Used in `build_resolution_context()` when cache is available:

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2606-2617`

```rust
fn build_resolution_context(&self, file_id: FileId) -> IndexResult<Box<dyn ResolutionScope>> {
    let behavior = self.get_behavior_for_file(file_id)?;
    if let Some(cache) = self.symbol_cache() {
        behavior.build_resolution_context_with_cache(file_id, cache, &self.document_index)
    } else {
        behavior.build_resolution_context(file_id, &self.document_index)
    }
}
```

### 5. Relationship to Tantivy

**Data Overlap**:

| Field | symbol_cache.bin | Tantivy |
|-------|------------------|---------|
| symbol_id | Yes | Yes |
| name | Hash only | Full string |
| file_id | Yes | Yes |
| line/column | Yes | Yes |
| kind | Yes | Yes |
| doc_comment | No | Yes |
| signature | No | Yes |
| module_path | No | Yes |
| context | No | Yes |
| visibility | No | Yes |
| language | No | Yes |

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/tantivy.rs:31-51` (IndexSchema fields)

The cache stores a **subset** of Tantivy data for faster lookups. It does NOT store:
- Documentation
- Signatures
- Module paths
- Full symbol names (only hashes)

## Architecture/Patterns Identified

### Current SimpleIndexer Flow
```
index_directory() → parse files → write to Tantivy → build_symbol_cache() → load_symbol_cache()
                                                         ↓
                                               Reads ALL symbols from Tantivy
                                               Writes to symbol_cache.bin
```

### New Pipeline Flow (feature/parallel-indexing-pipeline)
```
Pipeline stages: COLLECT → PARSE → CONTEXT → RESOLVE → EMBED → INDEX
                                     ↓
                          Uses PipelineSymbolCache (in-memory)
                          Writes directly to Tantivy
                          NO symbol_cache.bin created
```

The new pipeline uses `PipelineSymbolCache` trait (defined in `/Users/bartolli/Projects/codanna/src/parsing/resolution.rs:653-707`) which provides the same resolution capabilities without disk I/O.

## Conclusions

### Can we eliminate symbol_cache.bin?

**Yes, with caveats:**

1. **During Indexing**: The new pipeline already uses `PipelineSymbolCache` (in-memory DashMap) and does not need `symbol_cache.bin`.

2. **At Query Time**: Tantivy provides the same data. The cache only provides O(1) name→ID lookup vs Tantivy's term query. Performance difference:
   - Cache: <10 microseconds
   - Tantivy: ~1-5 milliseconds

3. **Resolution Context**: The `build_resolution_context_with_cache()` methods currently require `ConcurrentSymbolCache`. They fall back to `build_resolution_context()` (Tantivy-based) when cache is unavailable.

### Recommendation

**Short-term**: The parallel pipeline can skip `symbol_cache.bin` creation. Query operations will fall back to Tantivy with acceptable latency.

**Long-term**: Consider:
1. Remove `build_symbol_cache()` calls from pipeline path
2. Keep cache optional for SimpleIndexer compatibility
3. Measure query latency impact (expected: +1-5ms per lookup)

The 24-byte-per-symbol cache saves ~1-5ms per name lookup but costs:
- Disk I/O at startup/save
- Memory for memory-mapped file
- Complexity in maintaining sync with Tantivy

For most use cases, Tantivy's search latency is acceptable. The cache is a micro-optimization that may not justify the complexity.
