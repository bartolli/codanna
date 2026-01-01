# Research Report: MCP Server and Indexer API Dependencies

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The MCP server uses `Arc<RwLock<SimpleIndexer>>` for thread-safe shared access to the code index. Tools acquire read locks for queries and write locks for modifications. File change notifications flow through a broadcast channel system that connects file watchers to MCP clients.

## Key Findings

### 1. State Management Pattern

The `CodeIntelligenceServer` struct holds the indexer reference:

```rust
pub struct CodeIntelligenceServer {
    pub indexer: Arc<RwLock<SimpleIndexer>>,
    pub document_store: Option<Arc<RwLock<DocumentStore>>>,
    tool_router: ToolRouter<Self>,
    peer: Arc<Mutex<Option<Peer<RoleServer>>>>,
}
```

**Evidence**: `src/mcp/mod.rs:190-196`

Three constructor patterns exist:
- `new(indexer: SimpleIndexer)` - Takes ownership, wraps in Arc/RwLock
- `from_indexer(indexer: Arc<RwLock<SimpleIndexer>>)` - Uses existing Arc
- `new_with_indexer(indexer: Arc<RwLock<SimpleIndexer>>, settings: Arc<Settings>)` - For HTTP servers

**Evidence**: `src/mcp/mod.rs:198-227`

### 2. HTTP/HTTPS Server Setup

Both servers follow the same pattern:

1. Create shared indexer: `Arc::new(RwLock::new(SimpleIndexer::with_settings(...)))`
2. Load existing index via `IndexPersistence::load_with_settings()`
3. Replace indexer contents with loaded data
4. Pass `Arc<RwLock<SimpleIndexer>>` to `CodeIntelligenceServer::new_with_indexer()`

**Evidence**:
- `src/mcp/http_server.rs:25-45`
- `src/mcp/https_server.rs:27-47`

### 3. Notification System Architecture

The `NotificationBroadcaster` uses tokio broadcast channels:

```rust
pub enum FileChangeEvent {
    FileReindexed { path: PathBuf },
    FileCreated { path: PathBuf },
    FileDeleted { path: PathBuf },
    IndexReloaded,
}

pub struct NotificationBroadcaster {
    sender: broadcast::Sender<FileChangeEvent>,
}
```

**Evidence**: `src/mcp/notifications.rs:1-30`

Events are forwarded to MCP clients via three notification types:
- `notify_resource_updated` - Standard MCP resource update
- `notify_logging_message` - Logging for visibility
- `CustomNotification` - Custom codanna-specific events

**Evidence**: `src/mcp/notifications.rs:52-150`

### 4. MCP Tool API Calls on SimpleIndexer

**Read-only tools** (acquire `read().await`):

| Tool | SimpleIndexer Methods Called |
|------|------------------------------|
| `find_symbol` | `find_symbols_by_name()`, `get_symbol()`, `get_symbol_context()` |
| `get_calls` | `get_symbol()`, `find_symbols_by_name()`, `get_called_functions_with_metadata()` |
| `find_callers` | `get_symbol()`, `find_symbols_by_name()`, `get_calling_functions_with_metadata()` |
| `analyze_impact` | `get_symbol()`, `find_symbols_by_name()`, `get_impact_radius()`, `get_symbol_context()` |
| `get_index_info` | `symbol_count()`, `file_count()`, `relationship_count()`, `get_all_symbols()`, `get_semantic_metadata()` |
| `semantic_search_docs` | `has_semantic_search()`, `semantic_search_docs_with_language()`, `semantic_search_docs_with_threshold_and_language()` |
| `semantic_search_with_context` | Same as above plus `get_called_functions_with_metadata()`, `get_calling_functions()`, `get_extends()`, `get_extended_by()`, `get_implemented_traits()`, `get_implementations()`, `get_uses()`, `get_used_by()` |
| `search_symbols` | `search()` |
| `search_documents` | Uses `DocumentStore.search()` (separate store) |

**Evidence**: `src/mcp/mod.rs:265-1860`

**Write operations** (acquire `write().await`):

The `handle_force_reindex` custom request uses:
- `indexer.index_file()` for specific paths
- `indexer.index_directory()` for directory reindex

**Evidence**: `src/mcp/mod.rs:1915-1950`

### 5. File Watcher Integration

Two watcher systems exist:

**HotReloadWatcher** (polls for external index changes):
- Monitors `meta.json` and `state.json` modification times
- On change: `persistence.load_with_settings()` then replaces indexer via `write().await`
- After reload: Calls `indexer.load_semantic_search()` to restore semantic search
- Sends `FileChangeEvent::IndexReloaded` via broadcaster

**Evidence**: `src/watcher/hot_reload.rs:15-193`

**UnifiedWatcher** (handles live file changes):
- Receives `WatchAction` from handlers (CodeFileHandler, DocumentFileHandler, ConfigFileHandler)
- For `WatchAction::ReindexCode`: Calls `indexer.index_file()` with write lock
- For `WatchAction::RemoveCode`: Calls `indexer.remove_file()` with write lock
- Sends `FileChangeEvent::FileReindexed` or `FileChangeEvent::FileDeleted`

**Evidence**: `src/watcher/unified.rs:250-340`

### 6. Locking Patterns

| Operation | Lock Type | Held Duration |
|-----------|-----------|---------------|
| Tool queries | `read().await` | Duration of query execution |
| File reindex | `write().await` | Duration of single file indexing |
| Index reload | `write().await` | Duration of full index load |
| Directory index | `write().await` | Duration of directory walk + all file indexing |

Potential contention points:
- Full index reload blocks all queries during load
- Directory indexing (from config changes) can block for extended periods

## Architecture Diagram

```
                                   +-------------------+
                                   |  NotificationBroadcaster |
                                   |  (tokio::broadcast)       |
                                   +----------+--------+
                                              |
            +--------------------------------+|+---------------------------+
            |                                 |                            |
            v                                 v                            v
+----------------------+       +------------------------+    +------------------------+
| HotReloadWatcher    |       | UnifiedWatcher         |    | CodeIntelligenceServer |
| (polls meta.json)   |       | (notify::Watcher)      |    | (MCP tools)            |
+----------+-----------+       +-----------+------------+    +-----------+------------+
           |                               |                             |
           | write().await                 | write().await               | read().await
           |                               |                             |
           +---------------+---------------+-----------------------------+
                           |
                           v
             +---------------------------+
             | Arc<RwLock<SimpleIndexer>>|
             +---------------------------+
                           |
                           v
             +---------------------------+
             | DocumentIndex (Tantivy)   |
             | SemanticSearch            |
             | RelationshipGraph         |
             +---------------------------+
```

## Conclusions

1. **Thread Safety**: The `Arc<RwLock<SimpleIndexer>>` pattern provides correct concurrent access. Read-heavy MCP queries can proceed in parallel.

2. **Notification Flow**: File changes propagate correctly through the broadcast channel to all connected MCP clients.

3. **Potential Bottleneck**: Full index reloads and directory indexing acquire exclusive write locks, blocking all queries. For large codebases, this could cause noticeable latency spikes.

4. **API Surface**: MCP tools depend on ~25 distinct SimpleIndexer methods. Any changes to these methods must maintain backward compatibility or update all calling tools.

5. **Document Store**: Uses a separate `Arc<RwLock<DocumentStore>>` to avoid coupling document search with code index locking.
