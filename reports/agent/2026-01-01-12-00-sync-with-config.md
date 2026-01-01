# Research Report: SimpleIndexer::sync_with_config

**Date**: 2026-01-01
**Agent**: Research-Agent
**Model**: Opus 4.5

## Summary

The `sync_with_config` method synchronizes the indexer state with the configuration file (settings.toml). It compares stored indexed paths against current config paths, indexes new directories, and removes files from directories no longer in config.

## Method Signature

**Location**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2135-2285`

```rust
pub fn sync_with_config(
    &mut self,
    stored_paths: Option<Vec<PathBuf>>,
    config_paths: &[PathBuf],
    progress: bool,
) -> IndexResult<(usize, usize, usize, usize)>
```

### Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `stored_paths` | `Option<Vec<PathBuf>>` | Previously indexed directory paths (from IndexMetadata) |
| `config_paths` | `&[PathBuf]` | Current directory paths from settings.toml |
| `progress` | `bool` | Whether to show progress bar during operations |

### Return Value

Returns `IndexResult<(usize, usize, usize, usize)>` where the tuple contains:

| Index | Field | Description |
|-------|-------|-------------|
| 0 | `added_count` | Number of new directories indexed |
| 1 | `removed_count` | Number of directories removed from index |
| 2 | `files_indexed` | Total files indexed from new directories |
| 3 | `symbols_found` | Total symbols found in new directories |

## Algorithm

### Step 1: Path Comparison (lines 2147-2162)

Both path sets are canonicalized and converted to `HashSet<PathBuf>`:

```rust
let stored_set: HashSet<PathBuf> = stored_paths
    .unwrap_or_default()
    .into_iter()
    .filter_map(|p| p.canonicalize().ok())
    .collect();

let config_set: HashSet<PathBuf> = config_paths
    .iter()
    .filter_map(|p| p.canonicalize().ok())
    .collect();
```

Set difference operations identify:
- `new_paths`: Directories in config but not stored (to be indexed)
- `removed_paths`: Directories stored but not in config (to be removed)

### Step 2: Index New Directories (lines 2175-2205)

For each new directory:

1. Calls `index_directory(path, progress, false)` to index all files
2. Accumulates `files_indexed` and `symbols_found` from returned `IndexStats`
3. Calls `add_indexed_path(path)` to track the directory

### Step 3: Remove Deleted Directories (lines 2207-2278)

1. Gets all indexed file paths via `get_all_indexed_paths()`
2. Collects files whose canonical path starts with any removed directory
3. For each file to remove:
   - Calls `remove_file(file_path)` which:
     - Deletes imports for the file
     - Removes all Tantivy documents
     - Removes embeddings if semantic search enabled
     - Rebuilds symbol cache
4. Removes paths from `indexed_paths` HashSet

## Internal Method Calls

### `index_directory` (symbol_id:3020)

**Location**: `src/indexing/simple.rs:2423-2430`

```rust
pub fn index_directory(
    &mut self,
    dir: impl AsRef<Path>,
    progress: bool,
    dry_run: bool,
) -> IndexResult<IndexStats>
```

Delegates to `index_directory_with_options`. Uses `FileWalker` to discover files and indexes each one.

### `remove_file` (symbol_id:2963)

**Location**: `src/indexing/simple.rs:564-656`

```rust
pub fn remove_file(&mut self, path: impl AsRef<Path>) -> IndexResult<()>
```

Removes a file and all its symbols from the index:
1. Gets FileId from Tantivy
2. Finds all symbols for the file
3. Deletes imports for the file
4. Removes all Tantivy documents
5. Removes embeddings (if semantic search enabled)
6. Commits batch
7. Rebuilds symbol cache

### `add_indexed_path` (symbol_id:3010)

**Location**: `src/indexing/simple.rs:2100-2121`

```rust
pub fn add_indexed_path(&mut self, dir_path: &Path) -> IndexResult<()>
```

Tracks a directory as indexed:
- Canonicalizes the path
- Skips if already covered by an existing parent directory
- Removes any descendant paths (child directories)
- Inserts into `indexed_paths` HashSet

### `get_all_indexed_paths` (symbol_id:3009)

**Location**: `src/indexing/simple.rs:2090-2097`

```rust
pub fn get_all_indexed_paths(&self) -> Vec<PathBuf>
```

Returns all indexed file paths from the document index. Used to find files that need removal.

## Call Site

**Location**: `/Users/bartolli/Projects/codanna/src/main.rs:479-508`

Called during startup when:
1. Index persistence exists
2. Not a force re-index operation

Uses `IndexMetadata::load()` to get stored paths, then compares with `config.indexing.indexed_paths`.

## Key Implementation Details

1. **Canonicalization**: All paths are canonicalized before comparison to handle symlinks and relative paths
2. **Progress Display**: Optional progress bar shown during file removal when more than 1 file
3. **Error Handling**: Continues processing even if individual files fail to index/remove
4. **Path Tracking**: Uses `indexed_paths` HashSet on the indexer struct to track directories
5. **Batch Operations**: File removal uses batch/commit pattern for Tantivy writes

## Return Semantics

```rust
// Early return if no changes needed
if new_paths.is_empty() && removed_paths.is_empty() {
    return Ok((0, 0, 0, 0));
}

// Final return with all counts
Ok((
    new_paths.len(),      // directories added
    removed_paths.len(),  // directories removed
    total_files,          // files indexed
    total_symbols,        // symbols found
))
```

The `removed_count` refers to directories removed, not individual files. The `files_indexed` and `symbols_found` only count additions (new directories), not removals.
