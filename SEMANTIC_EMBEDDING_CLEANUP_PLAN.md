# Semantic Search Embedding Cleanup Plan

## Problem Summary
When files are re-indexed, old embeddings are not removed, causing:
- Embedding count to grow indefinitely (14 → 614)
- Stale embeddings in search results
- Memory usage increase over time

## Root Cause
The current implementation removes Tantivy documents but has no mechanism to remove semantic embeddings when symbols are deleted during re-indexing.

## Solution Design

### Phase 1: Add Embedding Removal API (2 hours)

**Task 1.1: Add remove_embeddings method to SimpleSemanticSearch**
```rust
impl SimpleSemanticSearch {
    /// Remove embeddings for specific symbols
    pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) {
        for id in symbol_ids {
            self.embeddings.remove(id);
        }
    }
}
```

**Task 1.2: Add remove_embeddings_for_symbols to SemanticVectorStorage**
```rust
impl SemanticVectorStorage {
    /// Remove embeddings for a batch of symbols
    pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) -> Result<(), SemanticSearchError> {
        // Implementation will track removed IDs for next save
    }
}
```

### Phase 2: Track Symbols for Removal (3 hours)

**Task 2.1: Collect symbols before file re-indexing**
- In `index_file_internal`, before calling `remove_file_documents()`
- Get all symbols for the file using `find_symbols_by_file()`
- Store these symbol IDs for removal

**Task 2.2: Remove embeddings for collected symbols**
- After `remove_file_documents()` but before re-indexing
- Call `semantic_search.remove_embeddings()` with collected IDs
- Only if semantic search is enabled

**Task 2.3: Update save logic to handle removals**
- Modify `SimpleSemanticSearch::save()` to skip removed embeddings
- Update metadata with correct embedding count

### Phase 3: Testing & Validation (2 hours)

**Task 3.1: Unit tests for removal methods**
- Test `remove_embeddings()` with various inputs
- Test persistence after removals
- Test empty edge cases

**Task 3.2: Integration test for re-indexing flow**
- Index file with doc comments
- Modify doc comment and re-index
- Verify old embedding removed, new one added
- Check embedding count stays stable

**Task 3.3: Performance validation**
- Ensure removal doesn't impact indexing speed
- Verify memory usage doesn't grow

### Phase 4: Handle Edge Cases (1 hour)

**Task 4.1: Batch removal optimization**
- If removing many symbols, optimize for batch operations
- Consider memory-mapped storage implications

**Task 4.2: Error recovery**
- Handle partial removal failures gracefully
- Ensure index consistency

## Implementation Guidelines Compliance

✅ **Zero-cost abstractions**: Using `&[SymbolId]` for removal lists
✅ **Single responsibility**: Each method does one thing
✅ **Type safety**: Using SymbolId newtype throughout
✅ **Error handling**: Structured errors with suggestions
✅ **API ergonomics**: Simple, clear method names
✅ **Performance**: Batch operations, no unnecessary allocations

## Backward Compatibility
- All changes are additive (new methods)
- Existing API remains unchanged
- Indexes without semantic search unaffected

## Success Criteria
1. Embedding count remains stable after re-indexing
2. No stale embeddings in search results
3. No performance regression
4. All existing tests pass

Total estimated time: 8 hours