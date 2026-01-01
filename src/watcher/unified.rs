//! Unified file watcher that routes events to pluggable handlers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, sleep};

use crate::documents::DocumentStore;
use crate::documents::config::ChunkingConfig;
use crate::indexing::facade::IndexFacade;
use crate::mcp::notifications::{FileChangeEvent, NotificationBroadcaster};

use super::debouncer::Debouncer;
use super::error::WatchError;
use super::handler::{WatchAction, WatchHandler};
use super::path_registry::PathRegistry;

/// Unified file watcher with pluggable handlers.
///
/// Provides a single `notify::RecommendedWatcher` that routes file events
/// to appropriate handlers based on path matching.
pub struct UnifiedWatcher {
    /// Registered handlers.
    handlers: Vec<Box<dyn WatchHandler>>,
    /// Path registry for tracking and directory computation.
    registry: PathRegistry,
    /// Shared debouncer for all file events.
    debouncer: Debouncer,
    /// Channel for receiving file events.
    event_rx: mpsc::Receiver<notify::Result<Event>>,
    /// The underlying file watcher.
    _watcher: notify::RecommendedWatcher,
    /// Notification broadcaster for MCP integration.
    broadcaster: Arc<NotificationBroadcaster>,
    /// Shared facade for executing code actions.
    facade: Arc<RwLock<IndexFacade>>,
    /// Document store for executing document actions (optional).
    document_store: Option<Arc<RwLock<DocumentStore>>>,
    /// Chunking config for document re-indexing.
    chunking_config: ChunkingConfig,
    /// Path for semantic search persistence.
    index_path: PathBuf,
    /// Workspace root for path resolution.
    workspace_root: PathBuf,
}

impl UnifiedWatcher {
    /// Create a builder for configuring the watcher.
    pub fn builder() -> UnifiedWatcherBuilder {
        UnifiedWatcherBuilder::new()
    }

    /// Start watching for file changes.
    ///
    /// This is the main event loop that:
    /// 1. Receives file events from notify
    /// 2. Debounces modification events
    /// 3. Routes events to matching handlers
    /// 4. Executes returned actions
    /// 5. Broadcasts notifications
    pub async fn watch(mut self) -> Result<(), WatchError> {
        // Initialize all handlers
        for handler in &self.handlers {
            if let Err(e) = handler.refresh_paths().await {
                tracing::warn!(
                    "[watcher] failed to initialize {} handler: {e}",
                    handler.name()
                );
            }
        }

        // Collect all paths from handlers and register them
        let mut all_paths = Vec::new();
        for handler in &self.handlers {
            all_paths.extend(handler.tracked_paths().await);
        }

        let new_dirs = self.registry.add_paths(all_paths);
        let total_paths = self.registry.path_count();
        let total_dirs = self.registry.dir_count();

        if total_paths == 0 {
            tracing::warn!("[watcher] no files to watch - index some files first");
        } else {
            crate::log_event!(
                "watcher",
                "monitoring",
                "{total_paths} files in {total_dirs} directories"
            );
        }

        // Watch all directories
        for dir in new_dirs {
            self.watch_directory(&dir)?;
        }

        // Subscribe to broadcaster for IndexReloaded events
        let mut broadcast_rx = self.broadcaster.subscribe();

        crate::log_event!("watcher", "started");

        loop {
            // Periodic check for debounced events
            let timeout = sleep(Duration::from_millis(100));
            tokio::pin!(timeout);

            tokio::select! {
                // Handle incoming file events
                Some(res) = self.event_rx.recv() => {
                    match res {
                        Ok(event) => {
                            self.handle_event(event).await;
                        }
                        Err(e) => {
                            tracing::error!("[watcher] file watch error: {e}");
                        }
                    }
                }

                // Process debounced changes
                _ = &mut timeout => {
                    let ready = self.debouncer.take_ready();
                    for path in ready {
                        self.process_modification(&path).await;
                    }
                }

                // Handle broadcast notifications
                Ok(event) = broadcast_rx.recv() => {
                    if matches!(event, FileChangeEvent::IndexReloaded) {
                        self.handle_index_reloaded().await;
                    }
                }
            }
        }
    }

    /// Watch a directory for changes.
    fn watch_directory(&mut self, dir: &PathBuf) -> Result<(), WatchError> {
        let watch_path = if dir.is_absolute() {
            dir.clone()
        } else {
            self.workspace_root.join(dir)
        };

        match self
            ._watcher
            .watch(&watch_path, RecursiveMode::NonRecursive)
        {
            Ok(_) => {
                crate::debug_event!("watcher", "watching", "{}", watch_path.display());
                Ok(())
            }
            Err(e) => {
                tracing::warn!("[watcher] failed to watch {}: {e}", watch_path.display());
                // Continue - don't fail completely
                Ok(())
            }
        }
    }

    /// Handle an incoming file event.
    async fn handle_event(&mut self, event: Event) {
        for path in event.paths {
            // Check if any handler cares about this path
            let matched = self.handlers.iter().any(|h| h.matches(&path));
            if !matched {
                crate::debug_event!(
                    "watcher",
                    "unmatched",
                    "{:?} {}",
                    event.kind,
                    path.display()
                );
                continue;
            }

            match event.kind {
                EventKind::Modify(_) => {
                    // Debounce modifications
                    self.debouncer.record(path);
                }
                EventKind::Remove(_) => {
                    // Handle deletions immediately
                    self.debouncer.remove(&path);
                    self.process_deletion(&path).await;
                }
                _ => {}
            }
        }
    }

    /// Process a debounced file modification.
    async fn process_modification(&self, path: &Path) {
        // Check if file still exists (handles rename-as-modify on macOS)
        if !path.exists() {
            self.process_deletion(path).await;
            return;
        }

        for handler in &self.handlers {
            if !handler.matches(path) {
                continue;
            }

            crate::log_event!(handler.name(), "modified", "{}", path.display());

            match handler.on_modify(path).await {
                Ok(action) => {
                    if let Err(e) = self.execute_action(action, handler.name()).await {
                        tracing::error!("[{}] action error: {e}", handler.name());
                    }
                }
                Err(e) => {
                    tracing::error!("[{}] handler error: {e}", handler.name());
                }
            }
        }
    }

    /// Process a file deletion.
    async fn process_deletion(&self, path: &Path) {
        for handler in &self.handlers {
            if !handler.matches(path) {
                continue;
            }

            crate::log_event!(handler.name(), "deleted", "{}", path.display());

            match handler.on_delete(path).await {
                Ok(action) => {
                    if let Err(e) = self.execute_action(action, handler.name()).await {
                        tracing::error!("[{}] action error: {e}", handler.name());
                    }
                }
                Err(e) => {
                    tracing::error!("[{}] handler error: {e}", handler.name());
                }
            }
        }
    }

    /// Execute an action returned by a handler.
    async fn execute_action(
        &self,
        action: WatchAction,
        handler_name: &str,
    ) -> Result<(), WatchError> {
        match action {
            WatchAction::ReindexCode { path } => {
                let mut indexer = self.facade.write().await;
                match indexer.index_file(&path) {
                    Ok(result) => {
                        use crate::IndexingResult;
                        match result {
                            IndexingResult::Indexed(_) => {
                                crate::log_event!(handler_name, "reindexed");

                                // Save semantic search
                                if indexer.has_semantic_search() {
                                    let semantic_path = self.index_path.join("semantic");
                                    if let Err(e) = indexer.save_semantic_search(&semantic_path) {
                                        tracing::warn!(
                                            "[{handler_name}] failed to save semantic search: {e}"
                                        );
                                    }
                                }

                                // Notify
                                self.broadcaster
                                    .send(FileChangeEvent::FileReindexed { path: path.clone() });
                            }
                            IndexingResult::Cached(_) => {
                                crate::debug_event!(handler_name, "unchanged (hash match)");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("[{handler_name}] reindex failed: {e}");
                    }
                }
            }

            WatchAction::RemoveCode { path } => {
                let mut indexer = self.facade.write().await;
                if let Err(e) = indexer.remove_file(&path) {
                    tracing::error!("[{handler_name}] failed to remove: {e}");
                } else {
                    crate::log_event!(handler_name, "removed");
                    self.broadcaster
                        .send(FileChangeEvent::FileDeleted { path: path.clone() });
                }
            }

            WatchAction::ReindexDocument { path } => {
                if let Some(ref store) = self.document_store {
                    let mut store = store.write().await;
                    match store.reindex_file(&path, &self.chunking_config) {
                        Ok(Some(chunks)) => {
                            crate::log_event!(handler_name, "reindexed", "{chunks} chunks");
                            self.broadcaster
                                .send(FileChangeEvent::FileReindexed { path: path.clone() });
                        }
                        Ok(None) => {
                            crate::debug_event!(handler_name, "not in index, skipped");
                        }
                        Err(e) => {
                            tracing::error!("[{handler_name}] reindex failed: {e}");
                        }
                    }
                }
            }

            WatchAction::RemoveDocument { path } => {
                if let Some(ref store) = self.document_store {
                    let mut store = store.write().await;
                    match store.remove_file(&path) {
                        Ok(true) => {
                            crate::log_event!(handler_name, "removed");
                            self.broadcaster
                                .send(FileChangeEvent::FileDeleted { path: path.clone() });
                        }
                        Ok(false) => {
                            crate::debug_event!(handler_name, "was not in index");
                        }
                        Err(e) => {
                            tracing::error!("[{handler_name}] failed to remove: {e}");
                        }
                    }
                }
            }

            WatchAction::ReloadConfig { added, removed } => {
                if !added.is_empty() {
                    crate::log_event!("config", "adding directories", "{}", added.len());
                    for path in &added {
                        tracing::info!("  + {}", path.display());
                    }

                    let mut indexer = self.facade.write().await;
                    for path in &added {
                        crate::log_event!("config", "indexing", "{}", path.display());
                        match indexer.index_directory(path, false) {
                            Ok(stats) => {
                                tracing::info!(
                                    "  indexed {} files, {} symbols",
                                    stats.files_indexed,
                                    stats.symbols_found
                                );
                            }
                            Err(e) => {
                                tracing::error!("  failed: {e}");
                            }
                        }
                    }
                }

                if !removed.is_empty() {
                    crate::log_event!("config", "removed directories", "{}", removed.len());
                    for path in &removed {
                        tracing::info!("  - {}", path.display());
                    }
                    tracing::info!("Run 'codanna clean' to remove symbols from these directories");
                }

                if !added.is_empty() || !removed.is_empty() {
                    self.broadcaster.send(FileChangeEvent::IndexReloaded);
                }
            }

            WatchAction::None => {
                crate::debug_event!(handler_name, "no action needed");
            }
        }

        Ok(())
    }

    /// Handle IndexReloaded notification - refresh all handlers.
    async fn handle_index_reloaded(&mut self) {
        crate::log_event!("watcher", "index reloaded, refreshing");

        for handler in &self.handlers {
            if let Err(e) = handler.refresh_paths().await {
                tracing::warn!(
                    "[watcher] failed to refresh {} handler: {e}",
                    handler.name()
                );
            }
        }

        // Rebuild path registry
        let mut all_paths = Vec::new();
        for handler in &self.handlers {
            all_paths.extend(handler.tracked_paths().await);
        }

        let old_dirs: HashSet<PathBuf> = self.registry.watch_dirs().clone();
        self.registry.rebuild(all_paths);

        // Collect new directories before mutably borrowing self
        let dirs_to_watch: Vec<PathBuf> = self
            .registry
            .watch_dirs()
            .difference(&old_dirs)
            .cloned()
            .collect();

        // Watch any new directories
        for dir in dirs_to_watch {
            if let Err(e) = self.watch_directory(&dir) {
                tracing::warn!("[watcher] failed to watch new directory: {e}");
            }
        }

        crate::log_event!(
            "watcher",
            "watching",
            "{} files in {} directories",
            self.registry.path_count(),
            self.registry.dir_count()
        );
    }
}

/// Builder for constructing a UnifiedWatcher.
pub struct UnifiedWatcherBuilder {
    handlers: Vec<Box<dyn WatchHandler>>,
    broadcaster: Option<Arc<NotificationBroadcaster>>,
    facade: Option<Arc<RwLock<IndexFacade>>>,
    document_store: Option<Arc<RwLock<DocumentStore>>>,
    chunking_config: ChunkingConfig,
    index_path: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    debounce_ms: u64,
}

impl UnifiedWatcherBuilder {
    /// Create a new builder with defaults.
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            broadcaster: None,
            facade: None,
            document_store: None,
            chunking_config: ChunkingConfig::default(),
            index_path: None,
            workspace_root: None,
            debounce_ms: 500,
        }
    }

    /// Add a handler.
    pub fn handler(mut self, handler: impl WatchHandler + 'static) -> Self {
        self.handlers.push(Box::new(handler));
        self
    }

    /// Set the notification broadcaster.
    pub fn broadcaster(mut self, broadcaster: Arc<NotificationBroadcaster>) -> Self {
        self.broadcaster = Some(broadcaster);
        self
    }

    /// Set the facade (renamed from indexer).
    pub fn indexer(mut self, facade: Arc<RwLock<IndexFacade>>) -> Self {
        self.facade = Some(facade);
        self
    }

    /// Set the document store.
    pub fn document_store(mut self, store: Arc<RwLock<DocumentStore>>) -> Self {
        self.document_store = Some(store);
        self
    }

    /// Set the chunking config for documents.
    pub fn chunking_config(mut self, config: ChunkingConfig) -> Self {
        self.chunking_config = config;
        self
    }

    /// Set the index path for semantic search persistence.
    pub fn index_path(mut self, path: PathBuf) -> Self {
        self.index_path = Some(path);
        self
    }

    /// Set the workspace root.
    pub fn workspace_root(mut self, path: PathBuf) -> Self {
        self.workspace_root = Some(path);
        self
    }

    /// Set the debounce duration in milliseconds.
    pub fn debounce_ms(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    /// Build the UnifiedWatcher.
    pub fn build(self) -> Result<UnifiedWatcher, WatchError> {
        let broadcaster = self.broadcaster.ok_or_else(|| WatchError::InitFailed {
            reason: "Broadcaster is required".to_string(),
        })?;

        let facade = self.facade.ok_or_else(|| WatchError::InitFailed {
            reason: "Facade is required".to_string(),
        })?;

        let workspace_root = self
            .workspace_root
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let index_path = self
            .index_path
            .unwrap_or_else(|| workspace_root.join(".codanna/index"));

        // Create channel for events
        let (tx, rx) = mpsc::channel(100);

        // Create the notify watcher
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let _ = tx.blocking_send(res);
        })?;

        Ok(UnifiedWatcher {
            handlers: self.handlers,
            registry: PathRegistry::new(),
            debouncer: Debouncer::new(self.debounce_ms),
            event_rx: rx,
            _watcher: watcher,
            broadcaster,
            facade,
            document_store: self.document_store,
            chunking_config: self.chunking_config,
            index_path,
            workspace_root,
        })
    }
}

impl Default for UnifiedWatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}
