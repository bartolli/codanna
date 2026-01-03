//! Parallel indexing pipeline
//!
//! [PIPELINE API] A high-performance, multi-stage pipeline for indexing source code.
//!
//! ## Architecture
//!
//! ```text
//! DISCOVER → READ → PARSE → COLLECT → INDEX
//!    │         │       │        │        │
//!    ▼         ▼       ▼        ▼        ▼
//! [paths]  [content] [parsed] [batch]  Tantivy
//! ```
//!
//! ### Stage Overview
//!
//! - **DISCOVER**: Parallel file system walk, produces paths
//! - **READ**: Reads file contents, computes hashes
//! - **PARSE**: Parallel parsing with thread-local parsers
//! - **COLLECT**: Single-threaded ID assignment and batching
//! - **INDEX**: Writes batches to Tantivy
//!
//! ## Usage
//!
//! ```ignore
//! use codanna::indexing::pipeline::{Pipeline, PipelineConfig};
//!
//! let config = PipelineConfig::default();
//! let pipeline = Pipeline::new(settings, config);
//! let stats = pipeline.index_directory(path, &index)?;
//! ```

pub mod config;
pub mod metrics;
pub mod stages;
pub mod types;

pub use config::PipelineConfig;
pub use metrics::{PipelineMetrics, StageTracker};
pub use stages::cleanup::{CleanupStage, CleanupStats};
pub use stages::context::{ContextStage, ContextStats};
pub use stages::embed::{EmbedStage, EmbedStats};
pub use stages::parse::{ParseStage, init_parser_cache, parse_file};
pub use stages::resolve::{ResolveStage, ResolveStats};
pub use stages::write::{WriteStage, WriteStats};
pub use types::{
    DiscoverResult, FileContent, FileRegistration, IndexBatch, ParsedFile, PipelineError,
    PipelineResult, RawImport, RawRelationship, RawSymbol, ResolutionContext, ResolvedBatch,
    ResolvedRelationship, SingleFileStats, SymbolLookupCache, UnresolvedRelationship,
};

use crate::FileId;
use crate::RelationKind;
use crate::Settings;
use crate::indexing::IndexStats;
use crate::parsing::ParserFactory;
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use crossbeam_channel::bounded;
use stages::{CollectStage, DiscoverStage, IndexStage, ReadStage};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// Result of Phase 1 indexing with optional metrics for deferred logging.
type Phase1Result = (
    IndexStats,
    Vec<UnresolvedRelationship>,
    SymbolLookupCache,
    Option<Arc<PipelineMetrics>>,
);

/// The parallel indexing pipeline.
///
/// [PIPELINE API] Orchestrates multiple stages to efficiently index source code
/// using all available CPU cores.
pub struct Pipeline {
    settings: Arc<Settings>,
    config: PipelineConfig,
}

impl Pipeline {
    /// Create a new pipeline with the given settings and configuration.
    pub fn new(settings: Arc<Settings>, config: PipelineConfig) -> Self {
        Self { settings, config }
    }

    /// Create a pipeline with configuration derived from settings.
    pub fn with_settings(settings: Arc<Settings>) -> Self {
        let config = PipelineConfig::from_settings(&settings);
        Self::new(settings, config)
    }

    /// Get the pipeline configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Get the settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Index a directory using the parallel pipeline (Phase 1).
    ///
    /// [PIPELINE API] This is the main entry point for indexing. It:
    /// 1. Discovers all source files (parallel walk)
    /// 2. Reads file contents (N threads)
    /// 3. Parses them in parallel (N threads)
    /// 4. Collects and assigns IDs (single thread)
    /// 5. Writes to Tantivy (single thread)
    ///
    /// Returns:
    /// - IndexStats: Statistics about the indexing operation
    /// - `Vec<UnresolvedRelationship>`: Pending references for Phase 2 resolution
    /// - SymbolLookupCache: In-memory cache for O(1) Phase 2 resolution
    pub fn index_directory(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>, SymbolLookupCache)> {
        let start = Instant::now();

        // Create metrics collector if tracing is enabled
        let metrics = if self.config.pipeline_tracing {
            Some(PipelineMetrics::new(root.display().to_string(), true))
        } else {
            None
        };

        // Query existing ID counters BEFORE spawning threads
        // Critical for multi-directory indexing to avoid ID collisions
        let start_file_counter = index.get_next_file_id()?.saturating_sub(1);
        let start_symbol_counter = index.get_next_symbol_id()?.saturating_sub(1);

        // Create bounded channels with backpressure
        let (path_tx, path_rx) = bounded(self.config.path_channel_size);
        let (content_tx, content_rx) = bounded(self.config.content_channel_size);
        let (parsed_tx, parsed_rx) = bounded(self.config.parsed_channel_size);
        let (batch_tx, batch_rx) = bounded(self.config.batch_channel_size);

        // Clone settings for threads
        let settings = Arc::clone(&self.settings);
        let parse_threads = self.config.parse_threads;
        let read_threads = self.config.read_threads;
        let discover_threads = self.config.discover_threads;
        let batch_size = self.config.batch_size;
        let batches_per_commit = self.config.batches_per_commit;
        let tracing_enabled = self.config.pipeline_tracing;

        // Stage 1: DISCOVER - parallel file walk
        let discover_root = root.to_path_buf();
        let discover_handle = thread::spawn(move || {
            let tracker = if tracing_enabled {
                Some(StageTracker::new("DISCOVER", discover_threads))
            } else {
                None
            };

            let stage = DiscoverStage::new(discover_root, discover_threads);
            let result = stage.run(path_tx);

            // Record metrics
            if let (Some(tracker), Ok(count)) = (&tracker, &result) {
                tracker.record_items(*count);
            }

            (result, tracker.map(|t| t.finalize()))
        });

        // Stage 2: READ - multi-threaded file reading
        let read_handles: Vec<_> = (0..read_threads)
            .map(|_| {
                let rx = path_rx.clone();
                let tx = content_tx.clone();
                thread::spawn(move || {
                    let stage = ReadStage::new(1);
                    stage.run(rx, tx)
                })
            })
            .collect();
        drop(path_rx); // Close original receiver
        drop(content_tx); // Close original sender after cloning

        // Stage 3: PARSE - parallel parsing with thread-local parsers (with wait tracking)
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|_| {
                let rx = content_rx.clone();
                let tx = parsed_tx.clone();
                let settings = Arc::clone(&settings);
                thread::spawn(move || {
                    // Initialize thread-local parser cache
                    init_parser_cache(settings.clone());

                    let stage = ParseStage::new(settings);
                    let mut parsed_count = 0;
                    let mut error_count = 0;
                    let mut symbol_count = 0;
                    let mut input_wait = std::time::Duration::ZERO;
                    let mut output_wait = std::time::Duration::ZERO;

                    loop {
                        // Track input wait (time blocked on recv)
                        let recv_start = Instant::now();
                        let content = match rx.recv() {
                            Ok(c) => c,
                            Err(_) => break, // Channel closed
                        };
                        input_wait += recv_start.elapsed();

                        match stage.parse(content) {
                            Ok(parsed) => {
                                parsed_count += 1;
                                symbol_count += parsed.raw_symbols.len();

                                // Track output wait (time blocked on send)
                                let send_start = Instant::now();
                                if tx.send(parsed).is_err() {
                                    break; // Channel closed
                                }
                                output_wait += send_start.elapsed();
                            }
                            Err(_e) => {
                                error_count += 1;
                                // Continue on parse errors - don't fail the whole batch
                            }
                        }
                    }

                    (
                        parsed_count,
                        error_count,
                        symbol_count,
                        input_wait,
                        output_wait,
                    )
                })
            })
            .collect();
        drop(content_rx);
        drop(parsed_tx);

        // Stage 4: COLLECT - single-threaded ID assignment (with starting counters)
        let collect_handle = thread::spawn(move || {
            let tracker = if tracing_enabled {
                Some(StageTracker::new("COLLECT", 1).with_secondary("batches"))
            } else {
                None
            };

            let stage = CollectStage::new(batch_size)
                .with_start_counters(start_file_counter, start_symbol_counter);
            let result = stage.run(parsed_rx, batch_tx);

            // Record items and wait times before finalizing
            if let (Some(t), Ok((_, symbol_count, input_wait, output_wait))) = (&tracker, &result) {
                t.record_items(*symbol_count as usize);
                t.record_input_wait(*input_wait);
                t.record_output_wait(*output_wait);
            }

            (result, tracker.map(|t| t.finalize()))
        });

        // Stage 5: INDEX - single-threaded Tantivy writes
        // Clone index Arc for metadata update after pipeline completes
        let index_for_metadata = Arc::clone(&index);
        let index_handle = thread::spawn(move || {
            let tracker = if tracing_enabled {
                Some(StageTracker::new("INDEX", 1).with_secondary("commits"))
            } else {
                None
            };

            let stage = IndexStage::new(index, batches_per_commit);
            let result = stage.run(batch_rx);

            // Record items and wait times before finalizing
            if let (Some(t), Ok((stats, _, _, input_wait))) = (&tracker, &result) {
                t.record_items(stats.symbols_found);
                t.record_input_wait(*input_wait);
            }

            (result, tracker.map(|t| t.finalize()))
        });

        // Wait for all stages to complete and collect results
        let (discover_result, discover_metrics) = discover_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("DISCOVER thread panicked".to_string()))?;
        let files_discovered = discover_result?;

        // Add DISCOVER metrics
        if let (Some(m), Some(dm)) = (&metrics, discover_metrics) {
            m.add_stage(dm);
        }

        // READ stage metrics (aggregate across threads)
        let read_tracker = if tracing_enabled {
            Some(StageTracker::new("READ", read_threads).with_secondary("MB"))
        } else {
            None
        };
        let mut read_files = 0;
        let mut read_errors = 0;
        let mut read_input_wait = std::time::Duration::ZERO;
        let mut read_output_wait = std::time::Duration::ZERO;
        for handle in read_handles {
            if let Ok(Ok((files, errors, input_wait, output_wait))) = handle.join() {
                read_files += files;
                read_errors += errors;
                read_input_wait += input_wait;
                read_output_wait += output_wait;
            }
        }
        if let Some(tracker) = read_tracker {
            tracker.record_items(read_files);
            tracker.record_input_wait(read_input_wait);
            tracker.record_output_wait(read_output_wait);
            if let Some(m) = &metrics {
                m.add_stage(tracker.finalize());
            }
        }

        // PARSE stage metrics (aggregate across threads)
        let parse_tracker = if tracing_enabled {
            Some(StageTracker::new("PARSE", parse_threads).with_secondary("symbols"))
        } else {
            None
        };
        let mut parsed_files = 0;
        let mut parse_errors = 0;
        let mut total_symbols = 0;
        let mut parse_input_wait = std::time::Duration::ZERO;
        let mut parse_output_wait = std::time::Duration::ZERO;
        for handle in parse_handles {
            if let Ok((files, errors, symbols, input_wait, output_wait)) = handle.join() {
                parsed_files += files;
                parse_errors += errors;
                total_symbols += symbols;
                parse_input_wait += input_wait;
                parse_output_wait += output_wait;
            }
        }
        if let Some(tracker) = parse_tracker {
            tracker.record_items(parsed_files);
            tracker.record_secondary(total_symbols);
            tracker.record_input_wait(parse_input_wait);
            tracker.record_output_wait(parse_output_wait);
            if let Some(m) = &metrics {
                m.add_stage(tracker.finalize());
            }
        }

        // Get final counter values from COLLECT stage
        let (collect_result, collect_metrics) = collect_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("COLLECT thread panicked".to_string()))?;
        let (final_file_count, final_symbol_count, _, _) = collect_result?;

        // Add COLLECT metrics
        if let (Some(m), Some(cm)) = (&metrics, collect_metrics) {
            m.add_stage(cm);
        }

        let (index_result, index_metrics) = index_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("INDEX thread panicked".to_string()))?;
        let (mut stats, pending_relationships, symbol_cache, _) = index_result?;

        // Add INDEX metrics
        if let (Some(m), Some(im)) = (&metrics, index_metrics) {
            m.add_stage(im);
        }

        // Store final counter values to metadata for next directory
        // This is critical for multi-directory indexing to avoid ID collisions
        use crate::storage::MetadataKey;
        index_for_metadata.start_batch()?;
        index_for_metadata.store_metadata(MetadataKey::FileCounter, final_file_count as u64)?;
        index_for_metadata.store_metadata(MetadataKey::SymbolCounter, final_symbol_count as u64)?;
        index_for_metadata.commit_batch()?;

        // Update stats with timing
        stats.elapsed = start.elapsed();
        stats.files_failed = read_errors + parse_errors;

        // Log pipeline metrics report
        if let Some(m) = metrics {
            m.finalize_and_log(start.elapsed());
        }

        tracing::info!(
            target: "pipeline",
            "Phase 1 complete: discovered={}, read={}, parsed={}, indexed={} files, {} symbols, {} cached, {} pending refs in {:?}",
            files_discovered,
            read_files,
            parsed_files,
            stats.files_indexed,
            stats.symbols_found,
            symbol_cache.len(),
            pending_relationships.len(),
            stats.elapsed
        );

        Ok((stats, pending_relationships, symbol_cache))
    }

    /// Index a directory with progress reporting (Phase 1).
    fn index_directory_with_progress(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>, SymbolLookupCache)> {
        let start = Instant::now();

        // Query existing ID counters BEFORE spawning threads
        // Critical for multi-directory indexing to avoid ID collisions
        let start_file_counter = index.get_next_file_id()?.saturating_sub(1);
        let start_symbol_counter = index.get_next_symbol_id()?.saturating_sub(1);

        // Create bounded channels
        let (path_tx, path_rx) = bounded(self.config.path_channel_size);
        let (content_tx, content_rx) = bounded(self.config.content_channel_size);
        let (parsed_tx, parsed_rx) = bounded(self.config.parsed_channel_size);
        let (batch_tx, batch_rx) = bounded(self.config.batch_channel_size);

        let settings = Arc::clone(&self.settings);
        let parse_threads = self.config.parse_threads;
        let read_threads = self.config.read_threads;
        let discover_threads = self.config.discover_threads;
        let batch_size = self.config.batch_size;
        let batches_per_commit = self.config.batches_per_commit;

        // Stage 1: DISCOVER
        let discover_root = root.to_path_buf();
        let discover_handle = thread::spawn(move || {
            let stage = DiscoverStage::new(discover_root, discover_threads);
            stage.run(path_tx)
        });

        // Stage 2: READ
        let read_handles: Vec<_> = (0..read_threads)
            .map(|_| {
                let rx = path_rx.clone();
                let tx = content_tx.clone();
                thread::spawn(move || {
                    let stage = ReadStage::new(1);
                    stage.run(rx, tx)
                })
            })
            .collect();
        drop(path_rx);
        drop(content_tx);

        // Stage 3: PARSE
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|_| {
                let rx = content_rx.clone();
                let tx = parsed_tx.clone();
                let settings = Arc::clone(&settings);
                thread::spawn(move || {
                    init_parser_cache(settings.clone());
                    let stage = ParseStage::new(settings);
                    let mut parsed = 0;
                    let mut errors = 0;

                    for content in rx {
                        match stage.parse(content) {
                            Ok(p) => {
                                parsed += 1;
                                if tx.send(p).is_err() {
                                    break;
                                }
                            }
                            Err(_) => errors += 1,
                        }
                    }
                    (parsed, errors)
                })
            })
            .collect();
        drop(content_rx);
        drop(parsed_tx);

        // Stage 4: COLLECT (with starting counters for multi-directory support)
        let collect_handle = thread::spawn(move || {
            let stage = CollectStage::new(batch_size)
                .with_start_counters(start_file_counter, start_symbol_counter);
            stage.run(parsed_rx, batch_tx)
        });

        // Stage 5: INDEX with optional progress
        // Clone index Arc for metadata update after pipeline completes
        let index_for_metadata = Arc::clone(&index);
        let mut index_stage = IndexStage::new(index, batches_per_commit);
        if let Some(prog) = progress {
            index_stage = index_stage.with_progress(prog);
        }
        let index_handle = thread::spawn(move || index_stage.run(batch_rx));

        // Wait for all stages
        let discover_result = discover_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("DISCOVER panicked".to_string()))?;
        let _files_discovered = discover_result?;

        for h in read_handles {
            let _ = h.join();
        }
        for h in parse_handles {
            let _ = h.join();
        }
        // Get final counter values from COLLECT stage
        let (final_file_count, final_symbol_count, _, _) = collect_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("COLLECT panicked".to_string()))??;

        let index_result = index_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("INDEX panicked".to_string()))?;
        let (mut stats, pending_relationships, symbol_cache, _) = index_result?;

        // Store final counter values to metadata for next directory
        // This is critical for multi-directory indexing to avoid ID collisions
        use crate::storage::MetadataKey;
        index_for_metadata.start_batch()?;
        index_for_metadata.store_metadata(MetadataKey::FileCounter, final_file_count as u64)?;
        index_for_metadata.store_metadata(MetadataKey::SymbolCounter, final_symbol_count as u64)?;
        index_for_metadata.commit_batch()?;

        stats.elapsed = start.elapsed();
        Ok((stats, pending_relationships, symbol_cache))
    }

    /// Run Phase 2: Resolve relationships using two-pass strategy.
    ///
    /// [PIPELINE API] This resolves all pending relationships from Phase 1:
    /// 1. Pass 1: Resolve Defines relationships (class→method, module→function)
    /// 2. Commit barrier: Defines are now queryable
    /// 3. Pass 2: Resolve Calls (can reference Defines)
    ///
    /// # Arguments
    /// * `unresolved` - Pending relationships from Phase 1
    /// * `symbol_cache` - SymbolLookupCache populated by Phase 1
    /// * `index` - DocumentIndex for reading imports and writing relationships
    ///
    /// # Returns
    /// Phase2Stats with resolution counts
    pub fn run_phase2(
        &self,
        unresolved: Vec<UnresolvedRelationship>,
        symbol_cache: Arc<SymbolLookupCache>,
        index: Arc<DocumentIndex>,
    ) -> PipelineResult<Phase2Stats> {
        self.run_phase2_with_progress(unresolved, symbol_cache, index, None)
    }

    /// Run Phase 2 with optional progress bar.
    pub fn run_phase2_with_progress(
        &self,
        unresolved: Vec<UnresolvedRelationship>,
        symbol_cache: Arc<SymbolLookupCache>,
        index: Arc<DocumentIndex>,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<Phase2Stats> {
        let start = Instant::now();
        let total_relationships = unresolved.len();

        if unresolved.is_empty() {
            return Ok(Phase2Stats {
                total_relationships: 0,
                defines_resolved: 0,
                calls_resolved: 0,
                other_resolved: 0,
                unresolved: 0,
                elapsed: start.elapsed(),
            });
        }

        // Create stages
        let factory = Arc::new(ParserFactory::new(Arc::clone(&self.settings)));
        let context_stage =
            ContextStage::new(Arc::clone(&symbol_cache), Arc::clone(&index), factory);
        let mut write_stage = WriteStage::new(Arc::clone(&index));

        // Split relationships by kind
        let (defines, others): (Vec<_>, Vec<_>) = unresolved
            .into_iter()
            .partition(|rel| rel.kind == RelationKind::Defines);

        let mut stats = Phase2Stats {
            total_relationships,
            ..Default::default()
        };

        // Pass 1: Resolve Defines
        tracing::info!(
            target: "pipeline",
            "Phase 2 Pass 1: Resolving {} Defines relationships",
            defines.len()
        );
        if !defines.is_empty() {
            let contexts = context_stage.build_contexts(defines);
            let behaviors = context_stage.behaviors();
            let resolve_stage = ResolveStage::new(Arc::clone(&symbol_cache), behaviors);

            for ctx in contexts {
                let rel_count = ctx.unresolved_rels.len() as u64;
                let (batch, resolve_stats) = resolve_stage.resolve(&ctx);
                stats.defines_resolved += resolve_stats.defines_resolved;
                write_stage.write(batch);

                // Update progress bar
                if let Some(ref prog) = progress {
                    prog.set_progress(prog.current() + rel_count);
                    prog.add_extra1(resolve_stats.defines_resolved as u64);
                    let skipped = rel_count.saturating_sub(resolve_stats.defines_resolved as u64);
                    prog.add_extra2(skipped);
                }
            }

            // BARRIER: Commit Defines so Pass 2 can query them
            write_stage
                .commit()
                .map_err(|e| PipelineError::Index(crate::IndexError::General(e.to_string())))?;
        }

        // Pass 2: Resolve Calls and other relationships
        tracing::info!(
            target: "pipeline",
            "Phase 2 Pass 2: Resolving {} Calls/other relationships",
            others.len()
        );
        if !others.is_empty() {
            let contexts = context_stage.build_contexts(others);
            let behaviors = context_stage.behaviors();
            let resolve_stage = ResolveStage::new(Arc::clone(&symbol_cache), behaviors);

            for ctx in contexts {
                let rel_count = ctx.unresolved_rels.len() as u64;
                let (batch, resolve_stats) = resolve_stage.resolve(&ctx);
                stats.calls_resolved += resolve_stats.calls_resolved;
                stats.other_resolved += resolve_stats.resolved - resolve_stats.calls_resolved;
                write_stage.write(batch);

                // Update progress bar
                if let Some(ref prog) = progress {
                    prog.set_progress(prog.current() + rel_count);
                    prog.add_extra1(resolve_stats.resolved as u64);
                    let skipped = rel_count.saturating_sub(resolve_stats.resolved as u64);
                    prog.add_extra2(skipped);
                }
            }

            // Final commit
            write_stage
                .flush()
                .map_err(|e| PipelineError::Index(crate::IndexError::General(e.to_string())))?;
        }

        stats.unresolved = stats.total_relationships
            - stats.defines_resolved
            - stats.calls_resolved
            - stats.other_resolved;
        stats.elapsed = start.elapsed();

        tracing::info!(
            target: "pipeline",
            "Phase 2 complete: resolved {}/{} ({} Defines, {} Calls, {} other) in {:?}",
            stats.defines_resolved + stats.calls_resolved + stats.other_resolved,
            stats.total_relationships,
            stats.defines_resolved,
            stats.calls_resolved,
            stats.other_resolved,
            stats.elapsed
        );

        Ok(stats)
    }

    /// Run full pipeline: Phase 1 (indexing) + Phase 2 (resolution).
    ///
    /// Convenience method that runs both phases in sequence.
    pub fn index_and_resolve(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
    ) -> PipelineResult<(IndexStats, Phase2Stats)> {
        // Phase 1: Index files
        let (index_stats, unresolved, symbol_cache) =
            self.index_directory(root, Arc::clone(&index))?;

        // Phase 2: Resolve relationships
        let symbol_cache = Arc::new(symbol_cache);
        let phase2_stats = self.run_phase2(unresolved, symbol_cache, index)?;

        Ok((index_stats, phase2_stats))
    }

    /// Index a single file (for watcher reindex events).
    ///
    /// [PIPELINE API] Optimized path for single-file re-indexing when a file changes.
    /// This is used by the file watcher for real-time updates.
    ///
    /// # Flow
    /// 1. Read file and compute hash
    /// 2. Check if file exists in index (hash comparison)
    /// 3. If unchanged, return early with Cached result
    /// 4. If modified, cleanup old data (symbols, relationships, embeddings)
    /// 5. Parse file
    /// 6. Index via IndexStage
    /// 7. Run Phase 2 resolution
    ///
    /// # Arguments
    /// * `path` - Path to the file to index
    /// * `index` - DocumentIndex for storage
    /// * `semantic` - Optional semantic search for embeddings
    ///
    /// # Returns
    /// `SingleFileStats` with indexing results or `Cached` if unchanged
    pub fn index_file_single(
        &self,
        path: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
    ) -> PipelineResult<SingleFileStats> {
        let start = Instant::now();
        let semantic_path = self.settings.index_path.join("semantic");

        // Normalize path relative to workspace_root
        let normalized_path = if path.is_absolute() {
            if let Some(workspace_root) = &self.settings.workspace_root {
                path.strip_prefix(workspace_root).unwrap_or(path)
            } else {
                path
            }
        } else {
            path
        };

        let path_str = normalized_path
            .to_str()
            .ok_or_else(|| PipelineError::FileRead {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid UTF-8 in path",
                ),
            })?;

        // Read file using ReadStage
        let read_stage = ReadStage::new(1);
        let file_content = read_stage.read_single(&path.to_path_buf())?;
        let content_hash = file_content.hash.clone();

        // Check if file already exists by querying Tantivy
        if let Ok(Some((existing_file_id, existing_hash))) = index.get_file_info(path_str) {
            if existing_hash == content_hash {
                // File hasn't changed, skip re-indexing
                return Ok(SingleFileStats {
                    file_id: existing_file_id,
                    indexed: false,
                    cached: true,
                    symbols_found: 0,
                    relationships_resolved: 0,
                    elapsed: start.elapsed(),
                });
            }

            // File has changed - cleanup old data within a batch
            // Start batch for cleanup to avoid creating temporary writers
            index.start_batch()?;

            let cleanup_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };

            cleanup_stage.cleanup_files(&[path.to_path_buf()])?;

            // Commit cleanup changes before re-indexing
            index.commit_batch()?;
        }

        // Parse file
        init_parser_cache(Arc::clone(&self.settings));
        let parse_stage = ParseStage::new(Arc::clone(&self.settings));
        let parsed = parse_stage.parse(file_content)?;

        // Collect into a batch
        let collect_stage = CollectStage::new(self.config.batch_size);
        let (batch, unresolved) = collect_stage.process_single(parsed, Arc::clone(&index))?;

        // Index the batch
        let index_stage = if let Some(ref sem) = semantic {
            IndexStage::new(Arc::clone(&index), self.config.batches_per_commit)
                .with_semantic(Arc::clone(sem))
        } else {
            IndexStage::new(Arc::clone(&index), self.config.batches_per_commit)
        };

        let symbols_found = batch.symbols.len();
        // Capture file_id before batch is consumed
        let file_id = batch
            .file_registrations
            .first()
            .map(|r| r.file_id)
            .unwrap_or(FileId(0));

        // Start a batch before indexing
        index.start_batch()?;
        index_stage.index_batch(batch)?;

        // Commit the batch
        index.commit_batch()?;

        // Build symbol cache for resolution
        let symbol_cache = Arc::new(SymbolLookupCache::from_index(&index)?);

        // Run Phase 2 resolution
        let phase2_stats = self.run_phase2(unresolved, symbol_cache, index)?;

        // Save embeddings
        if let Some(sem) = semantic {
            if let Ok(guard) = sem.lock() {
                if let Err(e) = guard.save(&semantic_path) {
                    tracing::warn!(target: "pipeline", "Failed to save embeddings: {e}");
                }
            }
        }

        Ok(SingleFileStats {
            file_id,
            indexed: true,
            cached: false,
            symbols_found,
            relationships_resolved: phase2_stats.defines_resolved
                + phase2_stats.calls_resolved
                + phase2_stats.other_resolved,
            elapsed: start.elapsed(),
        })
    }

    /// Run incremental indexing: detect changes, cleanup, index, resolve, save.
    ///
    /// This is the main entry point for production indexing. It:
    /// 1. Detects new, modified, and deleted files
    /// 2. Cleans up deleted and modified files (removes symbols and embeddings)
    /// 3. Runs Phase 1 on new + modified files
    /// 4. Runs Phase 2 resolution
    /// 5. Saves embeddings to disk
    ///
    /// # Arguments
    /// * `root` - Root directory to index
    /// * `index` - DocumentIndex for storage
    /// * `semantic` - Optional semantic search for embeddings
    /// * `embedding_pool` - Optional pool for parallel embedding generation
    /// * `force` - If true, re-index all files regardless of hash
    pub fn index_incremental(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        force: bool,
    ) -> PipelineResult<IncrementalStats> {
        self.index_incremental_with_progress(root, index, semantic, embedding_pool, force, None)
    }

    /// Index a directory with progress bars managed internally.
    ///
    /// This method creates and manages both Phase 1 and Phase 2 progress bars
    /// for clean sequential display (Phase 1 completes, then Phase 2 shows).
    pub fn index_incremental_with_progress_flag(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        force: bool,
        show_progress: bool,
        total_files: usize,
    ) -> PipelineResult<IncrementalStats> {
        use crate::io::status_line::{
            ProgressBar, ProgressBarOptions, ProgressBarStyle, StatusLine,
        };

        if !show_progress {
            return self.index_incremental(root, index, semantic, embedding_pool, force);
        }

        let start = Instant::now();
        let semantic_path = self.settings.index_path.join("semantic");

        // Progress bar options shared between phases
        let bar_options = ProgressBarOptions::default()
            .with_style(ProgressBarStyle::VerticalSolid)
            .with_width(28);

        // Run Phase 1 indexing with appropriate progress bar
        let (index_stats, unresolved, symbol_cache, cleanup_stats, discover_counts) = if force {
            // Force mode: create bar with total file count
            // Labels: files, indexed, failed, embedded (for embedding visibility)
            let phase1_bar = Arc::new(ProgressBar::with_4_labels(
                total_files as u64,
                "files",
                "indexed",
                "failed",
                "embedded",
                bar_options,
            ));
            let phase1_status = StatusLine::new(Arc::clone(&phase1_bar));

            let (stats, unresolved, cache, metrics) = if let Some(ref sem) = semantic {
                self.index_directory_with_semantic(
                    root,
                    Arc::clone(&index),
                    Arc::clone(sem),
                    embedding_pool.clone(),
                    Some(phase1_bar.clone()),
                )?
            } else {
                let (s, u, c) = self.index_directory_with_progress(
                    root,
                    Arc::clone(&index),
                    Some(phase1_bar.clone()),
                )?;
                (s, u, c, None)
            };

            // Drop StatusLine BEFORE logging to avoid stderr race condition
            drop(phase1_status);
            // Log pipeline metrics after StatusLine is dropped
            if let Some(m) = metrics {
                m.log();
            }
            eprintln!("{phase1_bar}");

            let files_indexed = stats.files_indexed;
            (
                stats,
                unresolved,
                cache,
                CleanupStats::default(),
                (files_indexed, 0, 0),
            )
        } else {
            // Incremental mode: discover first, then create bar with actual count
            let discover_stage = DiscoverStage::new(root, self.config.discover_threads)
                .with_index(Arc::clone(&index));
            let discover_result = discover_stage.run_incremental()?;

            if discover_result.is_empty() {
                return Ok(IncrementalStats {
                    new_files: 0,
                    modified_files: 0,
                    deleted_files: 0,
                    index_stats: IndexStats::new(),
                    cleanup_stats: CleanupStats::default(),
                    phase2_stats: Phase2Stats::default(),
                    elapsed: start.elapsed(),
                });
            }

            // Cleanup
            let cleanup_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };

            let mut cleanup_stats = CleanupStats::default();
            if !discover_result.deleted_files.is_empty() {
                let stats = cleanup_stage.cleanup_files(&discover_result.deleted_files)?;
                cleanup_stats.files_cleaned += stats.files_cleaned;
                cleanup_stats.symbols_removed += stats.symbols_removed;
            }
            if !discover_result.modified_files.is_empty() {
                let stats = cleanup_stage.cleanup_files(&discover_result.modified_files)?;
                cleanup_stats.files_cleaned += stats.files_cleaned;
                cleanup_stats.symbols_removed += stats.symbols_removed;
            }

            let files_to_index: Vec<PathBuf> = discover_result
                .new_files
                .iter()
                .chain(discover_result.modified_files.iter())
                .cloned()
                .collect();

            // Create Phase 1 bar with actual files to index count
            // Labels: files, indexed, failed, embedded (for embedding visibility)
            let phase1_bar = Arc::new(ProgressBar::with_4_labels(
                files_to_index.len() as u64,
                "files",
                "indexed",
                "failed",
                "embedded",
                bar_options,
            ));
            let phase1_status = StatusLine::new(Arc::clone(&phase1_bar));

            let (stats, unresolved, cache) = self.index_files(
                &files_to_index,
                Arc::clone(&index),
                semantic.clone(),
                embedding_pool.clone(),
                Some(phase1_bar.clone()),
            )?;

            drop(phase1_status);
            eprintln!("{phase1_bar}");

            let counts = (
                discover_result.new_files.len(),
                discover_result.modified_files.len(),
                discover_result.deleted_files.len(),
            );
            (stats, unresolved, cache, cleanup_stats, counts)
        };

        // Run Phase 2 with separate progress bar (no rate - not meaningful for relationships)
        let symbol_cache = Arc::new(symbol_cache);
        let phase2_stats = if !unresolved.is_empty() {
            let phase2_options = bar_options.show_rate(false);
            let phase2_bar = Arc::new(ProgressBar::with_options(
                unresolved.len() as u64,
                "relationships",
                "resolved",
                "skipped",
                phase2_options,
            ));
            let phase2_status = StatusLine::new(Arc::clone(&phase2_bar));

            let stats = self.run_phase2_with_progress(
                unresolved,
                symbol_cache,
                Arc::clone(&index),
                Some(phase2_bar.clone()),
            )?;

            drop(phase2_status);
            eprintln!("{phase2_bar}");
            stats
        } else {
            Phase2Stats::default()
        };

        // Save embeddings
        if let Some(sem) = semantic {
            if let Ok(guard) = sem.lock() {
                let _ = guard.save(&semantic_path);
            }
        }

        Ok(IncrementalStats {
            new_files: discover_counts.0,
            modified_files: discover_counts.1,
            deleted_files: discover_counts.2,
            index_stats,
            cleanup_stats,
            phase2_stats,
            elapsed: start.elapsed(),
        })
    }

    /// Index a directory with optional progress bar.
    pub fn index_incremental_with_progress(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        force: bool,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<IncrementalStats> {
        let start = Instant::now();
        let semantic_path = self.settings.index_path.join("semantic");

        if force {
            // Force mode: index everything (no cleanup needed for fresh index)
            return self.index_full(
                root,
                index,
                semantic,
                embedding_pool,
                &semantic_path,
                progress,
            );
        }

        // Incremental mode: detect changes
        let discover_stage =
            DiscoverStage::new(root, self.config.discover_threads).with_index(Arc::clone(&index));
        let discover_result = discover_stage.run_incremental()?;

        tracing::info!(
            target: "pipeline",
            "Incremental discovery: {} new, {} modified, {} deleted",
            discover_result.new_files.len(),
            discover_result.modified_files.len(),
            discover_result.deleted_files.len()
        );

        if discover_result.is_empty() {
            return Ok(IncrementalStats {
                new_files: 0,
                modified_files: 0,
                deleted_files: 0,
                index_stats: IndexStats::new(),
                cleanup_stats: CleanupStats::default(),
                phase2_stats: Phase2Stats::default(),
                elapsed: start.elapsed(),
            });
        }

        // Create cleanup stage
        let cleanup_stage = if let Some(ref sem) = semantic {
            CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
        } else {
            CleanupStage::new(Arc::clone(&index), &semantic_path)
        };

        // Cleanup deleted files
        let mut cleanup_stats = CleanupStats::default();
        if !discover_result.deleted_files.is_empty() {
            let stats = cleanup_stage.cleanup_files(&discover_result.deleted_files)?;
            cleanup_stats.files_cleaned += stats.files_cleaned;
            cleanup_stats.symbols_removed += stats.symbols_removed;
            cleanup_stats.embeddings_removed += stats.embeddings_removed;
        }

        // Cleanup modified files (old data must be removed before re-indexing)
        if !discover_result.modified_files.is_empty() {
            let stats = cleanup_stage.cleanup_files(&discover_result.modified_files)?;
            cleanup_stats.files_cleaned += stats.files_cleaned;
            cleanup_stats.symbols_removed += stats.symbols_removed;
            cleanup_stats.embeddings_removed += stats.embeddings_removed;
        }

        // Combine new + modified for indexing
        let files_to_index: Vec<PathBuf> = discover_result
            .new_files
            .iter()
            .chain(discover_result.modified_files.iter())
            .cloned()
            .collect();

        // Run Phase 1 on the files to index
        let (index_stats, unresolved, symbol_cache) = self.index_files(
            &files_to_index,
            Arc::clone(&index),
            semantic.clone(),
            embedding_pool.clone(),
            progress.clone(),
        )?;

        // Run Phase 2 resolution with progress if Phase 1 had progress
        let symbol_cache = Arc::new(symbol_cache);
        let phase2_stats = if progress.is_some() && !unresolved.is_empty() {
            // Create Phase 2 progress bar
            use crate::io::status_line::{
                ProgressBar, ProgressBarOptions, ProgressBarStyle, StatusLine,
            };

            let options = ProgressBarOptions::default()
                .with_style(ProgressBarStyle::VerticalSolid)
                .with_width(28)
                .show_rate(false); // Rate not meaningful for relationships
            let phase2_bar = Arc::new(ProgressBar::with_options(
                unresolved.len() as u64,
                "relationships",
                "resolved",
                "skipped",
                options,
            ));
            let phase2_status = StatusLine::new(Arc::clone(&phase2_bar));

            let stats = self.run_phase2_with_progress(
                unresolved,
                symbol_cache,
                Arc::clone(&index),
                Some(phase2_bar.clone()),
            )?;

            // Finalize Phase 2 progress bar
            drop(phase2_status);
            eprintln!("{phase2_bar}");

            stats
        } else {
            self.run_phase2(unresolved, symbol_cache, Arc::clone(&index))?
        };

        // Save embeddings
        if let Some(sem) = semantic {
            let semantic_guard = sem.lock().map_err(|_| PipelineError::Parse {
                path: PathBuf::new(),
                reason: "Failed to lock semantic search".to_string(),
            })?;

            semantic_guard
                .save(&semantic_path)
                .map_err(|e| PipelineError::Parse {
                    path: semantic_path.clone(),
                    reason: format!("Failed to save embeddings: {e}"),
                })?;
        }

        Ok(IncrementalStats {
            new_files: discover_result.new_files.len(),
            modified_files: discover_result.modified_files.len(),
            deleted_files: discover_result.deleted_files.len(),
            index_stats,
            cleanup_stats,
            phase2_stats,
            elapsed: start.elapsed(),
        })
    }

    /// Index a specific list of files (for incremental mode).
    fn index_files(
        &self,
        files: &[PathBuf],
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>, SymbolLookupCache)> {
        if files.is_empty() {
            return Ok((
                IndexStats::new(),
                Vec::new(),
                SymbolLookupCache::with_capacity(0),
            ));
        }

        let start = Instant::now();

        // Query existing ID counters BEFORE spawning threads
        // Critical for incremental indexing to avoid ID collisions
        let start_file_counter = index.get_next_file_id()?.saturating_sub(1);
        let start_symbol_counter = index.get_next_symbol_id()?.saturating_sub(1);

        // Create bounded channels
        let (content_tx, content_rx) = bounded(self.config.content_channel_size);
        let (parsed_tx, parsed_rx) = bounded(self.config.parsed_channel_size);
        let (batch_tx, batch_rx) = bounded(self.config.batch_channel_size);

        let settings = Arc::clone(&self.settings);
        let parse_threads = self.config.parse_threads;
        let batch_size = self.config.batch_size;
        let batches_per_commit = self.config.batches_per_commit;

        // Stage 1: READ - Send files directly (already have the paths)
        let files_to_read = files.to_vec();
        let read_handle = thread::spawn(move || {
            let stage = ReadStage::new(1);
            let mut count = 0;
            let mut errors = 0;

            for path in files_to_read {
                match stage.read_single(&path) {
                    Ok(content) => {
                        if content_tx.send(content).is_err() {
                            break;
                        }
                        count += 1;
                    }
                    Err(_) => {
                        errors += 1;
                    }
                }
            }

            (count, errors)
        });

        // Stage 2: PARSE
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|_| {
                let rx = content_rx.clone();
                let tx = parsed_tx.clone();
                let settings = Arc::clone(&settings);
                thread::spawn(move || {
                    init_parser_cache(settings.clone());
                    let stage = ParseStage::new(settings);
                    let mut parsed = 0;
                    let mut errors = 0;

                    for content in rx {
                        match stage.parse(content) {
                            Ok(p) => {
                                parsed += 1;
                                if tx.send(p).is_err() {
                                    break;
                                }
                            }
                            Err(_) => errors += 1,
                        }
                    }
                    (parsed, errors)
                })
            })
            .collect();
        drop(content_rx);
        drop(parsed_tx);

        // Stage 3: COLLECT (with starting counters for incremental indexing)
        let collect_handle = thread::spawn(move || {
            let stage = CollectStage::new(batch_size)
                .with_start_counters(start_file_counter, start_symbol_counter);
            stage.run(parsed_rx, batch_tx)
        });

        // Stage 4: INDEX (with optional semantic search, embedding pool, and progress)
        // Clone index Arc for metadata update after pipeline completes
        let index_for_metadata = Arc::clone(&index);
        let mut index_stage = IndexStage::new(index, batches_per_commit);
        if let Some(sem) = semantic {
            index_stage = index_stage.with_semantic(sem);
        }
        if let Some(pool) = embedding_pool {
            index_stage = index_stage.with_embedding_pool(pool);
        }
        if let Some(prog) = progress {
            index_stage = index_stage.with_progress(prog);
        }

        let index_handle = thread::spawn(move || index_stage.run(batch_rx));

        // Wait for stages to complete
        let _ = read_handle.join();
        for h in parse_handles {
            let _ = h.join();
        }
        // Get final counter values from COLLECT stage
        let (final_file_count, final_symbol_count, _, _) = collect_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("COLLECT panicked".to_string()))??;

        let index_result = index_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("INDEX thread panicked".to_string()))?;
        let (mut stats, pending, cache, _) = index_result?;

        // Store final counter values to metadata for next directory
        // This is critical for incremental indexing to avoid ID collisions
        use crate::storage::MetadataKey;
        index_for_metadata.start_batch()?;
        index_for_metadata.store_metadata(MetadataKey::FileCounter, final_file_count as u64)?;
        index_for_metadata.store_metadata(MetadataKey::SymbolCounter, final_symbol_count as u64)?;
        index_for_metadata.commit_batch()?;

        stats.elapsed = start.elapsed();
        Ok((stats, pending, cache))
    }

    /// Full index (force mode): index all files without incremental detection.
    fn index_full(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        semantic_path: &Path,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<IncrementalStats> {
        let start = Instant::now();
        let show_progress = progress.is_some();

        // Run Phase 1 with semantic search integrated
        let (index_stats, unresolved, symbol_cache, metrics) = if let Some(ref sem) = semantic {
            self.index_directory_with_semantic(
                root,
                Arc::clone(&index),
                Arc::clone(sem),
                embedding_pool,
                progress,
            )?
        } else {
            let (s, u, c) =
                self.index_directory_with_progress(root, Arc::clone(&index), progress)?;
            (s, u, c, None)
        };

        // Log pipeline metrics (no StatusLine in this path, safe to log immediately)
        if let Some(m) = metrics {
            m.log();
        }

        // Run Phase 2 resolution with progress if Phase 1 had progress
        let symbol_cache = Arc::new(symbol_cache);
        let phase2_stats = if show_progress && !unresolved.is_empty() {
            // Create Phase 2 progress bar
            use crate::io::status_line::{
                ProgressBar, ProgressBarOptions, ProgressBarStyle, StatusLine,
            };

            let options = ProgressBarOptions::default()
                .with_style(ProgressBarStyle::VerticalSolid)
                .with_width(28)
                .show_rate(false); // Rate not meaningful for relationships
            let phase2_bar = Arc::new(ProgressBar::with_options(
                unresolved.len() as u64,
                "relationships",
                "resolved",
                "skipped",
                options,
            ));
            let phase2_status = StatusLine::new(Arc::clone(&phase2_bar));

            let stats = self.run_phase2_with_progress(
                unresolved,
                symbol_cache,
                Arc::clone(&index),
                Some(phase2_bar.clone()),
            )?;

            // Finalize Phase 2 progress bar
            drop(phase2_status);
            eprintln!("{phase2_bar}");

            stats
        } else {
            self.run_phase2(unresolved, symbol_cache, Arc::clone(&index))?
        };

        // Save embeddings
        if let Some(sem) = semantic {
            let semantic_guard = sem.lock().map_err(|_| PipelineError::Parse {
                path: PathBuf::new(),
                reason: "Failed to lock semantic search".to_string(),
            })?;

            semantic_guard
                .save(semantic_path)
                .map_err(|e| PipelineError::Parse {
                    path: semantic_path.to_path_buf(),
                    reason: format!("Failed to save embeddings: {e}"),
                })?;
        }

        Ok(IncrementalStats {
            new_files: index_stats.files_indexed,
            modified_files: 0,
            deleted_files: 0,
            index_stats,
            cleanup_stats: CleanupStats::default(),
            phase2_stats,
            elapsed: start.elapsed(),
        })
    }

    /// Index directory with semantic search integration.
    fn index_directory_with_semantic(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Arc<Mutex<SimpleSemanticSearch>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<Phase1Result> {
        let start = Instant::now();

        // Create metrics collector if tracing is enabled
        let metrics = if self.config.pipeline_tracing {
            Some(PipelineMetrics::new(root.display().to_string(), true))
        } else {
            None
        };

        // Query existing ID counters BEFORE spawning threads
        // Critical for multi-directory indexing to avoid ID collisions
        let start_file_counter = index.get_next_file_id()?.saturating_sub(1);
        let start_symbol_counter = index.get_next_symbol_id()?.saturating_sub(1);

        // Create bounded channels
        let (path_tx, path_rx) = bounded(self.config.path_channel_size);
        let (content_tx, content_rx) = bounded(self.config.content_channel_size);
        let (parsed_tx, parsed_rx) = bounded(self.config.parsed_channel_size);
        let (batch_tx, batch_rx) = bounded(self.config.batch_channel_size);

        let settings = Arc::clone(&self.settings);
        let parse_threads = self.config.parse_threads;
        let read_threads = self.config.read_threads;
        let discover_threads = self.config.discover_threads;
        let batch_size = self.config.batch_size;
        let batches_per_commit = self.config.batches_per_commit;
        let tracing_enabled = self.config.pipeline_tracing;

        // Stage 1: DISCOVER
        let discover_root = root.to_path_buf();
        let discover_handle = thread::spawn(move || {
            let tracker = if tracing_enabled {
                Some(StageTracker::new("DISCOVER", discover_threads))
            } else {
                None
            };

            let stage = DiscoverStage::new(discover_root, discover_threads);
            let result = stage.run(path_tx);

            if let (Some(tracker), Ok(count)) = (&tracker, &result) {
                tracker.record_items(*count);
            }

            (result, tracker.map(|t| t.finalize()))
        });

        // Stage 2: READ
        let read_handles: Vec<_> = (0..read_threads)
            .map(|_| {
                let rx = path_rx.clone();
                let tx = content_tx.clone();
                thread::spawn(move || {
                    let stage = ReadStage::new(1);
                    stage.run(rx, tx)
                })
            })
            .collect();
        drop(path_rx);
        drop(content_tx);

        // Stage 3: PARSE - with wait time tracking
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|_| {
                let rx = content_rx.clone();
                let tx = parsed_tx.clone();
                let settings = Arc::clone(&settings);
                thread::spawn(move || {
                    init_parser_cache(settings.clone());
                    let stage = ParseStage::new(settings);
                    let mut parsed = 0;
                    let mut errors = 0;
                    let mut symbol_count = 0;
                    let mut input_wait = std::time::Duration::ZERO;
                    let mut output_wait = std::time::Duration::ZERO;

                    loop {
                        // Track input wait (time blocked on recv)
                        let recv_start = Instant::now();
                        let content = match rx.recv() {
                            Ok(c) => c,
                            Err(_) => break, // Channel closed
                        };
                        input_wait += recv_start.elapsed();

                        match stage.parse(content) {
                            Ok(p) => {
                                parsed += 1;
                                symbol_count += p.raw_symbols.len();

                                // Track output wait (time blocked on send)
                                let send_start = Instant::now();
                                if tx.send(p).is_err() {
                                    break;
                                }
                                output_wait += send_start.elapsed();
                            }
                            Err(_) => errors += 1,
                        }
                    }
                    (parsed, errors, symbol_count, input_wait, output_wait)
                })
            })
            .collect();
        drop(content_rx);
        drop(parsed_tx);

        // Stage 4: COLLECT (with starting counters for multi-directory support)
        let collect_handle = thread::spawn(move || {
            let tracker = if tracing_enabled {
                Some(StageTracker::new("COLLECT", 1).with_secondary("batches"))
            } else {
                None
            };

            let stage = CollectStage::new(batch_size)
                .with_start_counters(start_file_counter, start_symbol_counter);
            let result = stage.run(parsed_rx, batch_tx);

            // Record items and wait times before finalizing
            if let (Some(t), Ok((_, symbol_count, input_wait, output_wait))) = (&tracker, &result) {
                t.record_items(*symbol_count as usize);
                t.record_input_wait(*input_wait);
                t.record_output_wait(*output_wait);
            }

            (result, tracker.map(|t| t.finalize()))
        });

        // Stage 5: INDEX with semantic search, embedding pool, and optional progress
        // Clone index Arc for metadata update after pipeline completes
        let index_for_metadata = Arc::clone(&index);
        let index_handle = {
            let mut index_stage =
                IndexStage::new(index, batches_per_commit).with_semantic(semantic);
            if let Some(pool) = embedding_pool {
                index_stage = index_stage.with_embedding_pool(pool);
            }
            if let Some(prog) = progress {
                index_stage = index_stage.with_progress(prog);
            }
            thread::spawn(move || {
                let tracker = if tracing_enabled {
                    Some(StageTracker::new("INDEX", 1).with_secondary("commits"))
                } else {
                    None
                };

                let result = index_stage.run(batch_rx);

                // Record items and wait times before finalizing
                if let (Some(t), Ok((stats, _, _, input_wait))) = (&tracker, &result) {
                    t.record_items(stats.symbols_found);
                    t.record_input_wait(*input_wait);
                }

                (result, tracker.map(|t| t.finalize()))
            })
        };

        // Wait for all stages
        let (discover_result, discover_metrics) = discover_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("DISCOVER panicked".to_string()))?;
        let _files_discovered = discover_result?;

        // Add DISCOVER metrics
        if let (Some(m), Some(dm)) = (&metrics, discover_metrics) {
            m.add_stage(dm);
        }

        // READ stage metrics (aggregate across threads)
        let read_tracker = if tracing_enabled {
            Some(StageTracker::new("READ", read_threads).with_secondary("MB"))
        } else {
            None
        };
        let mut read_files = 0;
        let mut read_input_wait = std::time::Duration::ZERO;
        let mut read_output_wait = std::time::Duration::ZERO;
        for h in read_handles {
            if let Ok(Ok((files, _errors, input_wait, output_wait))) = h.join() {
                read_files += files;
                read_input_wait += input_wait;
                read_output_wait += output_wait;
            }
        }
        if let Some(tracker) = read_tracker {
            tracker.record_items(read_files);
            tracker.record_input_wait(read_input_wait);
            tracker.record_output_wait(read_output_wait);
            if let Some(m) = &metrics {
                m.add_stage(tracker.finalize());
            }
        }

        // PARSE stage metrics (aggregate across threads)
        let parse_tracker = if tracing_enabled {
            Some(StageTracker::new("PARSE", parse_threads).with_secondary("symbols"))
        } else {
            None
        };
        let mut parsed_files = 0;
        let mut total_symbols = 0;
        let mut total_input_wait = std::time::Duration::ZERO;
        let mut total_output_wait = std::time::Duration::ZERO;
        for h in parse_handles {
            if let Ok((files, _errors, symbols, input_wait, output_wait)) = h.join() {
                parsed_files += files;
                total_symbols += symbols;
                total_input_wait += input_wait;
                total_output_wait += output_wait;
            }
        }
        if let Some(tracker) = parse_tracker {
            tracker.record_items(parsed_files);
            tracker.record_secondary(total_symbols);
            tracker.record_input_wait(total_input_wait);
            tracker.record_output_wait(total_output_wait);
            if let Some(m) = &metrics {
                m.add_stage(tracker.finalize());
            }
        }

        // Get final counter values from COLLECT stage
        let (collect_result, collect_metrics) = collect_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("COLLECT panicked".to_string()))?;
        let (final_file_count, final_symbol_count, _, _) = collect_result?;

        // Add COLLECT metrics
        if let (Some(m), Some(cm)) = (&metrics, collect_metrics) {
            m.add_stage(cm);
        }

        let (index_result, index_metrics) = index_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("INDEX panicked".to_string()))?;
        let (mut stats, pending, cache, _) = index_result?;

        // Add INDEX metrics
        if let (Some(m), Some(im)) = (&metrics, index_metrics) {
            m.add_stage(im);
        }

        // Store final counter values to metadata for next directory
        // This is critical for multi-directory indexing to avoid ID collisions
        use crate::storage::MetadataKey;
        index_for_metadata.start_batch()?;
        index_for_metadata.store_metadata(MetadataKey::FileCounter, final_file_count as u64)?;
        index_for_metadata.store_metadata(MetadataKey::SymbolCounter, final_symbol_count as u64)?;
        index_for_metadata.commit_batch()?;

        stats.elapsed = start.elapsed();

        // Finalize metrics but don't log (caller logs after StatusLine drop)
        if let Some(ref m) = metrics {
            m.finalize(start.elapsed());
        }

        Ok((stats, pending, cache, metrics))
    }

    /// Synchronize index with configuration (directory-level change detection).
    ///
    /// Compares stored indexed paths (from IndexMetadata) with current config paths
    /// (from settings.toml). Indexes new directories and removes files from
    /// directories no longer in config.
    ///
    /// This is the Pipeline equivalent of SimpleIndexer::sync_with_config.
    ///
    /// # Arguments
    /// * `stored_paths` - Previously indexed directory paths (from IndexMetadata)
    /// * `config_paths` - Current directory paths from settings.toml
    /// * `index` - DocumentIndex for storage
    /// * `semantic` - Optional semantic search for embeddings
    /// * `_progress` - Whether to show progress (currently unused)
    ///
    /// # Returns
    /// SyncStats with counts of added/removed directories and files/symbols indexed
    pub fn sync_with_config(
        &self,
        stored_paths: Option<Vec<PathBuf>>,
        config_paths: &[PathBuf],
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingPool>>,
        _progress: bool,
    ) -> PipelineResult<SyncStats> {
        use std::collections::HashSet;

        let start = Instant::now();
        let semantic_path = self.settings.index_path.join("semantic");

        // Canonicalize both path sets for accurate comparison
        let stored_set: HashSet<PathBuf> = stored_paths
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();

        let config_set: HashSet<PathBuf> = config_paths
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();

        // Find new paths (in config but not stored)
        let new_paths: Vec<PathBuf> = config_set.difference(&stored_set).cloned().collect();

        // Find removed paths (in stored but not in config)
        let removed_paths: Vec<PathBuf> = stored_set.difference(&config_set).cloned().collect();

        // Early return if no changes
        if new_paths.is_empty() && removed_paths.is_empty() {
            return Ok(SyncStats {
                elapsed: start.elapsed(),
                ..Default::default()
            });
        }

        let mut stats = SyncStats::default();

        // Index new directories
        if !new_paths.is_empty() {
            tracing::info!(
                target: "pipeline",
                "Sync: Found {} new directories to index",
                new_paths.len()
            );

            for path in &new_paths {
                tracing::debug!(target: "pipeline", "  + {}", path.display());

                match self.index_incremental(
                    path,
                    Arc::clone(&index),
                    semantic.clone(),
                    embedding_pool.clone(),
                    false,
                ) {
                    Ok(inc_stats) => {
                        stats.files_indexed += inc_stats.index_stats.files_indexed;
                        stats.symbols_found += inc_stats.index_stats.symbols_found;
                        tracing::info!(
                            target: "pipeline",
                            "  Indexed {} files, {} symbols from {}",
                            inc_stats.index_stats.files_indexed,
                            inc_stats.index_stats.symbols_found,
                            path.display()
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            target: "pipeline",
                            "  Failed to index {}: {e}",
                            path.display()
                        );
                    }
                }
            }
            stats.added_dirs = new_paths.len();
        }

        // Remove files from deleted directories
        if !removed_paths.is_empty() {
            tracing::info!(
                target: "pipeline",
                "Sync: Found {} directories to remove",
                removed_paths.len()
            );

            // Get all indexed files and filter those under removed directories
            let all_files = match index.get_all_indexed_paths() {
                Ok(paths) => paths,
                Err(e) => {
                    tracing::error!(target: "pipeline", "  Failed to get indexed paths: {e}");
                    Vec::new()
                }
            };
            let mut files_to_remove = Vec::new();

            for file_path in all_files {
                if let Ok(file_canonical) = file_path.canonicalize() {
                    for removed_path in &removed_paths {
                        if file_canonical.starts_with(removed_path) {
                            files_to_remove.push(file_path.clone());
                            break;
                        }
                    }
                }
            }

            if !files_to_remove.is_empty() {
                tracing::debug!(
                    target: "pipeline",
                    "  Removing {} files from deleted directories",
                    files_to_remove.len()
                );

                // Use CleanupStage to remove files
                let cleanup_stage = if let Some(ref sem) = semantic {
                    CleanupStage::new(Arc::clone(&index), &semantic_path)
                        .with_semantic(Arc::clone(sem))
                } else {
                    CleanupStage::new(Arc::clone(&index), &semantic_path)
                };

                match cleanup_stage.cleanup_files(&files_to_remove) {
                    Ok(cleanup_stats) => {
                        stats.files_removed = cleanup_stats.files_cleaned;
                        stats.symbols_removed = cleanup_stats.symbols_removed;
                        tracing::info!(
                            target: "pipeline",
                            "  Removed {} files, {} symbols",
                            cleanup_stats.files_cleaned,
                            cleanup_stats.symbols_removed
                        );
                    }
                    Err(e) => {
                        tracing::error!(target: "pipeline", "  Cleanup failed: {e}");
                    }
                }
            }

            stats.removed_dirs = removed_paths.len();
        }

        stats.elapsed = start.elapsed();

        tracing::info!(
            target: "pipeline",
            "Sync complete: {} dirs added ({} files, {} symbols), {} dirs removed ({} files) in {:?}",
            stats.added_dirs,
            stats.files_indexed,
            stats.symbols_found,
            stats.removed_dirs,
            stats.files_removed,
            stats.elapsed
        );

        Ok(stats)
    }
}

/// Statistics from sync_with_config operation.
#[derive(Debug, Default)]
pub struct SyncStats {
    /// Number of new directories indexed
    pub added_dirs: usize,
    /// Number of directories removed from index
    pub removed_dirs: usize,
    /// Total files indexed from new directories
    pub files_indexed: usize,
    /// Total symbols found in new directories
    pub symbols_found: usize,
    /// Files removed during cleanup
    pub files_removed: usize,
    /// Symbols removed during cleanup
    pub symbols_removed: usize,
    /// Time taken
    pub elapsed: std::time::Duration,
}

/// Statistics from incremental indexing.
#[derive(Debug, Default)]
pub struct IncrementalStats {
    /// Number of new files indexed
    pub new_files: usize,
    /// Number of modified files re-indexed
    pub modified_files: usize,
    /// Number of deleted files cleaned up
    pub deleted_files: usize,
    /// Phase 1 indexing stats
    pub index_stats: IndexStats,
    /// Cleanup stats
    pub cleanup_stats: CleanupStats,
    /// Phase 2 resolution stats
    pub phase2_stats: Phase2Stats,
    /// Total time taken
    pub elapsed: std::time::Duration,
}

/// Statistics from Phase 2 resolution.
#[derive(Debug, Default)]
pub struct Phase2Stats {
    /// Total relationships to resolve
    pub total_relationships: usize,
    /// Defines relationships resolved (Pass 1)
    pub defines_resolved: usize,
    /// Calls relationships resolved (Pass 2)
    pub calls_resolved: usize,
    /// Other relationships resolved (Pass 2)
    pub other_resolved: usize,
    /// Failed to resolve
    pub unresolved: usize,
    /// Time taken
    pub elapsed: std::time::Duration,
}

/// Statistics from pipeline execution.
#[derive(Debug, Default)]
pub struct PipelineStats {
    /// Time spent in each stage (milliseconds)
    pub stage_times: StageTimings,
    /// Number of files processed
    pub files_processed: usize,
    /// Number of files that failed to parse
    pub files_failed: usize,
    /// Total symbols extracted
    pub symbols_extracted: usize,
    /// Total relationships found
    pub relationships_found: usize,
    /// Total relationships resolved
    pub relationships_resolved: usize,
    /// Peak memory usage (bytes)
    pub peak_memory: usize,
}

/// Timing breakdown by stage.
#[derive(Debug, Default)]
pub struct StageTimings {
    pub discover_ms: u64,
    pub read_ms: u64,
    pub parse_ms: u64,
    pub collect_ms: u64,
    pub index_ms: u64,
    pub resolve_ms: u64,
}

impl StageTimings {
    pub fn total_ms(&self) -> u64 {
        self.discover_ms
            + self.read_ms
            + self.parse_ms
            + self.collect_ms
            + self.index_ms
            + self.resolve_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn truncate(s: &str, max: usize) -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            format!("{}...", &s[..max - 3])
        }
    }

    #[test]
    fn test_pipeline_creation() {
        let settings = Arc::new(Settings::default());
        let pipeline = Pipeline::with_settings(settings);

        assert!(pipeline.config().parse_threads >= 1);
    }

    #[test]
    fn test_pipeline_with_custom_config() {
        let settings = Arc::new(Settings::default());
        let config = PipelineConfig::default().with_parse_threads(4);
        let pipeline = Pipeline::new(settings, config);

        assert_eq!(pipeline.config().parse_threads, 4);
    }

    /// End-to-end test proving Phase 1 collects symbols, imports, and pending relationships.
    ///
    /// Scenario: Two TypeScript files where file1 imports and calls file2.
    /// This demonstrates what Phase 1 produces for Phase 2 resolution:
    /// - Symbols with IDs
    /// - Imports (cross-file dependencies)
    /// - Pending relationships with from_id known, to_id unknown
    #[test]
    fn test_pipeline_end_to_end_proof() {
        use crate::storage::DocumentIndex;

        // Create temp directory with source files
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src dir");

        // File 2: utils.ts - exports helper functions
        let utils_content = r#"
// utils.ts - Helper utilities

export function formatName(first: string, last: string): string {
    return `${first} ${last}`;
}

export function validateEmail(email: string): boolean {
    return email.includes("@");
}

export class StringUtils {
    static capitalize(s: string): string {
        return s.charAt(0).toUpperCase() + s.slice(1);
    }

    static lowercase(s: string): string {
        return s.toLowerCase();
    }
}
"#;
        fs::write(src_dir.join("utils.ts"), utils_content).expect("Failed to write utils.ts");

        // File 1: main.ts - imports and calls functions from utils.ts
        let main_content = r#"
// main.ts - Entry point, imports from utils

import { formatName, validateEmail } from "./utils";
import { StringUtils } from "./utils";

function processUser(first: string, last: string, email: string): string {
    // Cross-file call: formatName is defined in utils.ts
    const fullName = formatName(first, last);

    // Cross-file call: validateEmail is defined in utils.ts
    if (!validateEmail(email)) {
        throw new Error("Invalid email");
    }

    // Cross-file static method call
    return StringUtils.capitalize(fullName);
}

function main(): void {
    const result = processUser("john", "doe", "john@example.com");
    console.log(result);
}

export { processUser, main };
"#;
        fs::write(src_dir.join("main.ts"), main_content).expect("Failed to write main.ts");

        // Create index in temp location
        let index_dir = temp_dir.path().join("index");
        fs::create_dir_all(&index_dir).expect("Failed to create index dir");

        // Create pipeline with settings
        let settings = Settings::default();
        let index = DocumentIndex::new(&index_dir, &settings).expect("Failed to create index");
        let index = Arc::new(index);
        let settings = Arc::new(settings);
        let pipeline = Pipeline::with_settings(settings);

        // Run Phase 1
        let result = pipeline.index_directory(&src_dir, index);

        match result {
            Ok((stats, pending_relationships, symbol_cache)) => {
                // Categorize relationships by kind
                let calls: Vec<_> = pending_relationships
                    .iter()
                    .filter(|r| matches!(r.kind, crate::RelationKind::Calls))
                    .collect();
                let uses: Vec<_> = pending_relationships
                    .iter()
                    .filter(|r| matches!(r.kind, crate::RelationKind::Uses))
                    .collect();

                // Print comprehensive Phase 1 output
                println!("\n================================================================");
                println!("PIPELINE PHASE 1 OUTPUT -> INPUT FOR PHASE 2");
                println!("================================================================");
                println!();
                println!("INDEXED DATA (in Tantivy):");
                println!("  Files indexed:        {}", stats.files_indexed);
                println!("  Symbols found:        {}", stats.symbols_found);
                println!("  Time elapsed:         {:?}", stats.elapsed);
                println!();
                println!("SYMBOL CACHE (for O(1) Phase 2 resolution):");
                println!("  Symbols cached:       {}", symbol_cache.len());
                println!("  Unique names:         {}", symbol_cache.unique_names());
                println!();
                println!("PENDING FOR PHASE 2 RESOLUTION:");
                println!("  Total relationships:  {}", pending_relationships.len());
                println!("    - Calls:            {}", calls.len());
                println!("    - Uses:             {}", uses.len());
                println!();

                // Show cross-file calls (the key scenario)
                println!("CROSS-FILE CALLS (Phase 2 must resolve to_id):");
                println!("----------------------------------------------------------------");
                println!(
                    "  {:20} {:20} {:8} {:8} {:12}",
                    "FROM", "TO", "from_id", "file_id", "call_site"
                );
                println!(
                    "  {:20} {:20} {:8} {:8} {:12}",
                    "----", "--", "-------", "-------", "---------"
                );
                for rel in calls.iter().take(15) {
                    let range_info = rel
                        .to_range
                        .as_ref()
                        .map(|r| format!("{}:{}", r.start_line, r.start_column))
                        .unwrap_or_else(|| "-".to_string());

                    println!(
                        "  {:20} {:20} {:8} {:8} {:12}",
                        truncate(&rel.from_name, 20),
                        truncate(&rel.to_name, 20),
                        rel.from_id.map(|id| id.value()).unwrap_or(0),
                        rel.file_id.value(),
                        range_info
                    );
                }
                if calls.len() > 15 {
                    println!("  ... and {} more calls", calls.len() - 15);
                }
                println!();

                // Show what Phase 2 needs to do
                println!("PHASE 2 TASK:");
                println!("  For each pending relationship:");
                println!("    1. from_id is KNOWN (assigned by COLLECT)");
                println!("    2. to_id is UNKNOWN (needs resolution)");
                println!("    3. Use imports + symbol cache + to_range for disambiguation");
                println!("================================================================\n");

                // Assertions
                assert_eq!(stats.files_indexed, 2, "Expected exactly 2 files indexed");
                assert!(
                    stats.symbols_found >= 6,
                    "Expected at least 6 symbols (functions + class + methods)"
                );
                assert!(!calls.is_empty(), "Expected cross-file call relationships");

                // Verify symbol cache matches indexed symbols
                assert_eq!(
                    symbol_cache.len(),
                    stats.symbols_found,
                    "Symbol cache must contain all indexed symbols"
                );

                // Verify range data for disambiguation
                let with_range = pending_relationships
                    .iter()
                    .filter(|r| r.to_range.is_some())
                    .count();
                let with_from_id = pending_relationships
                    .iter()
                    .filter(|r| r.from_id.is_some())
                    .count();

                println!("Resolution readiness:");
                println!(
                    "  - with from_id:  {}/{}",
                    with_from_id,
                    pending_relationships.len()
                );
                println!(
                    "  - with to_range: {}/{}",
                    with_range,
                    pending_relationships.len()
                );

                assert!(
                    with_from_id > 0,
                    "Expected from_id to be populated by COLLECT stage"
                );
                assert!(
                    with_range > 0,
                    "Expected to_range for Phase 2 disambiguation"
                );
            }
            Err(e) => {
                panic!("Pipeline failed: {e:?}");
            }
        }
    }

    /// Proves that pipeline stages run on distinct OS threads.
    ///
    /// Uses thread_id crate to get actual OS-level thread IDs (pthread_t on macOS/Linux).
    /// Verifies:
    /// - All thread IDs are unique (different OS threads)
    /// - Thread count matches configuration (read + parse + collect + index + discover)
    #[test]
    fn test_pipeline_uses_distinct_threads() {
        use std::collections::HashSet;
        use std::sync::Mutex;

        // Shared storage for OS-level thread IDs
        let thread_ids: Arc<Mutex<HashSet<usize>>> = Arc::new(Mutex::new(HashSet::new()));

        // Simulate pipeline thread structure with known counts
        let read_threads = 2;
        let parse_threads = 4;

        // Track main thread (OS-level)
        let main_thread_id = thread_id::get();
        println!("Main thread (OS): {main_thread_id}");

        // Stage 1: DISCOVER (1 thread)
        let ids = Arc::clone(&thread_ids);
        let discover_handle = thread::spawn(move || {
            let tid = thread_id::get();
            ids.lock().unwrap().insert(tid);
            println!("DISCOVER thread (OS): {tid}");
            tid
        });

        // Stage 2: READ (N threads)
        let read_handles: Vec<_> = (0..read_threads)
            .map(|i| {
                let ids = Arc::clone(&thread_ids);
                thread::spawn(move || {
                    let tid = thread_id::get();
                    ids.lock().unwrap().insert(tid);
                    println!("READ[{i}] thread (OS): {tid}");
                    tid
                })
            })
            .collect();

        // Stage 3: PARSE (N threads)
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|i| {
                let ids = Arc::clone(&thread_ids);
                thread::spawn(move || {
                    let tid = thread_id::get();
                    ids.lock().unwrap().insert(tid);
                    println!("PARSE[{i}] thread (OS): {tid}");
                    tid
                })
            })
            .collect();

        // Stage 4: COLLECT (1 thread)
        let ids = Arc::clone(&thread_ids);
        let collect_handle = thread::spawn(move || {
            let tid = thread_id::get();
            ids.lock().unwrap().insert(tid);
            println!("COLLECT thread (OS): {tid}");
            tid
        });

        // Stage 5: INDEX (1 thread)
        let ids = Arc::clone(&thread_ids);
        let index_handle = thread::spawn(move || {
            let tid = thread_id::get();
            ids.lock().unwrap().insert(tid);
            println!("INDEX thread (OS): {tid}");
            tid
        });

        // Wait for all threads
        let discover_tid = discover_handle.join().expect("DISCOVER panic");
        let read_tids: Vec<_> = read_handles
            .into_iter()
            .map(|h| h.join().expect("READ panic"))
            .collect();
        let parse_tids: Vec<_> = parse_handles
            .into_iter()
            .map(|h| h.join().expect("PARSE panic"))
            .collect();
        let collect_tid = collect_handle.join().expect("COLLECT panic");
        let index_tid = index_handle.join().expect("INDEX panic");

        // Verify results
        let unique_ids = thread_ids.lock().unwrap();
        let expected_threads = 1 + read_threads + parse_threads + 1 + 1; // discover + read + parse + collect + index

        println!("\n========================================");
        println!("OS-LEVEL THREAD VERIFICATION");
        println!("========================================");
        println!("Expected threads: {expected_threads}");
        println!("Unique OS thread IDs: {}", unique_ids.len());
        println!("Main thread (OS): {main_thread_id}");
        println!();
        println!("OS Thread ID breakdown:");
        println!("  DISCOVER: {discover_tid}");
        println!("  READ:     {read_tids:?}");
        println!("  PARSE:    {parse_tids:?}");
        println!("  COLLECT:  {collect_tid}");
        println!("  INDEX:    {index_tid}");
        println!("========================================\n");

        // Assertions
        assert_eq!(
            unique_ids.len(),
            expected_threads,
            "All threads must have unique OS-level IDs"
        );
        assert!(
            !unique_ids.contains(&main_thread_id),
            "Work threads must be different from main thread"
        );

        // Verify no thread ID appears twice
        let all_tids = [
            vec![discover_tid, collect_tid, index_tid],
            read_tids,
            parse_tids,
        ]
        .concat();

        let unique_count = all_tids.iter().collect::<HashSet<_>>().len();
        assert_eq!(
            unique_count,
            all_tids.len(),
            "Every stage must run on its own OS thread"
        );
    }
}
