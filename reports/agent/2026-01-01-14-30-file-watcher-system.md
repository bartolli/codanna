# Research Report: File Watcher System

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The file watcher system consists of two independent watchers: `UnifiedWatcher` for real-time file change detection, and `HotReloadWatcher` for polling external index changes. Both integrate with `SimpleIndexer` via `Arc<RwLock<SimpleIndexer>>` and broadcast events through `NotificationBroadcaster`.

## Key Findings

### 1. UnifiedWatcher Architecture

The `UnifiedWatcher` is a pluggable handler system that routes file events to specialized handlers based on path matching.

**Evidence**: `/Users/bartolli/Projects/codanna/src/watcher/unified.rs:25-48`

```rust
pub struct UnifiedWatcher {
    handlers: Vec<Box<dyn WatchHandler>>,
    registry: PathRegistry,
    debouncer: Debouncer,
    event_rx: mpsc::Receiver<notify::Result<Event>>,
    _watcher: notify::RecommendedWatcher,
    broadcaster: Arc<NotificationBroadcaster>,
    indexer: Arc<RwLock<SimpleIndexer>>,
    document_store: Option<Arc<RwLock<DocumentStore>>>,
    // ...
}
```

**Event Flow:**
1. `notify::RecommendedWatcher` detects file changes
2. Events sent via `mpsc::channel` to `UnifiedWatcher`
3. `handle_event()` routes to matching handlers
4. Modifications are debounced; deletions are immediate
5. Handlers return `WatchAction` enums
6. `execute_action()` calls indexer methods

### 2. Handler Types and Actions

Three handler types registered at startup:

| Handler | File | Matches | Actions |
|---------|------|---------|---------|
| `CodeFileHandler` | `handlers/code.rs` | Indexed source files | `ReindexCode`, `RemoveCode` |
| `DocumentFileHandler` | `handlers/document.rs` | Indexed documents | `ReindexDocument`, `RemoveDocument` |
| `ConfigFileHandler` | `handlers/config.rs` | `settings.toml` | `ReloadConfig` |

**Evidence**: `/Users/bartolli/Projects/codanna/src/watcher/handler.rs:9-32`

```rust
pub enum WatchAction {
    ReindexCode { path: PathBuf },
    ReindexDocument { path: PathBuf },
    RemoveCode { path: PathBuf },
    RemoveDocument { path: PathBuf },
    ReloadConfig { added: Vec<PathBuf>, removed: Vec<PathBuf> },
    None,
}
```

### 3. SimpleIndexer Integration Points

When `WatchAction::ReindexCode` is executed:

**Evidence**: `/Users/bartolli/Projects/codanna/src/watcher/unified.rs:247-280`

```rust
WatchAction::ReindexCode { path } => {
    let mut indexer = self.indexer.write().await;
    match indexer.index_file(&path) {
        Ok(IndexingResult::Indexed(_)) => {
            // Save semantic search
            if indexer.has_semantic_search() {
                indexer.save_semantic_search(&semantic_path)?;
            }
            // Broadcast notification
            self.broadcaster.send(FileChangeEvent::FileReindexed { path });
        }
        Ok(IndexingResult::Cached(_)) => { /* hash unchanged, skip */ }
        Err(e) => { /* log error */ }
    }
}
```

When `WatchAction::RemoveCode` is executed:

```rust
WatchAction::RemoveCode { path } => {
    let mut indexer = self.indexer.write().await;
    indexer.remove_file(&path)?;
    self.broadcaster.send(FileChangeEvent::FileDeleted { path });
}
```

**Key indexer methods called:**
- `index_file(&path)` - Re-index a single file
- `remove_file(&path)` - Remove file from index
- `index_directory(&path, false, false)` - For config changes adding directories
- `save_semantic_search(&path)` - Persist embeddings after reindex
- `get_all_indexed_paths()` - Used by handlers to initialize cache

### 4. Debouncer Implementation

Prevents excessive re-indexing during rapid saves.

**Evidence**: `/Users/bartolli/Projects/codanna/src/watcher/debouncer.rs:17-37`

```rust
pub struct Debouncer {
    pending: HashMap<PathBuf, Instant>,
    duration: Duration,
}

impl Debouncer {
    pub fn record(&mut self, path: PathBuf) {
        self.pending.insert(path, Instant::now());
    }

    pub fn take_ready(&mut self) -> Vec<PathBuf> {
        // Returns paths stable for >= duration
    }
}
```

- Default debounce: 500ms (configurable via `debounce_ms`)
- Checked every 100ms in the main loop
- Deletions bypass debouncing (processed immediately)

### 5. Hot Reload Watcher

Separate watcher that polls for external index changes (e.g., CI/CD, other terminals).

**Evidence**: `/Users/bartolli/Projects/codanna/src/watcher/hot_reload.rs:16-38`

```rust
pub struct HotReloadWatcher {
    index_path: PathBuf,
    indexer: Arc<RwLock<SimpleIndexer>>,
    settings: Arc<Settings>,
    persistence: IndexPersistence,
    last_modified: Option<SystemTime>,
    last_doc_modified: Option<SystemTime>,
    check_interval: Duration,
    broadcaster: Option<Arc<NotificationBroadcaster>>,
}
```

**Polling mechanism:**
- Checks `meta.json` modification time every N seconds (default: 5s)
- If changed, loads new index via `IndexPersistence::load_with_settings()`
- Replaces indexer contents: `*indexer_guard = new_indexer`
- Restores semantic search from `semantic/metadata.json`
- Broadcasts `FileChangeEvent::IndexReloaded`

### 6. Index Reload Handler Refresh

When `IndexReloaded` is broadcast, `UnifiedWatcher` refreshes all handlers:

**Evidence**: `/Users/bartolli/Projects/codanna/src/watcher/unified.rs:383-416`

```rust
async fn handle_index_reloaded(&mut self) {
    // Refresh all handlers' path caches
    for handler in &self.handlers {
        handler.refresh_paths().await?;
    }

    // Rebuild path registry and watch new directories
    self.registry.rebuild(all_paths);
    for dir in new_directories {
        self.watch_directory(&dir)?;
    }
}
```

### 7. Server Integration

Both watchers are spawned as tokio tasks in the MCP server.

**Evidence**: `/Users/bartolli/Projects/codanna/src/mcp/http_server.rs:67-170`

```rust
// Hot reload watcher (polls for external changes)
let hot_reload_watcher = HotReloadWatcher::new(
    indexer.clone(),
    settings.clone(),
    Duration::from_secs(5),
).with_broadcaster(broadcaster.clone());
tokio::spawn(hot_reload_watcher.watch());

// Unified watcher (real-time file changes)
let builder = UnifiedWatcher::builder()
    .broadcaster(broadcaster.clone())
    .indexer(indexer.clone())
    .handler(CodeFileHandler::new(indexer.clone(), workspace_root.clone()))
    .handler(ConfigFileHandler::new(settings_path)?)
    .handler(DocumentFileHandler::new(doc_store, workspace_root));
tokio::spawn(unified_watcher.watch());
```

## Architecture Diagram

```
+------------------+     +-------------------+
| notify::Watcher  |     | HotReloadWatcher  |
| (file system)    |     | (polls meta.json) |
+--------+---------+     +---------+---------+
         |                         |
         v                         v
+--------+---------+     +---------+---------+
|  UnifiedWatcher  |     | IndexPersistence  |
|  - debouncer     |     |   load_with_      |
|  - handlers[]    |     |   settings()      |
+--------+---------+     +---------+---------+
         |                         |
         v                         |
+--------+---------+               |
| WatchHandler     |               |
| - CodeFile       |               |
| - DocumentFile   |               |
| - ConfigFile     |               |
+--------+---------+               |
         |                         |
         v                         v
+--------+-------------------------+---------+
|           Arc<RwLock<SimpleIndexer>>       |
|  - index_file()                            |
|  - remove_file()                           |
|  - index_directory()                       |
|  - save_semantic_search()                  |
+--------------------+-----------------------+
                     |
                     v
+--------------------+-----------------------+
|         NotificationBroadcaster            |
|  - FileReindexed { path }                  |
|  - FileDeleted { path }                    |
|  - IndexReloaded                           |
+--------------------------------------------+
```

## Key Files

| File | Purpose |
|------|---------|
| `src/watcher/unified.rs` | Main watcher with event loop and action execution |
| `src/watcher/handler.rs` | `WatchHandler` trait and `WatchAction` enum |
| `src/watcher/handlers/code.rs` | Code file handler with path cache |
| `src/watcher/handlers/document.rs` | Document file handler |
| `src/watcher/handlers/config.rs` | Config file handler with diff detection |
| `src/watcher/debouncer.rs` | Time-based event debouncing |
| `src/watcher/hot_reload.rs` | External index change polling |
| `src/watcher/path_registry.rs` | Path interning and directory computation |

## Conclusions

1. **Dual-watcher design**: Real-time file watching (UnifiedWatcher) is separate from external index polling (HotReloadWatcher), allowing both local edits and CI/CD updates to trigger reindexing.

2. **Handler abstraction**: The `WatchHandler` trait enables clean separation between path matching and action execution. Adding new file types requires only implementing the trait.

3. **Thread-safe access**: `Arc<RwLock<SimpleIndexer>>` ensures safe concurrent access. Write locks are acquired only during actual indexing operations.

4. **Event broadcasting**: `NotificationBroadcaster` decouples watchers from MCP notification delivery, allowing multiple subscribers.

5. **Optimization via caching**: Handlers cache indexed paths in `HashSet` for O(1) `matches()` lookup instead of querying the indexer on every event.
