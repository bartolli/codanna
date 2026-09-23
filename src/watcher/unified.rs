//! Unified file watcher that routes events to pluggable handlers.

use std::collections::{BTreeSet, HashSet};
use std::ops::Bound;
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
    event_rx: mpsc::UnboundedReceiver<notify::Result<Event>>,
    /// The underlying file watcher.
    _watcher: Box<dyn Watcher + Send + Sync>,
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
    /// Watch topology. FSEvents stops delivering once one stream holds
    /// more than 4,096 paths, so on macOS roots register recursively and
    /// the directories under them do not register natively. Every other
    /// backend registers one non-recursive watch per directory.
    recursive_roots: bool,
    /// Roots registered recursively. Survives `PathRegistry::rebuild`, so
    /// an index reload never registers a root twice.
    native_roots: Vec<PathBuf>,
    /// Recursive topology only: every directory offered for registration,
    /// natively registered or covered by a root. It stands in for the
    /// per-directory watch set when gating events, so it is not cleared
    /// on index reload; vanished directories are pruned from it. Ordered
    /// so a prefix lookup is a range query, not a scan.
    admitted_dirs: BTreeSet<PathBuf>,
    /// Nearest surviving ancestors of pruned directories. They admit a
    /// direct-child directory event and nothing else, so a recreated
    /// directory reaches discovery.
    recreation_ancestors: HashSet<PathBuf>,
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

        self.register_watch_set(&new_dirs).await;

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

    /// Register the watch set in the order the topology needs: recursive
    /// roots first, so a new root covers its directories before they are
    /// offered; per-directory keeps directories first.
    async fn register_watch_set(&mut self, dirs: &[PathBuf]) {
        if self.recursive_roots {
            self.register_handler_roots().await;
            self.watch_directories(dirs);
        } else {
            self.watch_directories(dirs);
            self.register_handler_roots().await;
        }
    }

    fn normalized(&self, dirs: &[PathBuf]) -> Vec<PathBuf> {
        let mut watch_paths: Vec<_> = dirs
            .iter()
            .map(|dir| {
                if dir.is_absolute() {
                    dir.clone()
                } else {
                    self.workspace_root.join(dir)
                }
            })
            .collect();
        watch_paths.sort();
        watch_paths.dedup();
        watch_paths
    }

    fn watch_directories(&mut self, dirs: &[PathBuf]) -> Vec<PathBuf> {
        let mut watch_paths = self.normalized(dirs);
        if self.recursive_roots {
            self.admitted_dirs.extend(watch_paths.iter().cloned());
            watch_paths.retain(|path| !self.native_roots.iter().any(|r| path.starts_with(r)));
        }
        self.register_native(watch_paths, RecursiveMode::NonRecursive)
    }

    fn watch_roots(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
        if !self.recursive_roots {
            return self.watch_directories(roots);
        }
        let mut new_roots: Vec<PathBuf> = Vec::new();
        for root in self.normalized(roots) {
            self.admitted_dirs.insert(root.clone());
            let covered = self
                .native_roots
                .iter()
                .chain(&new_roots)
                .any(|r| root.starts_with(r));
            if !covered {
                new_roots.push(root);
            }
        }
        let failed = self.register_native(new_roots.clone(), RecursiveMode::Recursive);
        self.native_roots
            .extend(new_roots.into_iter().filter(|r| !failed.contains(r)));
        failed
    }

    fn register_native(&mut self, watch_paths: Vec<PathBuf>, mode: RecursiveMode) -> Vec<PathBuf> {
        if watch_paths.is_empty() {
            return Vec::new();
        }

        let mut failed = Vec::new();
        let mut paths = self._watcher.paths_mut();
        for path in &watch_paths {
            match paths.add(path, mode) {
                Ok(()) => {
                    crate::debug_event!(
                        "watcher",
                        "watching",
                        "{}",
                        crate::parsing::paths::render_absolute_path(path).display()
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[watcher] failed to watch {}: {e}",
                        crate::parsing::paths::render_absolute_path(path).display()
                    );
                    failed.push(path.clone());
                }
            }
        }
        if let Err(e) = paths.commit() {
            tracing::warn!("[watcher] failed to commit watch registrations: {e}");
            return watch_paths;
        }
        failed
    }

    /// Whether a per-directory watch set would have delivered `path`: an
    /// admitted directory, one of its direct children, or a vanished
    /// ancestor of one (a directory removal observation). A recursive
    /// root also reports ignored subtrees; the code handler refuses those
    /// files only after a whole-root discovery walk per path, so they are
    /// dropped here first. Under a remembered ancestor of a vanished
    /// directory only a directory is admitted: its files stay out, since
    /// ignore rules may have changed along with the removal.
    fn admits(&self, path: &Path) -> bool {
        let parent = path.parent();
        self.admitted_dirs.contains(path)
            || parent.is_some_and(|parent| self.admitted_dirs.contains(parent))
            || (self.admitted_under(path).next().is_some() && !path.exists())
            || (parent.is_some_and(|parent| self.recreation_ancestors.contains(parent))
                && path.is_dir())
    }

    fn admitted_under<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a PathBuf> {
        self.admitted_dirs
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .take_while(move |dir| dir.starts_with(path))
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
            if self.recursive_roots {
                if !self.admits(&path) {
                    crate::trace_event!(
                        "watcher",
                        "gated",
                        "{}",
                        crate::parsing::paths::render_absolute_path(&path).display()
                    );
                    continue;
                }
                // FSEvents watches by path: a directory recreated here
                // would keep its old admission, ignore rules notwithstanding.
                // The nearest surviving ancestor is remembered so the
                // recreation is recognized; discovery then decides what is
                // admitted again.
                if !path.exists() {
                    let vanished: Vec<PathBuf> = self.admitted_under(&path).cloned().collect();
                    for dir in &vanished {
                        self.admitted_dirs.remove(dir);
                    }
                    if !vanished.is_empty() {
                        if let Some(ancestor) = path.ancestors().skip(1).find(|a| a.exists()) {
                            if !self.admitted_dirs.contains(ancestor) {
                                self.recreation_ancestors.insert(ancestor.to_path_buf());
                            }
                        }
                    }
                }
            }

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
        // Recursive roots are offered every time: `native_roots` decides
        // what registers, not registry membership, which a root that is
        // also a tracked-file parent already holds.
        let new_roots: Vec<_> = roots
            .iter()
            .filter(|root| self.registry.add_watch_dir((*root).clone()) || self.recursive_roots)
            .cloned()
            .collect();
        self.watch_roots(&new_roots);
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

        // A pruned directory keeps its registry entry, so a recreated one
        // is offered again on admission, not on registry novelty.
        let new_dirs: Vec<_> = dirs
            .into_iter()
            .filter(|dir| {
                self.registry.add_watch_dir(dir.clone())
                    || (self.recursive_roots && !self.admitted_dirs.contains(dir))
            })
            .collect();
        self.watch_directories(&new_dirs);
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

        // Resolution defers across the covered roots so a burst whose
        // importing and imported files land in different roots binds
        // its cross-root edges regardless of loop order.
        let mut pending = crate::indexing::pipeline::PendingResolution::default();
        for root in &roots {
            crate::log_event!(
                "watcher",
                "batch sync",
                "{}",
                crate::parsing::paths::render_absolute_path(root).display()
            );
            let mut indexer = self.facade.write().await;
            match indexer.index_directory_deferred(root, false, &mut pending) {
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
        {
            let mut indexer = self.facade.write().await;
            match indexer.resolve_deferred(pending) {
                Ok(()) => {}
                Err(e) if is_writer_lock_contention(&e) => {
                    tracing::info!(
                        "[watcher] batch sync resolution skipped: another serve process holds the index writer; hot-reload converges"
                    );
                }
                Err(e) => {
                    tracing::error!("[watcher] batch sync resolution failed: {e}");
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
                    // Resolution defers across the added dirs so one new
                    // root's imports into another bind regardless of order.
                    let mut pending = crate::indexing::pipeline::PendingResolution::default();
                    for path in &added {
                        crate::log_event!(
                            "config",
                            "indexing",
                            "{}",
                            crate::parsing::paths::render_absolute_path(path).display()
                        );
                        match indexer.index_directory_deferred(path, false, &mut pending) {
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
                    if let Err(e) = indexer.resolve_deferred(pending) {
                        tracing::error!("  resolution failed: {e}");
                    }
                    // The next command's startup sync reads the roots
                    // from the metadata; a root missing there is indexed
                    // again. The deferred index above already persisted
                    // the semantic snapshot.
                    let persistence = crate::IndexPersistence::new(self.index_path.clone());
                    if let Err(e) = persistence.save_metadata(&indexer) {
                        tracing::warn!("  failed to save index metadata: {e}");
                    }
                    tracing::info!(
                        "  files and directories created under an added root are not watched until serve restarts"
                    );
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

        // Config reload can add or drop roots; re-register them.
        self.register_watch_set(&dirs_to_watch).await;

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
        let (tx, rx) = mpsc::unbounded_channel();

        // Create the notify watcher
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            deliver_event(&tx, res);
        })?;

        Ok(UnifiedWatcher {
            handlers: self.handlers,
            registry: PathRegistry::new(),
            debouncer: Debouncer::new(self.debounce_ms),
            event_rx: rx,
            _watcher: Box::new(watcher),
            broadcaster,
            facade,
            document_store: self.document_store,
            chunking_config: self.chunking_config,
            index_path,
            workspace_root,
            handler_roots: Vec::new(),
            batch_sync_roots: Vec::new(),
            recursive_roots: cfg!(target_os = "macos"),
            native_roots: Vec::new(),
            admitted_dirs: BTreeSet::new(),
            recreation_ancestors: HashSet::new(),
        })
    }
}

impl Default for UnifiedWatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Hands a notify event to the consumer from notify's callback thread.
/// The send must never park: registering a watch on FSEvents stops the
/// stream and spins until this callback returns, and the task doing the
/// registration is the channel's only drainer.
pub(crate) fn deliver_event(
    tx: &mpsc::UnboundedSender<notify::Result<Event>>,
    res: notify::Result<Event>,
) {
    // A closed receiver means the watcher task is gone; nothing is left
    // to deliver to.
    let _ = tx.send(res);
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
    use std::sync::Mutex;

    #[derive(Default)]
    struct RegistrationLog {
        individual: Vec<PathBuf>,
        batches: Vec<Vec<PathBuf>>,
        registered: HashSet<PathBuf>,
        commits: usize,
        fail_add: Option<PathBuf>,
        fail_commit: bool,
        allow_recursive: bool,
        recursive: Vec<PathBuf>,
    }

    struct RecordingWatcher(Arc<Mutex<RegistrationLog>>);

    impl Watcher for RecordingWatcher {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            Ok(Self(Arc::new(Mutex::new(RegistrationLog::default()))))
        }

        fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
            assert_eq!(mode, RecursiveMode::NonRecursive);
            let mut log = self.0.lock().unwrap();
            log.individual.push(path.to_path_buf());
            if log.fail_add.as_deref() == Some(path) {
                return Err(notify::Error::generic("registration rejected"));
            }
            log.registered.insert(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
            Ok(())
        }

        fn paths_mut(&mut self) -> Box<dyn notify::PathsMut + '_> {
            self.0.lock().unwrap().batches.push(Vec::new());
            Box::new(RecordingBatch(Arc::clone(&self.0)))
        }

        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    struct RecordingBatch(Arc<Mutex<RegistrationLog>>);

    impl notify::PathsMut for RecordingBatch {
        fn add(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
            let mut log = self.0.lock().unwrap();
            if mode == RecursiveMode::Recursive {
                assert!(log.allow_recursive, "recursive watch on {path:?}");
                log.recursive.push(path.to_path_buf());
            }
            log.batches.last_mut().unwrap().push(path.to_path_buf());
            if log.fail_add.as_deref() == Some(path) {
                return Err(notify::Error::generic("registration rejected"));
            }
            log.registered.insert(path.to_path_buf());
            Ok(())
        }

        fn remove(&mut self, _: &Path) -> notify::Result<()> {
            Ok(())
        }

        fn commit(self: Box<Self>) -> notify::Result<()> {
            let mut log = self.0.lock().unwrap();
            log.commits += 1;
            if log.fail_commit {
                return Err(notify::Error::generic("commit rejected"));
            }
            Ok(())
        }
    }

    struct TrackedPaths {
        files: Vec<PathBuf>,
        roots: Vec<PathBuf>,
    }

    #[async_trait::async_trait]
    impl WatchHandler for TrackedPaths {
        fn name(&self) -> &str {
            "registration"
        }

        fn matches(&self, _: &Path) -> bool {
            false
        }

        async fn tracked_paths(&self) -> Vec<PathBuf> {
            self.files.clone()
        }

        async fn watch_roots(&self) -> Vec<PathBuf> {
            self.roots.clone()
        }

        async fn on_modify(&self, _: &Path) -> Result<WatchAction, WatchError> {
            Ok(WatchAction::None)
        }

        async fn on_delete(&self, _: &Path) -> Result<WatchAction, WatchError> {
            Ok(WatchAction::None)
        }
    }

    fn record_registrations(watcher: &mut UnifiedWatcher) -> Arc<Mutex<RegistrationLog>> {
        let log = Arc::new(Mutex::new(RegistrationLog::default()));
        watcher._watcher = Box::new(RecordingWatcher(Arc::clone(&log)));
        log
    }

    #[tokio::test]
    async fn startup_registers_new_directories_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = watcher_over(dir.path(), dir.path()).await;
        watcher.handlers = vec![Box::new(TrackedPaths {
            files: vec![PathBuf::from("one/a.rs"), PathBuf::from("two/b.rs")],
            roots: Vec::new(),
        })];
        let log = record_registrations(&mut watcher);
        let mut running = Box::pin(watcher.watch());
        std::future::poll_fn(|cx| {
            assert!(running.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;

        let log = log.lock().unwrap();
        assert_eq!(
            log.batches.len(),
            1,
            "individual calls: {:?}",
            log.individual
        );
        assert_eq!(log.commits, 1);
        assert!(log.individual.is_empty());
        assert_eq!(
            log.batches[0].iter().cloned().collect::<HashSet<_>>(),
            [dir.path().join("one"), dir.path().join("two")].into()
        );
    }

    #[tokio::test]
    async fn handler_roots_register_new_directories_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = watcher_over(dir.path(), dir.path()).await;
        let roots = vec![
            dir.path().join("old"),
            dir.path().join("one"),
            dir.path().join("two"),
        ];
        watcher.registry.add_watch_dir(roots[0].clone());
        watcher.handlers = vec![Box::new(TrackedPaths {
            files: Vec::new(),
            roots: roots.clone(),
        })];
        let log = record_registrations(&mut watcher);

        watcher.register_handler_roots().await;
        watcher.register_handler_roots().await;

        let log = log.lock().unwrap();
        assert_eq!(
            log.batches.len(),
            1,
            "individual calls: {:?}",
            log.individual
        );
        assert_eq!(log.commits, 1);
        assert!(log.individual.is_empty());
        assert_eq!(
            log.batches[0].iter().cloned().collect::<HashSet<_>>(),
            roots[1..].iter().cloned().collect()
        );
        assert_eq!(watcher.handler_roots, roots);
    }

    #[tokio::test]
    async fn created_subtree_registers_new_directories_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let subtree = root.join("new");
        std::fs::create_dir_all(subtree.join("nested")).unwrap();
        std::fs::write(subtree.join("a.py"), "def a():\n    pass\n").unwrap();
        std::fs::write(subtree.join("nested/b.py"), "def b():\n    pass\n").unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        watcher.handler_roots = vec![root];
        let log = record_registrations(&mut watcher);

        watcher.handle_created_directory(&subtree).await;

        let log = log.lock().unwrap();
        assert_eq!(
            log.batches.len(),
            1,
            "individual calls: {:?}",
            log.individual
        );
        assert_eq!(log.commits, 1);
        assert!(log.individual.is_empty());
        assert_eq!(
            log.batches[0].iter().cloned().collect::<HashSet<_>>(),
            [subtree.clone(), subtree.join("nested")].into()
        );
        assert!(watcher.debouncer.has_pending());
    }

    #[tokio::test]
    async fn index_reload_registers_new_directories_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = watcher_over(dir.path(), dir.path()).await;
        watcher.registry.add_paths([PathBuf::from("old/a.rs")]);
        let roots = vec![dir.path().join("root_one"), dir.path().join("root_two")];
        watcher.handlers = vec![Box::new(TrackedPaths {
            files: vec![
                PathBuf::from("old/a.rs"),
                PathBuf::from("one/b.rs"),
                PathBuf::from("two/c.rs"),
            ],
            roots: roots.clone(),
        })];
        let log = record_registrations(&mut watcher);

        watcher.handle_index_reloaded().await;

        let log = log.lock().unwrap();
        assert_eq!(
            log.batches.len(),
            2,
            "individual calls: {:?}",
            log.individual
        );
        assert_eq!(log.commits, 2);
        assert!(log.individual.is_empty());
        assert_eq!(
            log.batches[0].iter().cloned().collect::<HashSet<_>>(),
            [dir.path().join("one"), dir.path().join("two")].into()
        );
        assert_eq!(
            log.batches[1].iter().cloned().collect::<HashSet<_>>(),
            roots.into_iter().collect()
        );
    }

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn registration_sites_preserve_paths_and_continue_after_bad_path() {
        for site in [
            "startup",
            "handler_roots",
            "created_subtree",
            "index_reload",
        ] {
            for fail in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let workspace = dir.path().canonicalize().unwrap();
                let root = workspace.join("root");
                let one = root.join("one");
                let two = root.join("two");
                std::fs::create_dir_all(&one).unwrap();
                std::fs::create_dir_all(&two).unwrap();
                std::fs::write(one.join("a.py"), "def a():\n    pass\n").unwrap();
                std::fs::write(two.join("b.py"), "def b():\n    pass\n").unwrap();
                let mut watcher = watcher_over(&workspace, &root).await;
                watcher.handlers = vec![Box::new(TrackedPaths {
                    files: vec![PathBuf::from("root/one/a.py"), two.join("b.py")],
                    roots: if site == "handler_roots" {
                        vec![PathBuf::from("root/one"), two.clone()]
                    } else {
                        Vec::new()
                    },
                })];
                watcher.handler_roots = vec![root.clone()];
                watcher.registry.add_watch_dir(root.clone());
                let log = record_registrations(&mut watcher);
                log.lock().unwrap().fail_add = fail.then(|| one.clone());
                let output = LogBuffer::default();
                let writer = output.clone();
                let _guard = tracing::subscriber::set_default(
                    tracing_subscriber::fmt()
                        .with_ansi(false)
                        .without_time()
                        .with_max_level(tracing::Level::WARN)
                        .with_writer(move || writer.clone())
                        .finish(),
                );

                match site {
                    "startup" => {
                        let mut running = Box::pin(watcher.watch());
                        std::future::poll_fn(|cx| {
                            assert!(running.as_mut().poll(cx).is_pending());
                            std::task::Poll::Ready(())
                        })
                        .await;
                    }
                    "handler_roots" => watcher.register_handler_roots().await,
                    "created_subtree" => watcher.handle_created_directory(&root).await,
                    "index_reload" => watcher.handle_index_reloaded().await,
                    _ => unreachable!(),
                }

                let expected = if fail {
                    HashSet::from([two])
                } else {
                    HashSet::from([one.clone(), two])
                };
                assert_eq!(log.lock().unwrap().registered, expected, "{site}");
                let output = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
                if fail {
                    let path = crate::parsing::paths::render_absolute_path(&one);
                    assert!(
                        output.contains(&format!(
                            "[watcher] failed to watch {}: registration rejected",
                            path.display()
                        )),
                        "{site}: {output}"
                    );
                } else {
                    assert!(output.is_empty(), "{site}: {output}");
                }
            }
        }
    }

    #[tokio::test]
    async fn registration_reports_failed_paths_without_duplicate_or_empty_batches() {
        for fail_commit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut watcher = watcher_over(dir.path(), dir.path()).await;
            let one = dir.path().join("one");
            let two = dir.path().join("two");
            let log = record_registrations(&mut watcher);
            {
                let mut log = log.lock().unwrap();
                log.fail_add = Some(one.clone());
                log.fail_commit = fail_commit;
            }

            let failed =
                watcher.watch_directories(&[PathBuf::from("one"), one.clone(), two.clone()]);

            let expected = if fail_commit {
                HashSet::from([one.clone(), two.clone()])
            } else {
                HashSet::from([one.clone()])
            };
            assert_eq!(failed.iter().cloned().collect::<HashSet<_>>(), expected);
            assert_eq!(failed.len(), expected.len());
            assert!(watcher.watch_directories(&[]).is_empty());
            let log = log.lock().unwrap();
            assert_eq!(log.batches.len(), 1);
            assert_eq!(log.commits, 1);
            assert_eq!(log.batches[0].len(), 2);
            assert_eq!(
                log.batches[0].iter().cloned().collect::<HashSet<_>>(),
                [one, two].into()
            );
        }
    }

    // The site tests observe a recording double. This one drives the
    // builder-constructed watcher: a `paths_mut` that falls back to
    // notify's per-path default restarts the FSEvents stream once per
    // directory and loses the ratio. Other backends register per path in
    // both shapes, so the ratio exists only on macOS.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn production_watcher_batches_fsevents_registration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let dirs: Vec<PathBuf> = (0..12).map(|i| root.join(format!("d{i}"))).collect();
        for d in &dirs {
            std::fs::create_dir(d).unwrap();
        }

        let mut per_path = watcher_over(&root, &root).await;
        let start = std::time::Instant::now();
        for d in &dirs {
            per_path
                ._watcher
                .watch(d, RecursiveMode::NonRecursive)
                .unwrap();
        }
        let per_path_elapsed = start.elapsed();

        let mut batched = watcher_over(&root, &root).await;
        let start = std::time::Instant::now();
        assert!(batched.watch_directories(&dirs).is_empty());
        let batched_elapsed = start.elapsed();

        assert!(
            batched_elapsed * 4 < per_path_elapsed,
            "per-path {per_path_elapsed:?}, batched {batched_elapsed:?}"
        );
    }

    fn record_recursive(watcher: &mut UnifiedWatcher) -> Arc<Mutex<RegistrationLog>> {
        watcher.recursive_roots = true;
        let log = record_registrations(watcher);
        log.lock().unwrap().allow_recursive = true;
        log
    }

    #[tokio::test]
    async fn recursive_roots_register_natively_without_their_directories() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().canonicalize().unwrap();
        let root = workspace.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let mut watcher = watcher_over(&workspace, &root).await;
        watcher.handlers = vec![Box::new(TrackedPaths {
            files: vec![
                root.join("one/a.rs"),
                root.join("two/b.rs"),
                workspace.join("conf/settings.toml"),
            ],
            roots: vec![root.clone()],
        })];
        let log = record_recursive(&mut watcher);

        let mut running = Box::pin(watcher.watch());
        std::future::poll_fn(|cx| {
            assert!(running.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;

        let log = log.lock().unwrap();
        assert_eq!(log.recursive, vec![root.clone()]);
        assert_eq!(
            log.registered,
            HashSet::from([root, workspace.join("conf")])
        );
    }

    fn created(path: &Path) -> Event {
        Event {
            kind: EventKind::Create(notify::event::CreateKind::Any),
            paths: vec![path.to_path_buf()],
            attrs: Default::default(),
        }
    }

    // Per-directory watching never delivers events from an ignored tree
    // because the tree is never registered. A recursive root delivers
    // them; `on_modify` would refuse each file, but only after a
    // whole-root discovery walk, so the gate keeps them out of the
    // debouncer.
    #[tokio::test]
    async fn recursive_roots_drop_events_outside_admitted_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let src = root.join("src");
        let ignored = root.join("target/debug");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&ignored).unwrap();
        std::fs::write(src.join("a.py"), "def a():\n    pass\n").unwrap();
        std::fs::write(ignored.join("gen.py"), "def gen():\n    pass\n").unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let _log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        watcher.watch_directories(std::slice::from_ref(&src));

        watcher.handle_event(created(&ignored.join("gen.py"))).await;
        watcher.handle_event(created(&ignored)).await;
        assert!(!watcher.debouncer.has_pending());
        assert!(!watcher.admitted_dirs.contains(&ignored));

        watcher.handle_event(created(&src.join("a.py"))).await;
        assert!(watcher.debouncer.has_pending());
    }

    #[tokio::test]
    async fn recursive_roots_admit_a_created_subtree_without_native_watches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        let subtree = root.join("new");
        let nested = subtree.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(subtree.join("a.py"), "def a():\n    pass\n").unwrap();

        watcher.handle_event(created(&subtree)).await;

        assert!(watcher.admitted_dirs.contains(&subtree));
        assert!(watcher.admits(&nested.join("later.py")));
        assert!(watcher.debouncer.has_pending());
        assert_eq!(log.lock().unwrap().registered, HashSet::from([root]));
    }

    #[tokio::test]
    async fn recursive_roots_prune_a_vanished_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let pkg = root.join("pkg");
        let deep = root.join("x/y/z");
        std::fs::create_dir_all(pkg.join("sub")).unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let _log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        for dir in [pkg.clone(), pkg.join("sub"), deep.clone()] {
            watcher.registry.add_watch_dir(dir.clone());
            watcher.watch_directories(&[dir]);
        }

        std::fs::remove_dir_all(&pkg).unwrap();
        watcher.handle_event(created(&pkg)).await;
        assert!(watcher.debouncer.has_pending_removals());
        assert!(!watcher.admitted_dirs.contains(&pkg));
        assert!(!watcher.admitted_dirs.contains(&pkg.join("sub")));

        // Recreated at the same path under an ignore rule: membership
        // from before the removal must not admit it.
        std::fs::write(root.join(".codannaignore"), "pkg/\n").unwrap();
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("a.py"), "def a():\n    pass\n").unwrap();
        watcher.handle_event(created(&pkg)).await;
        assert!(!watcher.admitted_dirs.contains(&pkg));
        assert!(!watcher.admits(&pkg.join("a.py")));

        // A vanished ancestor of an admitted directory is a removal
        // observation even though neither it nor its parent is admitted.
        std::fs::remove_dir_all(root.join("x/y")).unwrap();
        assert!(watcher.admits(&root.join("x/y")));
        assert!(!watcher.admits(&root.join("x/other")));
    }

    // Pruning leaves the registry entry behind, so re-admission cannot
    // hang on `add_watch_dir` reporting the directory as new.
    #[tokio::test]
    async fn recursive_roots_readmit_a_recreated_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let pkg = root.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let _log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        watcher.handle_event(created(&pkg)).await;
        assert!(watcher.admits(&pkg.join("a.py")));

        std::fs::remove_dir_all(&pkg).unwrap();
        watcher.handle_event(created(&pkg)).await;
        assert!(!watcher.admitted_dirs.contains(&pkg));

        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("a.py"), "def a():\n    pass\n").unwrap();
        watcher.handle_event(created(&pkg)).await;
        watcher.handle_index_reloaded().await;

        assert!(watcher.admits(&pkg.join("a.py")));
    }

    // Startup admits tracked-file parents only, so in `x/y/z` neither `x`
    // nor `x/y` is admitted. When `y` is removed and recreated (a branch
    // switch over a package-per-directory tree), the recreation is
    // observed only if an existing ancestor took over the admission.
    #[tokio::test]
    async fn recursive_roots_readmit_a_recreated_deep_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let deep = root.join("x/y/z");
        std::fs::create_dir_all(&deep).unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let _log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        watcher.registry.add_watch_dir(deep.clone());
        watcher.watch_directories(std::slice::from_ref(&deep));

        std::fs::remove_dir_all(root.join("x/y")).unwrap();
        watcher.handle_event(created(&deep)).await;
        watcher.handle_event(created(&root.join("x/y"))).await;

        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("a.py"), "def a():\n    pass\n").unwrap();
        watcher.handle_event(created(&root.join("x/y"))).await;

        assert!(watcher.admits(&deep.join("a.py")));
    }

    // The surviving ancestor only recognizes a recreated directory. Ignore
    // rules can change with the removal (a branch switch that also ignores
    // `x/`), so it must not admit its direct-child files.
    #[tokio::test]
    async fn recursive_roots_surviving_ancestor_admits_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let deep = root.join("x/y/z");
        std::fs::create_dir_all(&deep).unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let _log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        watcher.registry.add_watch_dir(deep.clone());
        watcher.watch_directories(std::slice::from_ref(&deep));

        std::fs::write(root.join(".codannaignore"), "x/\n").unwrap();
        std::fs::remove_dir_all(root.join("x/y")).unwrap();
        watcher.handle_event(created(&deep)).await;
        std::fs::write(root.join("x/generated.py"), "def g():\n    pass\n").unwrap();
        assert!(!watcher.admits(&root.join("x/generated.py")));

        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("a.py"), "def a():\n    pass\n").unwrap();
        watcher.handle_event(created(&root.join("x/y"))).await;
        assert!(!watcher.admits(&deep.join("a.py")));
    }

    // `PathRegistry::rebuild` clears membership on every reload. The root
    // must not register again (FSEvents appends duplicates toward its
    // path limit), and a discovered directory with no indexed file must
    // stay admitted.
    #[tokio::test]
    async fn recursive_roots_survive_index_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let mut watcher = watcher_over(&root, &root).await;
        let log = record_recursive(&mut watcher);
        watcher.register_handler_roots().await;
        watcher.handle_event(created(&empty)).await;

        for _ in 0..3 {
            watcher.handle_index_reloaded().await;
        }

        assert_eq!(log.lock().unwrap().recursive, vec![root]);
        assert!(watcher.admits(&empty.join("first.py")));
    }

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
        let mut watcher = UnifiedWatcher::builder()
            .handler(handler)
            .broadcaster(Arc::new(NotificationBroadcaster::new(16)))
            .indexer(facade)
            .workspace_root(dir.to_path_buf())
            .build()
            .unwrap();
        // Tests pick the topology; the platform default would make the
        // per-directory tests pass or fail by host.
        watcher.recursive_roots = false;
        watcher
    }

    #[tokio::test]
    async fn unchanged_code_events_do_not_notify_but_edits_do() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let source = root.join("a.py");
        std::fs::write(&source, "def alpha():\n    pass\n").unwrap();
        let watcher = watcher_over(dir.path(), &root).await;
        let mut events = watcher.broadcaster.subscribe();

        watcher
            .execute_action(
                WatchAction::ReindexCode {
                    path: source.clone(),
                    created: true,
                },
                "code",
            )
            .await
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            FileChangeEvent::FileCreated { path } if path == source
        ));

        // Duplicate observations take the cached branch and emit no reindex
        // notification. This facade has no semantic search, so the snapshot
        // save that the cached branch also skips is not exercised here.
        for _ in 0..2 {
            watcher
                .execute_action(
                    WatchAction::ReindexCode {
                        path: source.clone(),
                        created: false,
                    },
                    "code",
                )
                .await
                .unwrap();
            assert!(matches!(
                events.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ));
        }

        std::fs::write(&source, "def beta():\n    pass\n").unwrap();
        watcher
            .execute_action(
                WatchAction::ReindexCode {
                    path: source.clone(),
                    created: false,
                },
                "code",
            )
            .await
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            FileChangeEvent::FileReindexed { path } if path == source
        ));
        let facade = watcher.facade.read().await;
        assert!(facade.find_symbols_by_name("alpha", None).is_empty());
        assert_eq!(facade.find_symbols_by_name("beta", None).len(), 1);
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

    #[test]
    fn deliver_event_returns_without_a_drainer() {
        let (tx, rx) = mpsc::unbounded_channel();
        let producer = std::thread::spawn(move || {
            for _ in 0..1000 {
                deliver_event(&tx, Ok(Event::new(EventKind::Any)));
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !producer.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "a send parked with no drainer on the channel"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        producer.join().expect("producer thread completes");
        assert_eq!(rx.len(), 1000);
    }
}
