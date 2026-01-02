# Research Report: INDEX Stage Symbol Storage

**Date**: 2026-01-02 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The INDEX stage stores symbols in a `SymbolLookupCache` using DashMap. The `by_id` map **will overwrite** existing entries if a symbol with the same ID is inserted twice. However, this should not occur in normal operation because the COLLECT stage assigns sequential, unique IDs.

## Key Findings

### 1. DashMap Insert Behavior Overwrites

The `insert` method uses DashMap's default `insert`, which **overwrites** existing entries with the same key:

```rust
// Insert into by_id
self.by_id.insert(id, symbol);
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:376`

DashMap's `insert` returns `Option<V>` - the old value if the key existed. The current implementation ignores this return value, silently overwriting any existing symbol.

### 2. Secondary Indexes Accumulate Duplicates

While `by_id` overwrites, the secondary indexes `by_name` and `by_file_id` **append** without checking for duplicates:

```rust
// Insert into by_name (append to candidates)
self.by_name.entry(name).or_default().push(id);

// Insert into by_file_id (append to file's symbols)
self.by_file_id.entry(file_id).or_default().push(id);
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:379-382`

If the same symbol ID is inserted twice, `by_name` and `by_file_id` will have duplicate entries pointing to the same ID.

### 3. Symbol IDs Are Sequential in Full Indexing

The COLLECT stage assigns IDs sequentially via `next_symbol_id()`:

```rust
fn next_symbol_id(&mut self) -> SymbolId {
    self.symbol_counter += 1;
    SymbolId::new(self.symbol_counter).expect("SymbolId overflow")
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:108-111`

For full indexing, the counter starts at 0 and increments for each symbol. No duplicates possible.

### 4. Incremental Indexing Queries Next ID from Index

For single-file indexing (watcher), the COLLECT stage queries the next available IDs from DocumentIndex:

```rust
let next_file_id = index.get_next_file_id()?;
let next_symbol_id = index.get_next_symbol_id()?;
// ...
state.file_counter = next_file_id.saturating_sub(1);
state.symbol_counter = next_symbol_id.saturating_sub(1);
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:149-156`

This ensures incremental indexing continues from where the index left off.

### 5. from_index Could Cause Duplicates If Misused

The `from_index` method loads all symbols from Tantivy into the cache:

```rust
pub fn from_index(index: &crate::storage::DocumentIndex) -> PipelineResult<Self> {
    let cache = Self::with_capacity(count);
    let symbols = index.get_all_symbols(1_000_000)?;
    for symbol in symbols {
        cache.insert(symbol);
    }
    Ok(cache)
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:565-576`

If `from_index` is called after the INDEX stage has already populated the cache, or if Tantivy contains duplicate symbols, the cache would have issues.

## Potential Issues

### Issue 1: No Deduplication Logic

There is **no explicit deduplication** in the insert path. If a symbol with the same ID is inserted:
- `by_id`: Previous symbol is silently overwritten
- `by_name`: Duplicate ID appended to candidates list
- `by_file_id`: Duplicate ID appended to file's symbol list

**Risk Level**: Low for normal operation (IDs are sequential), but could cause subtle bugs if:
- A file is re-indexed without being removed first
- `from_index` is called on a populated cache
- Concurrent indexing assigns same ID (not possible with current design)

### Issue 2: No Warning on Overwrite

The return value of `self.by_id.insert(id, symbol)` is discarded. If a collision did occur, there would be no indication.

**Recommendation**: Add a debug assertion:
```rust
if let Some(old) = self.by_id.insert(id, symbol) {
    debug_assert!(false, "Symbol ID collision: {} overwrote {}", id, old.name);
}
```

### Issue 3: Secondary Index Cleanup Missing

When overwriting in `by_id`, the secondary indexes are not cleaned up. If a symbol named "foo" is replaced with a symbol named "bar" but same ID:
- `by_name["bar"]` gets the ID added
- `by_name["foo"]` still contains the old ID (stale reference)

**Mitigation**: This scenario cannot occur in practice because the COLLECT stage assigns new IDs to new symbols, not reusing IDs.

## Architecture

```
COLLECT Stage                         INDEX Stage                    SymbolLookupCache
     |                                      |                               |
     | (assigns sequential IDs)             | (writes to Tantivy)          |
     |                                      |                               |
     +----> Symbol {id: 1} -------------->  +---> symbol_cache.insert() -->|by_id: {1: Symbol}
     +----> Symbol {id: 2} -------------->  +---> symbol_cache.insert() -->|by_id: {2: Symbol}
     ...                                    ...                             |by_name: {"foo": [1,2]}
```

## Conclusions

1. **DashMap does overwrite** - This is by design and expected behavior
2. **No duplicates in normal operation** - Sequential ID assignment prevents collisions
3. **Silent failures possible** - If a collision occurred, it would be silently ignored
4. **Secondary indexes vulnerable** - Duplicate IDs would accumulate in `by_name`/`by_file_id` vectors

**Recommendation**: Consider adding a debug assertion to catch accidental overwrites during development, but the current design is correct for production use.
