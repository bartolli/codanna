# Research Report: Pipeline File Flow and Ordering

**Date**: 2026-01-02 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

Files from multiple directories flow through a 5-stage pipeline with parallel processing. File discovery order is non-deterministic due to parallel walking. ID assignment happens in a single-threaded COLLECT stage, ensuring sequential IDs but with non-deterministic file ordering. **Critical finding**: The `index_files()` method used for incremental indexing has a bug - it creates a new `CollectStage` starting at counter 0, which would cause ID collisions with existing indexed files.

## Key Findings

### 1. Discovery Stage Uses Parallel Walker

The `DiscoverStage` uses the `ignore` crate's parallel walker with 4 threads by default. Files are discovered in non-deterministic order based on thread scheduling.

```rust
let walker = WalkBuilder::new(&self.root)
    .hidden(false)
    .git_ignore(true)
    .threads(self.threads)
    .build_parallel();
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/discover.rs:47-54`

Files enter the pipeline in arbitrary order depending on which walker thread finds them first.

### 2. Files Are Mixed in READ and PARSE Stages

Multiple threads (configurable via `read_threads` and `parse_threads`) process files concurrently:

- READ stage: `read_threads` workers pull from path channel
- PARSE stage: `parse_threads` workers pull from content channel

Files are processed by whichever thread is available, mixing them further.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:125-180`

### 3. ID Assignment is Sequential but File Order is Not

The COLLECT stage is single-threaded and assigns FileId/SymbolId sequentially:

```rust
fn next_file_id(&mut self) -> FileId {
    self.file_counter += 1;
    FileId::new(self.file_counter).expect("FileId overflow")
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:103-107`

IDs are sequential (1, 2, 3...) but the **order of files receiving these IDs** depends on which file arrives at COLLECT first from the parallel PARSE stage.

### 4. Multi-Directory Indexing Calls Pipeline Sequentially

When `sync_with_config` indexes multiple directories, it calls `index_directory` in a loop:

```rust
for path in &to_add {
    let result = self.index_directory(path, false)?;
    stats.files_indexed += result.files_indexed;
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/facade.rs:1055-1059`

Each directory runs the full pipeline independently.

### 5. BUG CONFIRMED: index_files() Does Not Continue ID Counters

The `index_files()` method used for incremental indexing creates a fresh `CollectStage`:

```rust
// Stage 3: COLLECT
let collect_handle = thread::spawn(move || {
    let stage = CollectStage::new(batch_size);  // <-- Starts at counter 0!
    stage.run(parsed_rx, batch_tx)
});
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:1102-1105`

This contrasts with `process_single()` which correctly queries existing IDs:

```rust
pub fn process_single(&self, parsed: ParsedFile, index: Arc<DocumentIndex>) -> PipelineResult<...> {
    let next_file_id = index.get_next_file_id()?;
    let next_symbol_id = index.get_next_symbol_id()?;
    state.file_counter = next_file_id.saturating_sub(1);
    state.symbol_counter = next_symbol_id.saturating_sub(1);
    // ...
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/collect.rs:137-150`

### Why This Bug Exists

The `index_files()` method is called from `index_incremental_with_progress()` when processing new or modified files:

```rust
let (index_stats, unresolved, symbol_cache) = self.index_files(
    &files_to_index,
    Arc::clone(&index),
    semantic.clone(),
    progress.clone(),
)?;
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/mod.rs:948-953`

For incremental mode, if there are existing files with IDs (say 1-100), and we add 10 new files, they would get IDs 1-10, colliding with existing entries.

## Architecture Overview

```
DISCOVER (4 threads) --> paths arrive in arbitrary order
     |
     v
READ (N threads) ------> files read by whichever thread is free
     |
     v
PARSE (N threads) -----> files parsed by whichever thread is free
     |
     v
COLLECT (1 thread) ----> IDs assigned sequentially but to files in arrival order
     |                   BUG: starts at 0 instead of querying existing max ID
     v
INDEX (1 thread) ------> Tantivy writes
```

## File Flow Summary

| Aspect | Behavior |
|--------|----------|
| Discovery order | Non-deterministic (parallel walker) |
| Language grouping | None |
| Directory grouping | None within a single pipeline run |
| ID assignment | Sequential within run, but file-to-ID mapping varies |
| Cross-directory mixing | Not within same run (sequential calls) |

## Impact Assessment

### Within a Single Directory (Fresh Index)
- Safe: IDs start at 1 and increment
- Non-deterministic ordering is acceptable
- Relationships resolve via name+range lookup

### Incremental Mode (Adding Files)
- **BROKEN**: New files get IDs starting at 1, colliding with existing files
- Could cause data corruption in Tantivy
- Relationship resolution would fail or point to wrong symbols

### Force Re-index
- Safe: Entire index is cleared and rebuilt from scratch

## Recommended Fix

Modify `index_files()` to pass the `DocumentIndex` to `CollectStage` and query existing counters:

```rust
// In CollectStage, add a method like process_single has:
pub fn with_index_counters(mut self, index: &DocumentIndex) -> PipelineResult<Self> {
    let next_file_id = index.get_next_file_id()?;
    let next_symbol_id = index.get_next_symbol_id()?;
    self.state.file_counter = next_file_id.saturating_sub(1);
    self.state.symbol_counter = next_symbol_id.saturating_sub(1);
    Ok(self)
}
```

Or pass the Arc<DocumentIndex> to the COLLECT thread and have it query at startup.

## Conclusions

1. **File ordering is non-deterministic** by design - parallel processing optimizes throughput over ordering
2. **No language or directory grouping** - files from the same directory/language can be interleaved
3. **ID assignment is sequential** but the mapping of IDs to files varies per run
4. **CRITICAL BUG**: `index_files()` for incremental mode starts ID counters at 0 instead of querying existing max IDs
5. The bug likely went unnoticed because:
   - Fresh indexing (force=true) works correctly
   - Single-file updates via `process_single()` work correctly
   - Multi-file incremental adds may not have been tested extensively
