# Research Report: COLLECT Stage ID Assignment

**Date**: 2026-01-02 15:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The COLLECT stage in the parallel indexing pipeline assigns FileId and SymbolId sequentially using a `CollectorState` struct with simple increment counters. The implementation is sound for batch indexing but has a potential race condition in single-file reindexing scenarios.

## Key Findings

### 1. ID Assignment Mechanism

The `CollectorState` maintains two counters that start at 0 and increment before returning each ID:

```rust
fn next_file_id(&mut self) -> FileId {
    self.file_counter += 1;
    FileId::new(self.file_counter).expect("FileId overflow")
}

fn next_symbol_id(&mut self) -> SymbolId {
    self.symbol_counter += 1;
    SymbolId::new(self.symbol_counter).expect("SymbolId overflow")
}
```

IDs start at 1 (increment happens before return). The `FileId::new()` and `SymbolId::new()` functions reject 0 values, returning `None` if passed 0.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:100-109`

### 2. Batching Does Not Affect ID Continuity

The `should_flush()` method only triggers batch flushing based on symbol count:

```rust
fn should_flush(&self) -> bool {
    self.current_batch.symbol_count() >= self.batch_size
}
```

When a batch is flushed via `take_batch()`, only the batch contents are moved out. The counters remain in `CollectorState` and continue incrementing. This ensures IDs are globally unique across batches.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:111-117`

### 3. Single-File Reindexing: Potential Race Condition

The `process_single()` method (used for watcher-based reindexing) reads the next available IDs from DocumentIndex:

```rust
pub fn process_single(
    &self,
    parsed: ParsedFile,
    index: Arc<crate::storage::DocumentIndex>,
) -> PipelineResult<(IndexBatch, Vec<UnresolvedRelationship>)> {
    let next_file_id = index.get_next_file_id()?;
    let next_symbol_id = index.get_next_symbol_id()?;

    let mut state = CollectorState::new(self.batch_size);
    state.file_counter = next_file_id.saturating_sub(1);
    state.symbol_counter = next_symbol_id.saturating_sub(1);
    // ...
}
```

**SPINACH**: There is a race condition window between:
1. Reading `get_next_file_id()` / `get_next_symbol_id()`
2. Starting a batch with `index.start_batch()`

If two watcher threads call `process_single()` concurrently, they could both read the same "next" ID value before either starts a batch.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:136-161`

### 4. DocumentIndex Pending Counters

The DocumentIndex uses pending counters during batch operations to prevent this exact issue:

```rust
pub fn start_batch(&self) -> StorageResult<()> {
    // ... create writer ...

    // Initialize the pending symbol counter for this batch
    let current = self
        .query_metadata(MetadataKey::SymbolCounter)?
        .unwrap_or(0) as u32;
    if let Ok(mut pending_guard) = self.pending_symbol_counter.lock() {
        *pending_guard = Some(current + 1);
    }
    // Similar for file counter
}
```

When `get_next_symbol_id()` is called during an active batch, it uses and increments the pending counter (protected by mutex). This prevents ID reuse within a single batch.

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/tantivy.rs` (start_batch, get_next_symbol_id)

### 5. The Actual Race Window

The race exists because `process_single()` calls:
1. `index.get_next_file_id()` - reads from metadata or pending counter
2. `index.get_next_symbol_id()` - reads from metadata or pending counter
3. Returns to caller
4. Caller later calls `index.start_batch()`

If no batch is active when `get_next_*_id()` is called, it reads from committed metadata. Two concurrent callers would get the same values.

**Mitigation in current code**: The `index_file_single()` method in Pipeline wraps the entire operation, and single-file indexing is typically serialized through the watcher queue. However, this is an implicit guarantee, not enforced by the type system.

## Architecture Patterns Identified

1. **Sequential ID Generation**: Uses simple increment counters rather than atomic or distributed ID generation. This is appropriate for the single-threaded COLLECT stage.

2. **Batch Isolation**: The pending counter mechanism in DocumentIndex provides batch-level isolation for ID generation during active batches.

3. **Cache-Based Relationship Resolution**: Symbols are cached in `CollectorCaches` for O(1) relationship reconnection by (name, file_id, range) tuple.

## Potential Issues

### ID Collision Risk (Low)

**Scenario**: Two concurrent `process_single()` calls without an active batch.

**Probability**: Low in practice because:
- File watcher events are typically processed sequentially
- `index_file_single()` starts a batch early in the cleanup path

**Recommendation**: Move the ID acquisition inside the batch scope, or make `process_single()` require an active batch.

### No ID Reuse After File Deletion

When a file is deleted via `CleanupStage`, its FileId and SymbolIds become orphaned but are never reclaimed. The counters only grow. This is intentional for simplicity and consistency but means IDs are not compacted.

## Conclusions

The COLLECT stage ID assignment is correct for the primary use case (batch indexing). The single-file reindexing path has a theoretical race condition that is mitigated by implicit serialization in the watcher infrastructure. To make the code more robust, consider:

1. Acquiring the batch lock before reading next IDs in `process_single()`
2. Adding a `process_single_with_batch()` variant that takes an already-started batch
3. Documenting the serialization requirement for single-file indexing

The current implementation will not produce ID collisions under normal operation, but the safety guarantee relies on external factors rather than the type system.
