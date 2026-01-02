# Research Report: SymbolLookupCache by_name Index

**Date**: 2026-01-02 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The `SymbolLookupCache.by_name` index correctly handles multiple symbols with the same name. The implementation uses `DashMap<Box<str>, Vec<SymbolId>>` which stores a vector of symbol IDs for each name, preventing overwrites when duplicate names are inserted.

## Key Findings

### 1. Data Structure is Correct for Multiple Symbols per Name

The `by_name` field is typed as `DashMap<Box<str>, Vec<SymbolId>>`, not `DashMap<Box<str>, SymbolId>`. This means each name maps to a vector of symbol IDs, which is the correct design for handling multiple symbols with the same name.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:340-341`
```rust
pub struct SymbolLookupCache {
    by_id: dashmap::DashMap<crate::types::SymbolId, crate::Symbol>,
    by_name: dashmap::DashMap<Box<str>, Vec<crate::types::SymbolId>>,
    by_file_id: dashmap::DashMap<crate::types::FileId, Vec<crate::types::SymbolId>>,
}
```

### 2. Insert Method Appends, Does Not Overwrite

The `insert` method uses `entry().or_default().push()` pattern which:
1. Gets or creates an empty `Vec` for the name key
2. Pushes the new symbol ID to that vector

This ensures no symbol IDs are lost when inserting symbols with duplicate names.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:375-379`
```rust
pub fn insert(&self, symbol: crate::Symbol) {
    let id = symbol.id;
    let file_id = symbol.file_id;
    let name: Box<str> = symbol.name.as_ref().into();

    // Insert into by_id
    self.by_id.insert(id, symbol);

    // Insert into by_name (append to candidates)
    self.by_name.entry(name).or_default().push(id);

    // Insert into by_file_id (append to file's symbols)
    self.by_file_id.entry(file_id).or_default().push(id);
}
```

### 3. lookup_candidates Returns All Candidates

The `lookup_candidates` method returns a clone of the entire vector of symbol IDs for a given name. If no symbols exist for that name, it returns an empty vector.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:400-404`
```rust
pub fn lookup_candidates(&self, name: &str) -> Vec<crate::types::SymbolId> {
    self.by_name
        .get(name)
        .map(|r| r.value().clone())
        .unwrap_or_default()
}
```

### 4. Existing Test Validates Duplicate Name Handling

The test `test_symbol_cache_lookup_by_name` explicitly tests the duplicate name scenario:
- Two symbols named "process_data" with IDs 1 and 2
- One symbol named "validate_input" with ID 3
- Asserts that `lookup_candidates("process_data")` returns exactly 2 candidates containing both IDs

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/index.rs:344-393`
```rust
// Add symbols with known names
let sym1 = make_test_symbol(1, "process_data", 1);
let sym2 = make_test_symbol(2, "process_data", 1); // Duplicate name, different ID
let sym3 = make_test_symbol(3, "validate_input", 1);
// ...
let candidates = symbol_cache.lookup_candidates("process_data");
assert_eq!(candidates.len(), 2);
assert!(candidates.contains(&SymbolId::new(1).unwrap()));
assert!(candidates.contains(&SymbolId::new(2).unwrap()));
```

### 5. Parallel Implementation in memory.rs Uses Same Pattern

The `SymbolStore` in `memory.rs` uses the same `entry().or_default().push()` pattern, confirming this is the established approach for multi-symbol-per-name indexing in the codebase.

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/memory.rs:27-28`
```rust
self.by_name.entry(name).or_default().push(id);
```

## Architecture/Patterns Identified

### Resolution Strategy Uses Tiered Disambiguation

When `lookup_candidates` returns multiple symbols, the `resolve()` method in `PipelineSymbolCache` uses a tiered approach to disambiguate:

1. **Tier 1 (Local)**: Same file symbols, preferring those defined before the reference
2. **Tier 2 (Import)**: Match against import aliases or path segments
3. **Tier 3 (Same Language)**: Three-level visibility check (same file, same module, public)

If multiple candidates remain after filtering, the method returns `ResolveResult::Ambiguous(candidates)` for higher-level disambiguation.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:450-540`

## Conclusions

**No issues found with the by_name index implementation.**

The design correctly:
- Stores multiple symbol IDs per name using `Vec<SymbolId>`
- Appends new entries rather than overwriting
- Returns all candidates for resolution
- Has test coverage for the duplicate name scenario

The implementation is thread-safe via DashMap's concurrent access guarantees. The `entry().or_default().push()` pattern is atomic within DashMap's entry API, preventing race conditions when multiple threads insert symbols with the same name.
