# Research Report: main.rs Indexing Orchestration

**Date**: 2026-01-01 12:00
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The `main.rs` file orchestrates indexing through a multi-phase startup sequence: Settings load, SimpleIndexer creation (lazy or eager), config path seeding, metadata-based sync detection, and command dispatch. The current architecture uses `SimpleIndexer` as the central indexing engine while `Pipeline` exists as an alternative (used by `index-parallel` command). Migrating to Pipeline requires replacing the sync logic and integrating Pipeline's incremental mode.

## Key Findings

### 1. SimpleIndexer Creation and Storage

SimpleIndexer is created in `main.rs` around lines 330-395 based on command requirements:

```rust
let mut indexer: Option<SimpleIndexer> = if !needs_indexer {
    None
} else {
    Some({
        if persistence.exists() && !force_recreate_index {
            // Lazy load from disk
            persistence.load_with_settings_lazy(settings.clone(), skip_trait_resolver)
        } else {
            // Create fresh indexer
            SimpleIndexer::with_settings_lazy(settings.clone())
        }
    })
};
```

**Key points:**
- Stored as `Option<SimpleIndexer>` to allow None for commands that don't need indexing
- Uses lazy initialization via `with_settings_lazy` for faster CLI startup
- Loaded from `IndexPersistence` when existing index found (unless force flag)
- Settings are shared via `Arc<Settings>`

**Evidence**: `/Users/bartolli/Projects/codanna/src/main.rs:330-395`

### 2. Sync Logic (Lines 471-541)

The sync mechanism compares metadata's stored paths with current config:

```rust
if let Some(ref mut idx) = indexer {
    if persistence.exists() && !is_force_index {
        match IndexMetadata::load(&config.index_path) {
            Ok(metadata) => {
                let stored_paths = metadata.indexed_paths.clone();

                match idx.sync_with_config(
                    stored_paths,
                    &config.indexing.indexed_paths,
                    show_progress,
                ) {
                    Ok((added, removed, files, symbols)) => {
                        if added > 0 || removed > 0 {
                            sync_made_changes = Some(true);
                            // ... display messages and save
                        } else {
                            sync_made_changes = Some(false);
                        }
                    }
                    // ... error handling
                }
            }
            // ... error handling
        }
    }
}
```

**Key points:**
- Only runs when index exists and force flag is false
- Loads stored paths from `IndexMetadata.indexed_paths`
- Compares with `config.indexing.indexed_paths` (from settings.toml)
- Returns tuple: `(added_count, removed_count, files_indexed, symbols_found)`
- Sets `sync_made_changes` to `Some(true/false)` or `None` (metadata unavailable)

**Evidence**: `/Users/bartolli/Projects/codanna/src/main.rs:471-541`

### 3. IndexMetadata Structure

Located in `src/storage/metadata.rs`:

```rust
pub struct IndexMetadata {
    pub version: u32,
    pub data_source: DataSource,
    pub symbol_count: u32,
    pub file_count: u32,
    pub last_modified: u64,
    pub indexed_paths: Option<Vec<PathBuf>>,  // Canonicalized paths
}
```

**Key methods:**
- `load(base_path)` - Loads from `{base_path}/index.meta`
- `save(base_path)` - Saves to `{base_path}/index.meta`
- `update_indexed_paths(paths)` - Updates stored paths

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/metadata.rs:10-30`

### 4. sync_made_changes Flow to Index Command

The flow passes through these stages:

1. **main.rs** sets `sync_made_changes` based on sync result:
   - `Some(true)` - sync added/removed directories
   - `Some(false)` - no changes needed
   - `None` - metadata unavailable

2. **Index command** receives it as parameter:
   ```rust
   Commands::Index { paths, force, ... } => {
       run_index(
           IndexArgs { ... },
           &mut config,
           indexer.as_mut().expect("index requires indexer"),
           &persistence,
           sync_made_changes,  // <-- passed here
       );
   }
   ```

3. **index.rs** uses it to decide behavior:
   ```rust
   if !force {
       match sync_made_changes {
           Some(false) => {
               println!("Index already up to date...");
               persistence.save(indexer)?;  // Just save
           }
           Some(true) => {
               // Sync already performed work
           }
           None => {
               println!("Skipping incremental update...");
           }
       }
       return;  // Skip re-indexing
   }
   ```

**Evidence**: `/Users/bartolli/Projects/codanna/src/main.rs:612-634` and `/Users/bartolli/Projects/codanna/src/cli/commands/index.rs:92-116`

### 5. What Would Need to Change for Pipeline

The `index-parallel` command already uses Pipeline but bypasses `main.rs` orchestration:

**Current Pipeline usage** (`src/cli/commands/index_parallel.rs`):
```rust
// Creates its own DocumentIndex
let index = Arc::new(DocumentIndex::new(&index_path, settings)?);

// Creates semantic search
let semantic = create_semantic_search(settings, &semantic_path);

// Creates pipeline directly
let pipeline = Pipeline::with_settings(settings_arc);

// Uses incremental indexing
pipeline.index_incremental(path, index, semantic, force)
```

**Changes needed to use Pipeline in main `index` command:**

1. **Replace SimpleIndexer with DocumentIndex + Pipeline:**
   - `SimpleIndexer` wraps Tantivy operations
   - `Pipeline` uses `DocumentIndex` (also wraps Tantivy)
   - Need to decide: replace SimpleIndexer entirely, or keep both?

2. **Replace sync_with_config:**
   - Current: `SimpleIndexer.sync_with_config(stored, config, progress)`
   - Pipeline equivalent: `Pipeline.index_incremental(path, index, semantic, false)`
   - Pipeline's incremental mode already detects new/modified/deleted files via hash comparison

3. **Metadata handling:**
   - Pipeline doesn't use `IndexMetadata` for path tracking
   - Would need to either:
     - Add path tracking to Pipeline/DocumentIndex
     - Keep IndexMetadata alongside DocumentIndex

4. **Semantic search integration:**
   - SimpleIndexer manages semantic search internally
   - Pipeline receives it as an external `Arc<Mutex<SimpleSemanticSearch>>`

5. **Persistence changes:**
   - `IndexPersistence.save(SimpleIndexer)` extracts data from SimpleIndexer
   - Would need `IndexPersistence.save_from_document_index(DocumentIndex)` or similar

## Architecture/Patterns Identified

### Current Flow (SimpleIndexer-based)
```
main.rs:
  Settings::load()
    -> IndexPersistence::load_with_settings_lazy()
      -> SimpleIndexer (with Tantivy)
    -> seed_indexer_with_config_paths()
    -> IndexMetadata::load()
    -> sync_with_config()
    -> Commands::Index dispatch
      -> index.rs::run()
        -> SimpleIndexer.index_directory()
        -> IndexPersistence.save()
```

### Pipeline Flow (index-parallel)
```
index_parallel.rs:
  Settings (passed in)
    -> DocumentIndex::new()
    -> SimpleSemanticSearch (optional)
    -> Pipeline::new()
    -> pipeline.index_incremental()
    -> (no explicit save - DocumentIndex commits internally)
```

### Key Difference
- SimpleIndexer: Single object manages Tantivy + symbols + semantic search
- Pipeline: Separated concerns - DocumentIndex (storage), Pipeline (orchestration), SemanticSearch (optional)

## Conclusions

1. **Migration path exists** but requires architectural decisions about state management

2. **sync_with_config equivalent** in Pipeline is `index_incremental` - it already handles change detection through file hash comparison

3. **Metadata tracking gap**: Pipeline does not persist `indexed_paths` for sync detection. Either:
   - Add this to DocumentIndex/Pipeline
   - Keep IndexMetadata as a separate concern

4. **Two options for integration:**
   - **Gradual**: Keep both paths, let users choose via command flag
   - **Full migration**: Replace SimpleIndexer internals with Pipeline, keep API surface

5. **index-parallel command** already demonstrates full Pipeline usage without SimpleIndexer - this is the reference implementation for migration
