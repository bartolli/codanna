//! Index stage - Tantivy batch writes
//!
//! [PIPELINE API] Part of the new parallel indexing pipeline.
//!
//! Parallel stage that:
//! - Receives IndexBatch from COLLECT stage
//! - Writes symbols, imports, file registrations to Tantivy (parallel via RwLock)
//! - Accumulates UnresolvedRelationships for Phase 2
//! - Builds SymbolLookupCache for O(1) Phase 2 resolution (concurrent DashMap)
//! - Commits every N batches for efficient I/O
//!
//! Note: Embedding generation moved to separate EMBED stage (parallel with INDEX).

use crate::indexing::IndexStats;
use crate::indexing::pipeline::types::{
    IndexBatch, PipelineError, PipelineResult, SymbolLookupCache, UnresolvedRelationship,
};
use crate::io::status_line::ProgressBar;
use crate::storage::DocumentIndex;
use crossbeam_channel::Receiver;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Index stage for Tantivy writes.
///
/// [PIPELINE API] Receives batches from COLLECT and writes to Tantivy efficiently.
/// Commits are batched to reduce fsync overhead.
///
/// Error handling uses proper `#[from]` conversion - StorageError -> PipelineError.
/// Progress callback type for INDEX stage.
pub type IndexProgressCallback = Arc<dyn Fn(u64) + Send + Sync>;

pub struct IndexStage {
    index: Arc<DocumentIndex>,
    batches_per_commit: usize,
    /// Optional progress bar for live updates.
    progress: Option<Arc<ProgressBar>>,
    /// Optional progress callback (alternative to progress bar).
    progress_callback: Option<IndexProgressCallback>,
    /// Starting (file, symbol) counters; seeds the durable high-water
    /// mark so a partial run never persists counters below ids consumed
    /// by earlier generations.
    counter_floor: (u32, u32),
}

impl IndexStage {
    /// Create a new index stage.
    ///
    /// `batches_per_commit` controls how often we commit to Tantivy.
    /// Higher values improve throughput but increase memory usage.
    pub fn new(index: Arc<DocumentIndex>, batches_per_commit: usize) -> Self {
        Self {
            index,
            batches_per_commit: batches_per_commit.max(1),
            progress: None,
            progress_callback: None,
            counter_floor: (0, 0),
        }
    }

    /// Seed the durable-counter high-water mark with the run's starting
    /// counters (from `get_start_counters`).
    pub fn with_counter_floor(mut self, file: u32, symbol: u32) -> Self {
        self.counter_floor = (file, symbol);
        self
    }

    /// Add progress bar for live updates.
    pub fn with_progress(mut self, progress: Arc<ProgressBar>) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Add a progress callback that receives the count of files indexed per batch.
    pub fn with_progress_callback(mut self, callback: IndexProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Run the index stage.
    ///
    /// Returns (stats, accumulated_relationships, symbol_cache, input_wait) for Phase 2.
    /// The symbol cache enables O(1) lookups during resolution instead of Tantivy queries.
    pub fn run(
        &self,
        receiver: Receiver<IndexBatch>,
    ) -> PipelineResult<(
        IndexStats,
        Vec<UnresolvedRelationship>,
        SymbolLookupCache,
        std::time::Duration,
    )> {
        use std::time::{Duration, Instant};

        let mut stats = IndexStats::new();
        let mut pending_relationships: Vec<UnresolvedRelationship> = Vec::new();
        let mut batch_count = 0;
        let mut failed_files_total = 0usize;
        let mut input_wait = Duration::ZERO;

        // Pre-allocate cache based on expected symbols (will grow if needed)
        let symbol_cache = SymbolLookupCache::with_capacity(10_000);

        // Start initial batch - StorageError converts to PipelineError via #[from]
        self.index.start_batch()?;

        let (mut file_high_water, mut symbol_high_water) = self.counter_floor;

        loop {
            // Track input wait (time blocked on recv)
            let recv_start = Instant::now();
            let mut batch = match receiver.recv() {
                Ok(b) => b,
                Err(_) => break, // Channel closed
            };
            input_wait += recv_start.elapsed();

            // Accumulate relationships for Phase 2
            pending_relationships.extend(std::mem::take(&mut batch.unresolved_relationships));

            for registration in &batch.file_registrations {
                file_high_water = file_high_water.max(registration.file_id.value());
            }
            for symbol in &batch.symbols {
                symbol_high_water = symbol_high_water.max(symbol.id.value());
            }

            failed_files_total += self.process_batch(batch, &mut stats, &symbol_cache)?;

            batch_count += 1;

            // Commit every N batches
            if batch_count % self.batches_per_commit == 0 {
                self.persist_counters(file_high_water, symbol_high_water)?;
                self.commit_and_restart()?;
            }
        }

        // Final commit
        self.persist_counters(file_high_water, symbol_high_water)?;
        self.index.commit_batch()?;

        // Clean work is committed and durable; failed files are unregistered
        // (no content hash) so the next run re-visits them. The stage still
        // fails so the run does not report success over a known gap.
        if failed_files_total > 0 {
            return Err(PipelineError::Parse {
                path: PathBuf::new(),
                reason: format!(
                    "{failed_files_total} file(s) had failed index writes and were left \
                     unregistered; re-run indexing to fill the gap"
                ),
            });
        }

        Ok((stats, pending_relationships, symbol_cache, input_wait))
    }

    /// Process a single batch.
    ///
    /// Writes symbols, imports, and file registrations to Tantivy in parallel.
    /// Accumulates symbols in cache for Phase 2 resolution.
    ///
    /// Returns the number of files in this batch that were left unregistered
    /// because a write failed.
    fn process_batch(
        &self,
        batch: IndexBatch,
        stats: &mut IndexStats,
        symbol_cache: &SymbolLookupCache,
    ) -> PipelineResult<usize> {
        // Symbols are written before file registrations: a file whose symbol
        // writes fail must not be registered with its content hash, or the
        // next incremental run treats it as up-to-date and the gap becomes
        // permanent. COLLECT keeps a file's registration and symbols in one
        // batch, so the skip below covers every registration for the file.
        let failed_files: Mutex<HashSet<Box<str>>> = Mutex::new(HashSet::new());

        // Write symbols to Tantivy in parallel
        // SymbolLookupCache uses DashMap which is concurrent-safe
        let symbols_in_batch = batch.symbols.len();
        batch.symbols.into_par_iter().for_each(|symbol| {
            if let Err(e) = self.index.index_symbol(&symbol, &symbol.file_path) {
                tracing::error!(
                    target: "pipeline",
                    "Failed to index symbol {}: {e}",
                    symbol.name
                );
                if let Ok(mut failed) = failed_files.lock() {
                    failed.insert(symbol.file_path.clone());
                }
            }
            // Insert into cache for O(1) Phase 2 resolution (DashMap is concurrent)
            symbol_cache.insert(symbol);
        });
        stats.symbols_found += symbols_in_batch;

        let failed = failed_files
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Write file registrations in parallel, skipping failed files
        let registration_failures = AtomicUsize::new(0);
        batch
            .file_registrations
            .par_iter()
            .filter(|registration| !failed.contains(registration.path.to_string_lossy().as_ref()))
            .for_each(|registration| {
                if let Err(e) = self.index.store_file_registration(registration) {
                    tracing::error!(
                        target: "pipeline",
                        "Failed to store file registration for {}: {e}",
                        registration.path.display()
                    );
                    registration_failures.fetch_add(1, Ordering::Relaxed);
                }
            });
        let files_in_batch = batch.file_registrations.len();
        let files_failed = failed.len() + registration_failures.load(Ordering::Relaxed);
        stats.files_indexed += files_in_batch - files_failed;
        stats.files_failed += files_failed;

        // Write imports in parallel
        batch.imports.par_iter().for_each(|import| {
            if let Err(e) = self.index.store_import(import) {
                tracing::warn!(
                    target: "pipeline",
                    "Failed to store import {}: {e}",
                    import.path
                );
            }
        });

        // Update progress AFTER all work is complete
        // This ensures 100% only shows when files are truly fully processed
        if let Some(ref progress) = self.progress {
            for _ in 0..files_in_batch {
                progress.inc();
            }
            progress.add_extra1(files_in_batch as u64);
        }

        // Report via callback (for dual progress bar)
        if let Some(ref callback) = self.progress_callback {
            callback(files_in_batch as u64);
        }

        Ok(files_failed)
    }

    /// Commit current batch and start a new one.
    fn commit_and_restart(&self) -> PipelineResult<()> {
        self.index.commit_batch()?;
        self.index.start_batch()?;
        Ok(())
    }

    /// Stage the id-counter high-water mark into the open batch so docs
    /// and counters become durable in ONE commit. A run that dies after
    /// any commit leaves the persisted counters at or above every
    /// committed id; the next generation cannot re-issue a live id.
    fn persist_counters(&self, file: u32, symbol: u32) -> PipelineResult<()> {
        use crate::storage::MetadataKey;
        if file > 0 {
            self.index
                .store_metadata(MetadataKey::FileCounter, u64::from(file))?;
        }
        if symbol > 0 {
            self.index
                .store_metadata(MetadataKey::SymbolCounter, u64::from(symbol))?;
        }
        Ok(())
    }

    /// Index a single batch (for single-file indexing).
    ///
    /// [PIPELINE API] Used by `Pipeline::index_file_single()` for watcher reindex.
    /// Caller must handle start_batch/commit_batch.
    pub fn index_batch(&self, batch: IndexBatch) -> PipelineResult<()> {
        let symbol_cache = SymbolLookupCache::new();
        let mut stats = IndexStats::new();
        let failed = self.process_batch(batch, &mut stats, &symbol_cache)?;
        if failed > 0 {
            return Err(PipelineError::Parse {
                path: PathBuf::new(),
                reason: format!(
                    "{failed} file(s) had failed index writes and were left unregistered"
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolKind;
    use crate::config::Settings;
    use crate::indexing::pipeline::types::FileRegistration;
    use crate::parsing::LanguageId;
    use crate::symbol::Symbol;
    use crate::types::{FileId, Range, SymbolId};
    use crossbeam_channel::bounded;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_test_symbol(id: u32, name: &str, file_id: u32) -> Symbol {
        Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(file_id).unwrap(),
            Range::new(1, 0, 1, 10),
        )
        .with_file_path(format!("test_{file_id}.rs"))
    }

    fn make_test_batch(file_id: u32, symbol_count: usize) -> IndexBatch {
        let mut batch = IndexBatch::new();

        batch.file_registrations.push(FileRegistration {
            path: PathBuf::from(format!("test_{file_id}.rs")),
            file_id: FileId::new(file_id).unwrap(),
            content_hash: "abc123def456".to_string(),
            language_id: LanguageId::new("rust"),
            timestamp: 1700000000,
            mtime: 1700000000,
        });

        for i in 0..symbol_count {
            let sym_id = (file_id - 1) * symbol_count as u32 + i as u32 + 1;
            let symbol = make_test_symbol(sym_id, &format!("sym_{sym_id}"), file_id);
            batch.symbols.push(symbol);
        }

        batch
    }

    #[test]
    fn test_index_stage_writes_symbols() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);

        // Send a batch with symbols
        let batch = make_test_batch(1, 3);
        batch_tx.send(batch).unwrap();
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 10);
        let result = stage.run(batch_rx);

        assert!(result.is_ok());
        let (stats, rels, symbol_cache, _) = result.unwrap();

        println!(
            "Indexed {} files, {} symbols, cache has {} entries",
            stats.files_indexed,
            stats.symbols_found,
            symbol_cache.len()
        );

        assert_eq!(stats.files_indexed, 1);
        assert_eq!(stats.symbols_found, 3);
        assert!(rels.is_empty());
        assert_eq!(symbol_cache.len(), 3);
    }

    #[test]
    fn test_index_stage_commits_every_n_batches() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(20);

        // Send 5 batches
        for i in 1..=5 {
            batch_tx.send(make_test_batch(i, 2)).unwrap();
        }
        drop(batch_tx);

        // Commit every 2 batches
        let stage = IndexStage::new(Arc::clone(&index), 2);
        let result = stage.run(batch_rx);

        assert!(result.is_ok());
        let (stats, _, symbol_cache, _) = result.unwrap();

        println!(
            "Indexed {} files, {} symbols with batches_per_commit=2, cache has {} entries",
            stats.files_indexed,
            stats.symbols_found,
            symbol_cache.len()
        );

        assert_eq!(stats.files_indexed, 5);
        assert_eq!(stats.symbols_found, 10);
        assert_eq!(symbol_cache.len(), 10);
    }

    /// Crash-window regression: docs must never be durable above the
    /// persisted counters. Run the stage WITHOUT the pipeline's tail
    /// save_final_counters; the stored counters must already cover
    /// every committed id.
    #[test]
    fn counters_are_durable_at_every_commit_without_tail_save() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);
        batch_tx.send(make_test_batch(1, 3)).unwrap(); // symbol ids 1..=3
        batch_tx.send(make_test_batch(2, 3)).unwrap(); // symbol ids 4..=6
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 1);
        stage.run(batch_rx).unwrap();

        assert_eq!(
            index.get_next_symbol_id().unwrap(),
            7,
            "symbol counter must cover every committed id without a tail save"
        );
        assert_eq!(
            index.get_next_file_id().unwrap(),
            3,
            "file counter must cover every committed registration"
        );
    }

    /// A run whose batches carry ids below the starting counters (an
    /// incremental run over an existing index) must not regress the
    /// persisted counters below the floor.
    #[test]
    fn counter_floor_prevents_regression_below_prior_generations() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);
        batch_tx.send(make_test_batch(1, 2)).unwrap(); // file id 1, symbol ids 1..=2
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 1).with_counter_floor(10, 20);
        stage.run(batch_rx).unwrap();

        assert_eq!(index.get_next_file_id().unwrap(), 11);
        assert_eq!(index.get_next_symbol_id().unwrap(), 21);
    }

    #[test]
    fn test_index_stage_accumulates_relationships() {
        use crate::RelationKind;
        use std::sync::Arc as StdArc;

        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);

        // Create batch with relationships
        let mut batch = make_test_batch(1, 2);
        batch.unresolved_relationships.push(UnresolvedRelationship {
            from_id: Some(SymbolId::new(1).unwrap()),
            from_name: StdArc::from("caller"),
            to_name: StdArc::from("callee"),
            file_id: FileId::new(1).unwrap(),
            kind: RelationKind::Calls,
            metadata: None,
            to_range: None,
        });

        batch_tx.send(batch).unwrap();
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 10);
        let result = stage.run(batch_rx);

        assert!(result.is_ok());
        let (stats, rels, symbol_cache, _) = result.unwrap();

        println!(
            "Accumulated {} relationships for Phase 2, cache has {} symbols",
            rels.len(),
            symbol_cache.len()
        );

        assert_eq!(stats.files_indexed, 1);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].from_name.as_ref(), "caller");
        assert_eq!(rels[0].to_name.as_ref(), "callee");
        assert_eq!(symbol_cache.len(), 2);
    }

    #[test]
    fn test_symbol_cache_lookup_by_name() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);

        // Create batch with specific symbol names
        let mut batch = IndexBatch::new();
        batch.file_registrations.push(FileRegistration {
            path: PathBuf::from("test.rs"),
            file_id: FileId::new(1).unwrap(),
            content_hash: "abc123def456".to_string(),
            language_id: LanguageId::new("rust"),
            timestamp: 1700000000,
            mtime: 1700000000,
        });

        // Add symbols with known names
        let sym1 = make_test_symbol(1, "process_data", 1);
        let sym2 = make_test_symbol(2, "process_data", 1); // Duplicate name, different ID
        let sym3 = make_test_symbol(3, "validate_input", 1);

        batch.symbols.push(sym1);
        batch.symbols.push(sym2);
        batch.symbols.push(sym3);

        batch_tx.send(batch).unwrap();
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 10);
        let (_, _, symbol_cache, _) = stage.run(batch_rx).unwrap();

        // Verify lookup by name returns correct candidates
        let candidates = symbol_cache.lookup_candidates("process_data");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.contains(&SymbolId::new(1).unwrap()));
        assert!(candidates.contains(&SymbolId::new(2).unwrap()));

        let validate_candidates = symbol_cache.lookup_candidates("validate_input");
        assert_eq!(validate_candidates.len(), 1);
        assert!(validate_candidates.contains(&SymbolId::new(3).unwrap()));

        // Non-existent name returns empty
        let missing = symbol_cache.lookup_candidates("nonexistent");
        assert!(missing.is_empty());

        // Verify direct ID lookup
        let sym = symbol_cache.get(SymbolId::new(1).unwrap());
        assert!(sym.is_some());
        assert_eq!(&*sym.unwrap().name, "process_data");

        println!(
            "Cache: {} symbols, {} unique names",
            symbol_cache.len(),
            symbol_cache.unique_names()
        );
    }
}
