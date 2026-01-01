# Research Report: Incremental Indexing and Change Detection

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Sonnet 4.5

## Summary

The system uses content hash comparison to detect file changes and implements a delete-all-for-file then re-index strategy. Incoming relationships are preserved across reindexes through a capture-restore mechanism. Symbol cache is rebuilt after every modification.

## Key Findings

### 1. Change Detection Mechanism

Content hash comparison using SHA-256. The hash is calculated when reading the file and compared against the stored hash in Tantivy.

**Location**: `src/indexing/simple.rs:500-510`

```rust
// Check if file already exists by querying Tantivy
if let Ok(Some((file_id, existing_hash))) = self.document_index.get_file_info(path_str) {
    if !force && existing_hash == content_hash {
        // File hasn't changed, skip re-indexing
        return Ok(crate::IndexingResult::Cached(file_id));
    }
    // File has changed or force re-indexing
```

**Hash computation**: `src/indexing/simple.rs:659-672` - Uses `calculate_hash()` on file content after lossy UTF-8 conversion.

### 2. Update/Delete Flow

The flow is: **detect change -> capture incoming relationships -> delete all for file -> index new content -> restore relationships -> rebuild cache**

#### Step-by-step for file modification:

1. **Read and hash file** (`read_file_with_hash`)
2. **Compare hash** - if unchanged, return `IndexingResult::Cached`
3. **Capture incoming relationships** before deletion (`capture_incoming_relationships`)
   - Stores from_id, qualified_name, kind, metadata for each incoming relationship
   - **Evidence**: `src/indexing/simple.rs:3161-3192`
4. **Delete ALL documents for file path** via `remove_file_documents`
   - Uses Tantivy term deletion on file_path field
   - **Evidence**: `src/storage/tantivy.rs:1043-1062`
5. **Remove semantic embeddings** for old symbol IDs
   - **Evidence**: `src/indexing/simple.rs:518-532`
6. **Register new file** with new hash
7. **Index content** (`reindex_file_content`)
8. **Commit batch**
9. **Resolve cross-file relationships** - includes restoring captured incoming relationships
   - **Evidence**: `src/indexing/simple.rs:3220-3259`
10. **Rebuild symbol cache** (`build_symbol_cache`)

#### For file deletion:

1. **Get file info** and all symbols for file
2. **Start batch**
3. **Delete imports** for file (`delete_imports_for_file`)
4. **Remove file documents** from Tantivy
5. **Remove embeddings** for symbols
6. **Commit batch**
7. **Rebuild symbol cache**

**Evidence**: `src/indexing/simple.rs:564-656`

### 3. Relationship Invalidation

Relationships are deleted when the file's documents are removed via `remove_file_documents` because relationships are stored as Tantivy documents with a `file_path` field.

However, **incoming relationships from OTHER files** are NOT automatically invalidated when a symbol is deleted. This is handled by:

1. **Capture before delete**: `capture_incoming_relationships` saves relationships where the file's symbols are the `to_id`
2. **Restore after reindex**: `restore_incoming_relationships` uses qualified name matching to find the new symbol IDs and recreates the relationships

**Key detail**: The delete uses `delete_relationships_for_symbol` which deletes both `from_symbol_id = id` AND `to_symbol_id = id` relationships:

```rust
// Delete where from_symbol_id = id
let from_term = Term::from_field_u64(self.schema.from_symbol_id, id.0 as u64);
writer.delete_term(from_term);

// Delete where to_symbol_id = id
let to_term = Term::from_field_u64(self.schema.to_symbol_id, id.0 as u64);
writer.delete_term(to_term);
```

**Evidence**: `src/storage/tantivy.rs:1855-1874`

### 4. Cache Invalidation Strategy

**ConcurrentSymbolCache** is rebuilt completely after:
- File removal (`remove_file`)
- Batch commit (`commit_tantivy_batch`)
- Manual rebuild command

The cache is a memory-mapped file that provides O(1) lookup by symbol name. Rebuilding:
1. Clears existing cache (`clear_symbol_cache`)
2. Gets all symbols from index
3. Builds new cache file
4. Memory-maps it for use

**Evidence**: `src/indexing/simple.rs:3315-3362`

**No incremental cache updates** - the entire cache is rebuilt from scratch each time.

### 5. Watch Flow for Live Changes

File watcher uses debounced modification events:

1. `UnifiedWatcher` receives file events
2. Debounces modifications, processes deletions immediately
3. Routes to `CodeFileHandler` which returns `WatchAction::ReindexCode` or `WatchAction::RemoveCode`
4. `execute_action` calls `indexer.index_file()` or `indexer.remove_file()`
5. Broadcasts `FileChangeEvent` for notification

**Evidence**: `src/watcher/unified.rs:165-380`

## Architecture/Patterns Identified

### Delete-All-Then-Reindex Pattern

The system does NOT attempt incremental symbol updates. When a file changes:
- All symbols for that file are deleted
- File is re-parsed from scratch
- New symbols are created with potentially new IDs

This simplifies consistency but means symbol IDs are not stable across edits.

### Relationship Preservation via Qualified Names

Instead of trying to track symbol ID changes, the system:
- Captures incoming relationships using qualified names (`module::name` format)
- Restores them by matching qualified names to new symbols
- Handles renames gracefully (relationship lost if name changes)

### Tantivy as Single Source of Truth

The symbol cache is derived from Tantivy, not maintained separately. This ensures consistency but requires full rebuilds.

## Conclusions

1. **Change detection is efficient** - SHA-256 hash comparison avoids re-indexing unchanged files
2. **Update flow is complete delete + reindex** - no incremental symbol updates
3. **Relationships are preserved** across reindexes via qualified name matching
4. **Cache invalidation is expensive** - full rebuild on every change
5. **Potential optimization**: Incremental cache updates could improve performance for large codebases with frequent changes

### Key Questions Answered

- **Is it delete-all-for-file then re-index?** YES
- **Are relationships automatically invalidated when to_id symbol is deleted?** YES, via Tantivy term deletion on both from_id and to_id
- **What's the update flow?** detect change -> capture incoming rels -> delete old -> index new -> restore rels -> rebuild cache
