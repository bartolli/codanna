//! Unified file watcher that routes events to pluggable handlers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::{RwLock, mpsc};
use tokio::time::Duration;

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
    /// Registered watch roots from handlers; scopes created-directory
    /// handling and stays watched even when a root holds no indexed
    /// file directly.
    handler_roots: Vec<PathBuf>,
    /// Roots whose owning handler is covered by the batch incremental
    /// lane. Removal waves batch-sync these so the shared discovery can
    /// pair renames (remove + create of identical content).
    batch_sync_roots: Vec<PathBuf>,
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

        self.register_handler_roots().await;

        // Subscribe to broadcaster for IndexReloaded events
        let mut broadcast_rx = self.broadcaster.subscribe();

        crate::log_event!("watcher", "started");

        // The drain fires on a fixed cadence, never deferred by event
        // pressure: a per-iteration sleep resets on every received
        // event, so any sustained stream with sub-interval arrivals
        // starves the drain -- and with it every debounced reindex,
        // removal wave, and notification -- for as long as the stream
        // lasts. The debouncer's own per-path quiet windows decide
        // what each tick actually drains.
        let mut drain = tokio::time::interval(Duration::from_millis(100));
        drain.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
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
                _ = drain.tick() => {
                    if self.debouncer.has_pending_removals() {
                        // A removal may be one side of a rename. Hold the
                        // whole burst until every side is stable, then hand
                        // remove + create to the shared batch lane in one
                        // wave so discovery can pair them.
                        if let Some((removed, modified)) = self.debouncer.take_settled_burst() {
                            self.process_removal_wave(removed, modified).await;
                        }
                    } else {
                        let ready = self.debouncer.take_ready();
                        let (vanished, alive): (Vec<PathBuf>, Vec<PathBuf>) =
                            ready.into_iter().partition(|path| !path.exists());
                        if vanished.is_empty() {
                            for path in alive {
                                self.process_modification(&path).await;
                            }
                        } else {
                            // rename-as-modify (macOS): vanished paths are
                            // removal observations, and the survivors of the
                            // same batch must ride the same wave -- indexing
                            // a rename's create side per-file here would
                            // leave discovery nothing to pair.
                            for path in vanished {
                                self.debouncer.record_removal(path);
                            }
                            for path in alive {
                                self.debouncer.record(path);
                            }
                        }
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
                crate::debug_event!(
                    "watcher",
                    "watching",
                    "{}",
                    crate::parsing::paths::render_absolute_path(&watch_path).display()
                );
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    "[watcher] failed to watch {}: {e}",
                    crate::parsing::paths::render_absolute_path(&watch_path).display()
                );
                // Continue - don't fail completely
                Ok(())
            }
        }
    }

    /// Handle an incoming file event.
    async fn handle_event(&mut self, event: Event) {
        // Access events observe state; they never change it. inotify
        // emits Access(Open) for every directory read -- including the
        // watcher's OWN catch-up walks -- so routing them into the
        // directory branch below livelocks: walk emits Open, Open
        // triggers walk. FSEvents emits no Access events, which is why
        // only Linux exhibits it. The file-level kind match already
        // discards Access; state-bearing kinds are untouched.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        for path in event.paths {
            crate::trace_event!(
                "watcher",
                "event",
                "{:?} {}",
                event.kind,
                crate::parsing::paths::render_absolute_path(&path).display()
            );
            // A directory never matches a file handler (extension gate);
            // it is the watcher's own concern: extend the watch set and
            // catch up files that landed before the watch existed. Disk
            // truth decides, not event kind -- a dir rename's to-side
            // arrives as Modify(Name), never Create.
            if path.is_dir() {
                self.handle_created_directory(&path).await;
                continue;
            }

            // A vanished path that prefixes watched directories is a
            // directory removal observation (dir-rename from-side or true
            // dir delete). It arrives as Modify(Name) or a stale Create
            // with NO per-file events following; one removal observation
            // stands in for the subtree and the wave's batch sync
            // re-derives the owning root.
            if !path.exists()
                && self
                    .registry
                    .watch_dirs()
                    .iter()
                    .any(|dir| dir.starts_with(&path))
            {
                self.debouncer.record_removal(path);
                continue;
            }

            // Check if any handler cares about this path
            let matched = self.handlers.iter().any(|h| h.matches(&path));
            if !matched {
                crate::trace_event!(
                    "watcher",
                    "unmatched",
                    "{:?} {}",
                    event.kind,
                    crate::parsing::paths::render_absolute_path(&path).display()
                );
                continue;
            }

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    // Debounce creations and modifications alike; the
                    // exists() re-check in process_modification handles
                    // paths that vanish before the debounce fires.
                    self.debouncer.record(path);
                }
                EventKind::Remove(_) => {
                    // Deferred, not immediate: a rename arrives as
                    // remove(old) + create(new), and only a batch holding
                    // both sides lets the shared discovery pair them.
                    // Genuine deletions pay one debounce window before
                    // cleanup.
                    self.debouncer.record_removal(path);
                }
                _ => {}
            }
        }
    }

    /// Register handler watch roots: watched directly so directory
    /// creation at the top of a root is visible even when the root
    /// holds no indexed file itself.
    async fn register_handler_roots(&mut self) {
        let mut roots = Vec::new();
        let mut sync_roots = Vec::new();
        for handler in &self.handlers {
            let handler_roots = handler.watch_roots().await;
            if handler.covered_by_batch_sync() {
                sync_roots.extend(handler_roots.iter().cloned());
            }
            roots.extend(handler_roots);
        }
        for root in &roots {
            if self.registry.add_watch_dir(root.clone()) {
                if let Err(e) = self.watch_directory(root) {
                    tracing::warn!("[watcher] failed to watch root: {e}");
                }
            }
        }
        self.handler_roots = roots;
        self.batch_sync_roots = sync_roots;
    }

    /// A directory appeared under a registered root: watch every
    /// traversable directory of the new subtree (ignore chains anchored
    /// at the root prune ignored trees), then route the files already
    /// inside through the normal debounce -> eligibility -> reindex path.
    async fn handle_created_directory(&mut self, path: &Path) {
        if !self.handler_roots.iter().any(|r| path.starts_with(r)) {
            return;
        }

        let (dirs, files) = {
            let facade = self.facade.read().await;
            (
                facade.discoverable_dirs(path),
                facade.discoverable_files(path),
            )
        };

        for dir in dirs {
            if self.registry.add_watch_dir(dir.clone()) {
                if let Err(e) = self.watch_directory(&dir) {
                    tracing::warn!("[watcher] failed to watch created dir: {e}");
                }
            }
        }
        if !files.is_empty() {
            crate::log_event!(
                "watcher",
                "created dir",
                "{} ({} files to catch up)",
                crate::parsing::paths::render_absolute_path(path).display(),
                files.len()
            );
        }
        for file in files {
            self.debouncer.record(file);
        }
    }

    /// Process a debounced file modification.
    async fn process_modification(&self, path: &Path) {
        // Vanished since the drain: the removal lane owns it -- the
        // caller recorded a removal observation, or the Remove event is
        // in flight.
        if !path.exists() {
            return;
        }

        for handler in &self.handlers {
            if !handler.matches(path) {
                continue;
            }

            crate::log_event!(
                handler.name(),
                "modified",
                "{}",
                crate::parsing::paths::render_absolute_path(path).display()
            );

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

    /// Process one settled burst that contains removal observations.
    ///
    /// Roots owned by a batch-sync-covered handler run the shared batch
    /// incremental lane: its discovery re-derives new/modified/deleted
    /// from disk-vs-index truth and pairs renames -- the one boundary
    /// all incremental entry points share. Paths outside every synced
    /// root keep per-file semantics.
    async fn process_removal_wave(&mut self, removed: Vec<PathBuf>, modified: Vec<PathBuf>) {
        let mut roots: Vec<PathBuf> = Vec::new();
        for path in removed.iter().chain(modified.iter()) {
            if let Some(root) = self
                .batch_sync_roots
                .iter()
                .find(|root| path.starts_with(root))
            {
                if !roots.contains(root) {
                    roots.push(root.clone());
                }
            }
        }

        for root in &roots {
            crate::log_event!(
                "watcher",
                "batch sync",
                "{}",
                crate::parsing::paths::render_absolute_path(root).display()
            );
            let mut indexer = self.facade.write().await;
            match indexer.index_directory(root, false) {
                Ok(stats) => {
                    crate::log_event!(
                        "watcher",
                        "batch synced",
                        "{} indexed, {} removed",
                        stats.files_indexed,
                        stats.files_removed
                    );
                }
                Err(e) if is_writer_lock_contention(&e) => {
                    tracing::info!(
                        "[watcher] batch sync skipped: another serve process holds the index writer; hot-reload converges"
                    );
                }
                Err(e) => {
                    tracing::error!("[watcher] batch sync failed: {e}");
                }
            }
        }

        if !roots.is_empty() {
            // Handler caches and subscribers refresh through the same
            // event hot-reload uses; the sync may have relocated paths.
            self.broadcaster.send(FileChangeEvent::IndexReloaded);
        }

        // Per-file semantics for everything the batch sync does not
        // subsume: paths outside every synced root, and handlers not
        // covered by the batch lane even under one (document files can
        // live inside a code root).
        for path in &removed {
            let covered = roots.iter().any(|root| path.starts_with(root));
            self.process_wave_residual(path, covered, true).await;
        }
        for path in &modified {
            let covered = roots.iter().any(|root| path.starts_with(root));
            if !path.exists() {
                continue;
            }
            self.process_wave_residual(path, covered, false).await;
        }
    }

    /// Route one wave path through every handler the batch sync did not
    /// subsume.
    async fn process_wave_residual(&self, path: &Path, batch_covered: bool, is_removal: bool) {
        for handler in &self.handlers {
            if !handler.matches(path) {
                continue;
            }
            if batch_covered && handler.covered_by_batch_sync() {
                continue;
            }

            let (verb, result) = if is_removal {
                ("deleted", handler.on_delete(path).await)
            } else {
                ("modified", handler.on_modify(path).await)
            };
            crate::log_event!(
                handler.name(),
                verb,
                "{}",
                crate::parsing::paths::render_absolute_path(path).display()
            );

            match result {
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
            WatchAction::ReindexCode { path, created } => {
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

                                // A first-time file grew the resource list;
                                // the lanes map FileCreated to list_changed
                                // and FileReindexed to a URI-filtered update.
                                let event = if created {
                                    FileChangeEvent::FileCreated { path: path.clone() }
                                } else {
                                    FileChangeEvent::FileReindexed { path: path.clone() }
                                };
                                self.broadcaster.send(event);
                            }
                            IndexingResult::Cached(_) => {
                                crate::debug_event!(handler_name, "unchanged (hash match)");
                            }
                        }
                    }
                    Err(e) if is_writer_lock_contention(&e) => {
                        tracing::info!(
                            "[{handler_name}] reindex skipped: another serve process holds the index writer; hot-reload converges"
                        );
                    }
                    Err(e) => {
                        tracing::error!("[{handler_name}] reindex failed: {e}");
                    }
                }
            }

            WatchAction::RemoveCode { path } => {
                let mut indexer = self.facade.write().await;
                if let Err(e) = indexer.remove_file(&path) {
                    if is_writer_lock_contention(&e) {
                        tracing::info!(
                            "[{handler_name}] remove skipped: another serve process holds the index writer; hot-reload converges"
                        );
                    } else {
                        tracing::error!("[{handler_name}] failed to remove: {e}");
                    }
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
                        tracing::info!(
                            "  + {}",
                            crate::parsing::paths::render_absolute_path(path).display()
                        );
                    }

                    let mut indexer = self.facade.write().await;
                    for path in &added {
                        crate::log_event!(
                            "config",
                            "indexing",
                            "{}",
                            crate::parsing::paths::render_absolute_path(path).display()
                        );
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
                        tracing::info!(
                            "  - {}",
                            crate::parsing::paths::render_absolute_path(path).display()
                        );
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

        // Config reload can add or drop roots; re-register them.
        self.register_handler_roots().await;

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
            handler_roots: Vec::new(),
            batch_sync_roots: Vec::new(),
        })
    }
}

impl Default for UnifiedWatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Another serve process holds the Tantivy index writer for this
/// workspace: its watcher indexes the change and this process
/// converges via hot-reload. Tantivy surfaces the contention as a
/// lockfile-acquire failure in the storage error chain; that text is
/// the only marker crossing the boxed layers.
fn is_writer_lock_contention(e: &crate::IndexError) -> bool {
    e.to_string().contains("Failed to acquire Lockfile")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::watcher::handlers::CodeFileHandler;
    use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
    use std::path::Path;

    async fn watcher_over(dir: &Path, root: &Path) -> UnifiedWatcher {
        let mut settings = Settings {
            index_path: dir.join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .add_indexed_path(root.to_path_buf())
            .expect("register indexed path");
        let facade = Arc::new(RwLock::new(IndexFacade::new(Arc::new(settings)).unwrap()));
        let handler = CodeFileHandler::new(Arc::clone(&facade), dir.to_path_buf());
        handler.init_cache().await;
        UnifiedWatcher::builder()
            .handler(handler)
            .broadcaster(Arc::new(NotificationBroadcaster::new(16)))
            .indexer(facade)
            .workspace_root(dir.to_path_buf())
            .build()
            .unwrap()
    }

    // A dir rename's from-side arrives as Modify(Name) on a path that no
    // longer exists, and no per-file events follow. A vanished path that
    // prefixes watched directories is a directory removal observation:
    // it must enter the removal wave, not fall to the unmatched trace.
    #[tokio::test]
    async fn vanished_watched_dir_records_a_removal_observation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/a.py"), "def a():\n    pass\n").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let pkg = canonical_root.join("pkg");

        let mut watcher = watcher_over(dir.path(), &root).await;
        watcher.registry.add_watch_dir(pkg.clone());

        std::fs::remove_dir_all(&pkg).unwrap();
        watcher
            .handle_event(Event {
                kind: EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
                paths: vec![pkg],
                attrs: Default::default(),
            })
            .await;

        assert!(
            watcher.debouncer.has_pending_removals(),
            "a vanished watched directory must record a removal observation"
        );
    }

    // A dir rename's to-side arrives as Modify(Name) on a path that IS a
    // directory -- never as Create. Disk truth decides the route: an
    // existing directory under a handler root runs created-directory
    // catch-up regardless of event kind.
    #[tokio::test]
    async fn existing_dir_routes_to_catchup_regardless_of_event_kind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("pkg_renamed")).unwrap();
        std::fs::write(root.join("pkg_renamed/a.py"), "def a():\n    pass\n").unwrap();
        let canonical_root = root.canonicalize().unwrap();

        let mut watcher = watcher_over(dir.path(), &root).await;
        watcher.handler_roots = vec![canonical_root.clone()];

        watcher
            .handle_event(Event {
                kind: EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
                paths: vec![canonical_root.join("pkg_renamed")],
                attrs: Default::default(),
            })
            .await;

        assert!(
            watcher.debouncer.has_pending(),
            "an existing directory's files must enter the catch-up debounce on any event kind"
        );
    }

    // inotify emits Access(Open) for every directory read, including
    // the catch-up walk's own opens; routing those into the directory
    // branch livelocks (walk emits Open, Open triggers walk), which
    // starves the debounce drain and silences every notification.
    // Access observes state and never changes it: dropped before any
    // routing. FSEvents emits no Access events, so only Linux
    // exercises this.
    #[tokio::test]
    async fn access_events_route_nowhere() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/a.py"), "def a():\n    pass\n").unwrap();
        let canonical_root = root.canonicalize().unwrap();

        let mut watcher = watcher_over(dir.path(), &root).await;
        watcher.handler_roots = vec![canonical_root.clone()];

        watcher
            .handle_event(Event {
                kind: EventKind::Access(AccessKind::Open(AccessMode::Any)),
                paths: vec![canonical_root.join("pkg")],
                attrs: Default::default(),
            })
            .await;

        assert!(
            !watcher.debouncer.has_pending(),
            "an Access event on a directory must not enter catch-up"
        );
        assert!(
            !watcher.debouncer.has_pending_removals(),
            "an Access event must not record a removal observation"
        );
    }

    #[test]
    fn writer_lock_contention_is_classified_from_the_error_chain() {
        let contended = crate::IndexError::General(
            "Pipeline error: Storage error: Tantivy error: \
             Failed to acquire Lockfile: LockBusy. \
             Some(\"Failed to acquire index lock.\")"
                .to_string(),
        );
        assert!(is_writer_lock_contention(&contended));

        let unrelated = crate::IndexError::General("Pipeline error: parse failed".to_string());
        assert!(!is_writer_lock_contention(&unrelated));
    }
}
