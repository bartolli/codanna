//! Change-driven indexing: incremental runs, single-file reindex, config sync.

use super::stages::{CleanupStage, CollectStage, DiscoverStage, IndexStage, ReadStage};
use super::{
    CleanupStats, DiscoverResult, EmbedOptions, FileBarriers, FileBindings, FileSource,
    IncrementalStats, ParseStage, Phase1Options, Phase2Stats, Pipeline, PipelineError,
    PipelineResult, ProgressSink, SingleFileStats, SymbolLookupCache, SyncStats,
    UnresolvedRelationship, init_parser_cache,
};
use crate::FileId;
use crate::indexing::IndexStats;
use crate::io::status_line::DualProgressBar;
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Phase 1 outputs accumulated across sequential root walks so Phase 2
/// runs once over the union. Per-root resolution loses cross-root edges:
/// a root's imports can only bind after every root's symbols are
/// committed, so the candidate table must be seeded from the persisted
/// index after the last walk.
#[derive(Default)]
pub struct PendingResolution {
    unresolved: Vec<UnresolvedRelationship>,
    variable_bindings: FileBindings,
    this_barriers: FileBarriers,
    captured_inbound: Vec<super::stages::cleanup::CapturedInboundEdge>,
    /// True once any deferred run reached its indexing tail; an
    /// all-cached sequence never ran cleanup or Phase 1, so resolution
    /// and embedding persistence are skipped entirely (matches the
    /// pre-deferral early-return).
    ran: bool,
}

impl Pipeline {
    /// Add the unique caller files of this run's relocated and deleted
    /// targets to the modified set, so their edges are re-derived by
    /// parse and resolution rather than preserved by rebind or lost
    /// with the target. Deleted targets invalidate unconditionally:
    /// even without a replacement file, the fresh oracle can re-resolve
    /// a caller's evidence to an alternative referent once the primary
    /// is gone; caller resolution selects a destination only when
    /// source evidence supports one. Runs BEFORE cleanup: collection
    /// reads the inbound-edge rows cleanup deletes. Membership in the
    /// modified set also excludes these files from relocation capture
    /// and routes them through the reindex capture lane, which
    /// preserves edges pointing INTO them -- invalidation is one hop,
    /// not a reverse-dependency closure.
    fn invalidate_target_callers(
        index: &DocumentIndex,
        discover: &mut DiscoverResult,
    ) -> PipelineResult<usize> {
        if discover.renamed_files.is_empty() && discover.deleted_files.is_empty() {
            return Ok(0);
        }
        let mut in_run: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        in_run.extend(discover.new_files.iter().cloned());
        in_run.extend(discover.modified_files.iter().cloned());
        in_run.extend(discover.deleted_files.iter().cloned());
        for (old, new) in &discover.renamed_files {
            in_run.insert(old.clone());
            in_run.insert(new.clone());
        }
        let mut target_paths: Vec<PathBuf> = discover
            .renamed_files
            .iter()
            .map(|(old, _)| old.clone())
            .collect();
        target_paths.extend(discover.deleted_files.iter().cloned());
        let callers = super::stages::cleanup::inbound_caller_files(index, &target_paths, &in_run)?;
        let count = callers.len();
        tracing::debug!(
            target: "pipeline",
            "caller invalidation: renamed={} deleted={} new={} modified={} targets={} -> invalidated={count}",
            discover.renamed_files.len(),
            discover.deleted_files.len(),
            discover.new_files.len(),
            discover.modified_files.len(),
            target_paths.len(),
        );
        if count > 0 {
            tracing::info!(
                target: "pipeline",
                "Relocation invalidated {count} caller file(s) for re-resolution"
            );
            discover.modified_files.extend(callers);
        }
        Ok(count)
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
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
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

        // Read file using ReadStage (with absolute path for fs access)
        let read_stage = ReadStage::new(1);
        let mut file_content = read_stage.read_single(&path.to_path_buf())?;
        // Use normalized path for storage consistency with full index
        file_content.path = normalized_path.to_path_buf();
        let content_hash = file_content.hash.clone();

        // Check if file already exists by querying Tantivy
        let mut existing_changed = false;
        let mut captured_inbound: Vec<
            crate::indexing::pipeline::stages::cleanup::CapturedInboundEdge,
        > = Vec::new();
        if let Ok(Some((existing_file_id, existing_hash, _mtime))) = index.get_file_info(path_str) {
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
            existing_changed = true;
        }

        // Parse BEFORE cleanup: parse is pure (settings-only), so a parse
        // failure here leaves the file's old rows untouched. Cleanup-first
        // turned a construction failure into durable row loss.
        init_parser_cache(Arc::clone(&self.settings));
        let parse_stage = ParseStage::new(Arc::clone(&self.settings));
        let parsed = parse_stage.parse(file_content)?;

        if existing_changed {
            // File has changed - cleanup old data within a batch
            // Start batch for cleanup to avoid creating temporary writers
            index.start_batch()?;

            let cleanup_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };

            // Capture the edges pointing into this file before its rows go:
            // the re-index below re-derives only this file's OWN outgoing
            // edges, so inbound edges owned by unchanged files would be lost.
            let (_, captured) =
                cleanup_stage.cleanup_files_for_reindex(&[normalized_path.to_path_buf()])?;
            captured_inbound = captured;

            // Commit cleanup changes before re-indexing
            index.commit_batch()?;
        }

        // Collect into a batch (now includes embedding candidates)
        let collect_stage = CollectStage::new(self.config.batch_size);
        let (mut batch, unresolved, embed_batch) =
            collect_stage.process_single(parsed, Arc::clone(&index))?;
        let variable_bindings = std::mem::take(&mut batch.variable_bindings);
        let this_barrier_spans = std::mem::take(&mut batch.this_barrier_spans);

        // Index the batch
        let index_stage = IndexStage::new(Arc::clone(&index), self.config.batches_per_commit);

        let symbols_found = batch.symbols.len();
        // Capture file_id before batch is consumed
        let file_id = batch
            .file_registrations
            .first()
            .map(|r| r.file_id)
            .unwrap_or(FileId(0));
        // Ids issued by process_single start above the committed counters,
        // so the batch maxima are the new high-water marks.
        let max_file_id = batch
            .file_registrations
            .iter()
            .map(|r| r.file_id.value())
            .max();
        let max_symbol_id = batch.symbols.iter().map(|s| s.id.value()).max();

        // Start a batch before indexing
        index.start_batch()?;
        index_stage.index_batch(batch)?;

        // Counters become durable in the same commit as the docs; without
        // this, the next single-file run re-reads the stale counter and
        // re-issues live ids (duplicate symbol_id across generations).
        {
            use crate::storage::MetadataKey;
            if let Some(value) = max_file_id {
                index.store_metadata(MetadataKey::FileCounter, u64::from(value))?;
            }
            if let Some(value) = max_symbol_id {
                index.store_metadata(MetadataKey::SymbolCounter, u64::from(value))?;
            }
        }

        // Commit the batch
        index.commit_batch()?;

        // Replacements are visible now, so the captured edges can find their
        // new targets. Runs after the commit because the rebind looks the
        // targets up through the searcher.
        if !captured_inbound.is_empty() {
            let rebind_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };
            rebind_stage.rebind_inbound_edges(&captured_inbound)?;
        }

        // Generate embeddings for symbols with doc_comments
        if let (Some(pool), Some(sem)) = (&embedding_pool, &semantic) {
            if !embed_batch.candidates.is_empty() {
                tracing::info!(
                    target: "pipeline",
                    "Generating {} embeddings for {}",
                    embed_batch.candidates.len(),
                    path.display()
                );

                // Convert to the format expected by embed_parallel
                let items: Vec<_> = embed_batch
                    .candidates
                    .iter()
                    .map(|(id, doc, lang)| (*id, doc.as_ref(), lang.as_ref()))
                    .collect();

                // Generate embeddings
                let embeddings = pool
                    .embed_parallel(&items)
                    .map_err(|e| PipelineError::Parse {
                        path: path.to_path_buf(),
                        reason: format!("Embedding generation failed: {e}"),
                    })?;

                // store_embeddings warns internally on any dropped embeddings.
                if !embeddings.is_empty() {
                    if let Ok(mut guard) = sem.lock() {
                        guard.store_embeddings(embeddings);
                    }
                }
            }
        }

        // Build symbol cache for resolution
        let symbol_cache = Arc::new(SymbolLookupCache::from_index(&index)?);

        // Run Phase 2 resolution
        let phase2_stats = self.run_phase2(
            unresolved,
            variable_bindings,
            this_barrier_spans,
            symbol_cache,
            index,
        )?;

        // Save embeddings
        self.persist_embeddings(semantic.as_ref(), &semantic_path)?;

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
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
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
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
        force: bool,
        show_progress: bool,
        total_files: usize,
    ) -> PipelineResult<IncrementalStats> {
        let mut pending = PendingResolution::default();
        let mut stats = self.index_incremental_deferred(
            root,
            Arc::clone(&index),
            semantic.clone(),
            embedding_pool,
            force,
            show_progress,
            total_files,
            &mut pending,
        )?;
        stats.phase2_stats = self.resolve_pending(pending, index, semantic, show_progress)?;
        Ok(stats)
    }

    /// Run discovery, cleanup, and Phase 1 for one root, deferring
    /// resolution: outputs accumulate into `pending` for a later
    /// `resolve_pending` call. Multi-root flows call this per root so
    /// Phase 2 sees every root's symbols; `phase2_stats` in the returned
    /// stats is empty by construction.
    #[allow(clippy::too_many_arguments)]
    pub fn index_incremental_deferred(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
        force: bool,
        show_progress: bool,
        total_files: usize,
        pending: &mut PendingResolution,
    ) -> PipelineResult<IncrementalStats> {
        use crate::io::status_line::{
            ProgressBar, ProgressBarOptions, ProgressBarStyle, StatusLine,
        };

        let start = Instant::now();
        let semantic_path = self.settings.index_path.join("semantic");

        // Progress bar options shared between phases
        let bar_options = ProgressBarOptions::default()
            .with_style(ProgressBarStyle::VerticalSolid)
            .with_width(28);

        // Run Phase 1 indexing with appropriate progress bar
        let (
            index_stats,
            unresolved,
            variable_bindings,
            this_barriers,
            cleanup_stats,
            deleted_symbols,
            discover_counts,
            captured_inbound,
        ) = if force {
            // Force mode: use DualProgressBar for semantic+embedding, else single bar
            let has_embedding = semantic.is_some() && embedding_pool.is_some();

            if has_embedding {
                // Dual progress: EMBED and INDEX running in parallel
                // Estimate embedding candidates = total_files (actual will vary based on symbols per file)
                let dual_bar = show_progress.then(|| {
                    Arc::new(DualProgressBar::new(
                        "EMBED",
                        total_files as u64, // Estimated embedding candidates
                        "embedded",
                        "INDEX",
                        total_files as u64,
                        "files",
                    ))
                });
                let dual_status = dual_bar
                    .as_ref()
                    .map(|bar| StatusLine::new(Arc::clone(bar)));

                let (stats, unresolved, bindings, barriers, _cache, metrics) = self.run_phase1(
                    FileSource::Walk(root.to_path_buf()),
                    Arc::clone(&index),
                    Phase1Options {
                        progress: dual_bar
                            .clone()
                            .map_or(ProgressSink::Silent, ProgressSink::Dual),
                        embed: Some(EmbedOptions {
                            pool: embedding_pool
                                .clone()
                                .expect("has_embedding checked pool.is_some()"),
                            semantic: Arc::clone(
                                semantic
                                    .as_ref()
                                    .expect("has_embedding checked semantic.is_some()"),
                            ),
                        }),
                    },
                )?;

                // Drop StatusLine BEFORE logging to avoid stderr race condition
                drop(dual_status);
                if let Some(m) = metrics {
                    m.log();
                }
                if let Some(bar) = dual_bar {
                    eprintln!("{bar}");
                }

                let files_indexed = stats.files_indexed;
                (
                    stats,
                    unresolved,
                    bindings,
                    barriers,
                    CleanupStats::default(),
                    0,
                    (files_indexed, 0, 0, 0, 0),
                    Vec::new(),
                )
            } else {
                // Single progress bar (no embedding or no semantic)
                let phase1_bar = show_progress.then(|| {
                    Arc::new(ProgressBar::with_4_labels(
                        total_files as u64,
                        "files",
                        "indexed",
                        "failed",
                        "embedded",
                        bar_options,
                    ))
                });
                let phase1_status = phase1_bar
                    .as_ref()
                    .map(|bar| StatusLine::new(Arc::clone(bar)));

                // has_embedding is false here, so semantic and pool are never
                // both present: the embed stage cannot run in this arm.
                let (stats, unresolved, bindings, barriers, _cache, metrics) = self.run_phase1(
                    FileSource::Walk(root.to_path_buf()),
                    Arc::clone(&index),
                    Phase1Options {
                        progress: phase1_bar
                            .clone()
                            .map_or(ProgressSink::Silent, ProgressSink::Bar),
                        embed: None,
                    },
                )?;

                // Drop StatusLine BEFORE logging to avoid stderr race condition
                drop(phase1_status);
                if let Some(m) = metrics {
                    m.log();
                }
                if let Some(bar) = phase1_bar {
                    eprintln!("{bar}");
                }

                let files_indexed = stats.files_indexed;
                (
                    stats,
                    unresolved,
                    bindings,
                    barriers,
                    CleanupStats::default(),
                    0,
                    (files_indexed, 0, 0, 0, 0),
                    Vec::new(),
                )
            }
        } else {
            // Incremental mode: discover first, then create bar with actual count
            let discover_stage = DiscoverStage::new(root, self.config.discover_threads)
                .with_index(Arc::clone(&index))
                .with_workspace_root(self.settings.workspace_root.clone());
            let mut discover_result = discover_stage.run_incremental()?;

            if discover_result.is_empty() {
                return Ok(IncrementalStats {
                    new_files: 0,
                    modified_files: 0,
                    deleted_files: 0,
                    renamed_files: 0,
                    invalidated_caller_files: 0,
                    deleted_symbols: 0,
                    index_stats: IndexStats::new(),
                    cleanup_stats: CleanupStats::default(),
                    phase2_stats: Phase2Stats::default(),
                    elapsed: start.elapsed(),
                });
            }

            let modified_on_disk = discover_result.modified_files.len();
            let invalidated_caller_files =
                Self::invalidate_target_callers(&index, &mut discover_result)?;

            let files_to_index: Vec<PathBuf> = discover_result
                .new_files
                .iter()
                .chain(discover_result.modified_files.iter())
                .chain(discover_result.renamed_files.iter().map(|(_, new)| new))
                .cloned()
                .collect();

            // Validate parser construction for every language in the
            // change set BEFORE removing the modified files' old rows: a
            // config error must fail the run with the rows still in place.
            super::stages::preflight_file_parsers(&files_to_index, &self.settings)?;

            // Cleanup
            let cleanup_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };

            let mut cleanup_stats = CleanupStats::default();
            let mut deleted_symbols = 0;
            let mut captured_inbound: Vec<
                crate::indexing::pipeline::stages::cleanup::CapturedInboundEdge,
            > = Vec::new();
            if !discover_result.deleted_files.is_empty() {
                let stats = cleanup_stage.cleanup_files(&discover_result.deleted_files)?;
                cleanup_stats.files_cleaned += stats.files_cleaned;
                cleanup_stats.symbols_removed += stats.symbols_removed;
                deleted_symbols = stats.symbols_removed;
            }
            // Relocation cleanup runs BEFORE the modified capture: edges
            // sourced in renamed files must already be gone from the
            // searcher when the modified files capture, or a dead from-id
            // gets rebound.
            if !discover_result.renamed_files.is_empty() {
                let (stats, captured) = cleanup_stage.cleanup_files_for_relocation(
                    &discover_result.renamed_files,
                    &discover_result.modified_files,
                )?;
                cleanup_stats.files_cleaned += stats.files_cleaned;
                cleanup_stats.symbols_removed += stats.symbols_removed;
                captured_inbound.extend(captured);
            }
            if !discover_result.modified_files.is_empty() {
                let (stats, captured) =
                    cleanup_stage.cleanup_files_for_reindex(&discover_result.modified_files)?;
                cleanup_stats.files_cleaned += stats.files_cleaned;
                cleanup_stats.symbols_removed += stats.symbols_removed;
                captured_inbound.extend(captured);
            }

            // Create Phase 1 bar with actual files to index count
            // Labels: files, indexed, failed, embedded (for embedding visibility)
            let phase1_bar = show_progress.then(|| {
                Arc::new(ProgressBar::with_4_labels(
                    files_to_index.len() as u64,
                    "files",
                    "indexed",
                    "failed",
                    "embedded",
                    bar_options,
                ))
            });
            let phase1_status = phase1_bar
                .as_ref()
                .map(|bar| StatusLine::new(Arc::clone(bar)));

            let embed = match (&semantic, &embedding_pool) {
                (Some(sem), Some(pool)) => Some(EmbedOptions {
                    pool: Arc::clone(pool),
                    semantic: Arc::clone(sem),
                }),
                _ => None,
            };
            let (stats, unresolved, bindings, barriers, _run_cache, metrics) = self.run_phase1(
                FileSource::List(files_to_index),
                Arc::clone(&index),
                Phase1Options {
                    progress: phase1_bar
                        .clone()
                        .map_or(ProgressSink::Silent, ProgressSink::Bar),
                    embed,
                },
            )?;

            // Drop StatusLine BEFORE logging to avoid stderr race condition
            drop(phase1_status);
            if let Some(m) = metrics {
                m.log();
            }
            if let Some(bar) = phase1_bar {
                eprintln!("{bar}");
            }

            let counts = (
                discover_result.new_files.len(),
                modified_on_disk,
                discover_result.deleted_files.len(),
                discover_result.renamed_files.len(),
                invalidated_caller_files,
            );
            (
                stats,
                unresolved,
                bindings,
                barriers,
                cleanup_stats,
                deleted_symbols,
                counts,
                captured_inbound,
            )
        };

        // Resolution is deferred: accumulate this root's Phase 1 outputs.
        pending.unresolved.extend(unresolved);
        pending.variable_bindings.extend(variable_bindings);
        pending.this_barriers.extend(this_barriers);
        pending.captured_inbound.extend(captured_inbound);
        pending.ran = true;

        Ok(IncrementalStats {
            new_files: discover_counts.0,
            modified_files: discover_counts.1,
            deleted_files: discover_counts.2,
            renamed_files: discover_counts.3,
            invalidated_caller_files: discover_counts.4,
            deleted_symbols,
            index_stats,
            cleanup_stats,
            phase2_stats: Phase2Stats::default(),
            elapsed: start.elapsed(),
        })
    }

    /// Resolve everything accumulated by deferred runs: seed the
    /// candidate table from the persisted index (run-scoped caches hold
    /// only their own walk's files, hiding other roots' and unchanged
    /// files' symbols and re-export aliases), run Phase 2 once, re-point
    /// captured inbound edges at the committed replacements, persist
    /// embeddings. A `pending` from all-cached runs is a no-op.
    pub fn resolve_pending(
        &self,
        pending: PendingResolution,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        show_progress: bool,
    ) -> PipelineResult<Phase2Stats> {
        if !pending.ran {
            return Ok(Phase2Stats::default());
        }
        let semantic_path = self.settings.index_path.join("semantic");

        let symbol_cache = Arc::new(SymbolLookupCache::from_index(&index)?);
        let phase2_stats = self.run_phase2_maybe_bar(
            pending.unresolved,
            pending.variable_bindings,
            pending.this_barriers,
            symbol_cache,
            Arc::clone(&index),
            show_progress,
        )?;

        // Phase 2 has stored the changed files' own edges; the replacements
        // are committed and findable, so the edges captured from unchanged
        // files can be re-pointed at them.
        if !pending.captured_inbound.is_empty() {
            let rebind_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };
            rebind_stage.rebind_inbound_edges(&pending.captured_inbound)?;
        }

        // Save embeddings
        self.persist_embeddings(semantic.as_ref(), &semantic_path)?;

        Ok(phase2_stats)
    }

    /// Index a directory with optional progress bar.
    pub fn index_incremental_with_progress(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
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
        let discover_stage = DiscoverStage::new(root, self.config.discover_threads)
            .with_index(Arc::clone(&index))
            .with_workspace_root(self.settings.workspace_root.clone());
        let mut discover_result = discover_stage.run_incremental()?;

        tracing::info!(
            target: "pipeline",
            "Incremental discovery: {} new, {} modified, {} deleted, {} renamed",
            discover_result.new_files.len(),
            discover_result.modified_files.len(),
            discover_result.deleted_files.len(),
            discover_result.renamed_files.len()
        );

        if discover_result.is_empty() {
            return Ok(IncrementalStats {
                new_files: 0,
                modified_files: 0,
                deleted_files: 0,
                renamed_files: 0,
                invalidated_caller_files: 0,
                deleted_symbols: 0,
                index_stats: IndexStats::new(),
                cleanup_stats: CleanupStats::default(),
                phase2_stats: Phase2Stats::default(),
                elapsed: start.elapsed(),
            });
        }

        let modified_on_disk = discover_result.modified_files.len();
        let invalidated_caller_files =
            Self::invalidate_target_callers(&index, &mut discover_result)?;

        // Combine new + modified + renamed-to paths for indexing
        let files_to_index: Vec<PathBuf> = discover_result
            .new_files
            .iter()
            .chain(discover_result.modified_files.iter())
            .chain(discover_result.renamed_files.iter().map(|(_, new)| new))
            .cloned()
            .collect();

        // Validate parser construction for every language in the change
        // set BEFORE removing the modified files' old rows: a config
        // error must fail the run with the rows still in place.
        super::stages::preflight_file_parsers(&files_to_index, &self.settings)?;

        // Create cleanup stage
        let cleanup_stage = if let Some(ref sem) = semantic {
            CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
        } else {
            CleanupStage::new(Arc::clone(&index), &semantic_path)
        };

        // Cleanup deleted files
        let mut cleanup_stats = CleanupStats::default();
        let mut deleted_symbols = 0;
        if !discover_result.deleted_files.is_empty() {
            let stats = cleanup_stage.cleanup_files(&discover_result.deleted_files)?;
            cleanup_stats.files_cleaned += stats.files_cleaned;
            cleanup_stats.symbols_removed += stats.symbols_removed;
            cleanup_stats.embeddings_removed += stats.embeddings_removed;
            deleted_symbols = stats.symbols_removed;
        }

        // Relocation cleanup runs BEFORE the modified capture: edges sourced
        // in renamed files must already be gone from the searcher when the
        // modified files capture, or a dead from-id gets rebound.
        let mut captured_inbound = Vec::new();
        if !discover_result.renamed_files.is_empty() {
            let (stats, captured) = cleanup_stage.cleanup_files_for_relocation(
                &discover_result.renamed_files,
                &discover_result.modified_files,
            )?;
            cleanup_stats.files_cleaned += stats.files_cleaned;
            cleanup_stats.symbols_removed += stats.symbols_removed;
            cleanup_stats.embeddings_removed += stats.embeddings_removed;
            captured_inbound.extend(captured);
        }

        // Cleanup modified files (old data must be removed before re-indexing).
        // Capture the edges pointing into them first: deleting a file's rows
        // also deletes every edge targeting its symbols, and the re-index
        // re-derives only the file's OWN outgoing edges.
        if !discover_result.modified_files.is_empty() {
            let (stats, captured) =
                cleanup_stage.cleanup_files_for_reindex(&discover_result.modified_files)?;
            cleanup_stats.files_cleaned += stats.files_cleaned;
            cleanup_stats.symbols_removed += stats.symbols_removed;
            cleanup_stats.embeddings_removed += stats.embeddings_removed;
            captured_inbound.extend(captured);
        }

        // Run Phase 1 on the files to index
        let show_progress = progress.is_some();
        let embed = match (&semantic, &embedding_pool) {
            (Some(sem), Some(pool)) => Some(EmbedOptions {
                pool: Arc::clone(pool),
                semantic: Arc::clone(sem),
            }),
            _ => None,
        };
        let (index_stats, unresolved, variable_bindings, this_barriers, _run_cache, metrics) = self
            .run_phase1(
                FileSource::List(files_to_index),
                Arc::clone(&index),
                Phase1Options {
                    progress: progress.map_or(ProgressSink::Silent, ProgressSink::Bar),
                    embed,
                },
            )?;

        // Log pipeline metrics (no StatusLine in this path, safe to log immediately)
        if let Some(m) = metrics {
            m.log();
        }

        // Run Phase 2 resolution with progress if Phase 1 had progress.
        // Seed the cache from the persisted index: the run-scoped cache
        // holds only this run's files, hiding unchanged files' symbols
        // and re-export aliases from resolution.
        let symbol_cache = Arc::new(SymbolLookupCache::from_index(&index)?);
        let phase2_stats = self.run_phase2_maybe_bar(
            unresolved,
            variable_bindings,
            this_barriers,
            symbol_cache,
            Arc::clone(&index),
            show_progress,
        )?;

        // Replacements are committed and findable now, so the captured edges
        // can be re-pointed at them.
        if !captured_inbound.is_empty() {
            cleanup_stage.rebind_inbound_edges(&captured_inbound)?;
        }

        // Save embeddings
        self.persist_embeddings(semantic.as_ref(), &semantic_path)?;

        Ok(IncrementalStats {
            new_files: discover_result.new_files.len(),
            modified_files: modified_on_disk,
            deleted_files: discover_result.deleted_files.len(),
            renamed_files: discover_result.renamed_files.len(),
            invalidated_caller_files,
            deleted_symbols,
            index_stats,
            cleanup_stats,
            phase2_stats,
            elapsed: start.elapsed(),
        })
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
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RelationKind, Settings};

    fn calls_edge_exists(index: &DocumentIndex, from: &str, to: &str) -> bool {
        let callers = index.find_symbols_by_name(from, None).unwrap();
        let callees = index.find_symbols_by_name(to, None).unwrap();
        let (Some(caller), Some(callee)) = (callers.first(), callees.first()) else {
            return false;
        };
        index
            .get_relationships_from(caller.id, RelationKind::Calls)
            .unwrap()
            .iter()
            .any(|(_, to_id, _)| *to_id == callee.id)
    }

    // Regression: the watcher single-file path committed symbol docs
    // without persisting the id counters, so the next single-file event
    // re-read the stale counter and re-issued live ids — the duplicate
    // symbol_id state observed on the live index. Consecutive runs on
    // different files must consume disjoint id ranges.
    #[test]
    fn consecutive_single_file_runs_issue_disjoint_symbol_ids() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fixture");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("a.py"),
            "def alpha_one():\n    pass\n\n\ndef alpha_two():\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            root.join("b.py"),
            "def beta_one():\n    pass\n\n\ndef beta_two():\n    pass\n",
        )
        .unwrap();

        let settings = Arc::new(Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        });
        let index =
            Arc::new(DocumentIndex::new(settings.index_path.join("tantivy"), &settings).unwrap());
        let pipeline = Pipeline::with_settings(Arc::clone(&settings));

        pipeline
            .index_file_single(&root.join("a.py"), Arc::clone(&index), None, None)
            .unwrap();
        pipeline
            .index_file_single(&root.join("b.py"), Arc::clone(&index), None, None)
            .unwrap();

        let mut ids = std::collections::HashSet::new();
        for name in ["alpha_one", "alpha_two", "beta_one", "beta_two"] {
            let found = index.find_symbols_by_name(name, None).unwrap();
            assert_eq!(found.len(), 1, "{name} must exist exactly once");
            ids.insert(found[0].id);
        }
        assert_eq!(
            ids.len(),
            4,
            "symbol ids must be disjoint across single-file generations"
        );
    }

    // Regression: batch-incremental Phase 2 resolved against a cache scoped
    // to the run's files, so a consumer touched alone lost its edge through
    // an unchanged __init__.py re-export (and any candidate in an unchanged
    // file). Full rebuilds and the single-file watcher path were unaffected.
    #[test]
    fn batch_incremental_keeps_edges_through_unchanged_reexports() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fixture");
        let pkg = root.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("__init__.py"), "from pkg.a import helper\n").unwrap();
        std::fs::write(pkg.join("a.py"), "def helper(x):\n    return x\n").unwrap();
        let consumer =
            "from pkg import helper\n\n\ndef reexport_caller(x):\n    return helper(x)\n";
        std::fs::write(pkg.join("c.py"), consumer).unwrap();

        // Canonical root, as the facade guarantees before the pipeline
        // sees it: the Walk-lane module-path strip needs it.
        let root = root.canonicalize().unwrap();
        let settings = Arc::new(Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        });
        let index =
            Arc::new(DocumentIndex::new(settings.index_path.join("tantivy"), &settings).unwrap());
        let pipeline = Pipeline::with_settings(Arc::clone(&settings));

        // First pass force=true: production never runs incremental on an
        // empty index (the facade auto-forces at document_count == 0),
        // and only the force lane's Walk source computes module paths
        // here. The re-export arc must form on module identity, not on
        // the None/"" storage collapse this lock predated.
        pipeline
            .index_incremental(&root, Arc::clone(&index), None, None, true)
            .unwrap();
        assert!(
            calls_edge_exists(&index, "reexport_caller", "helper"),
            "first pass must resolve the re-export edge"
        );

        // Touch only the consumer; the __init__.py re-export stays unchanged.
        // Discovery short-circuits on second-granularity mtime equality, so
        // bump mtime past the first pass explicitly instead of racing it.
        std::fs::write(pkg.join("c.py"), format!("{consumer}\n# touched\n")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(pkg.join("c.py"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        let stats = pipeline
            .index_incremental(&root, Arc::clone(&index), None, None, false)
            .unwrap();
        assert_eq!(
            (stats.new_files, stats.modified_files, stats.deleted_files),
            (0, 1, 0),
            "touch must register as modified"
        );
        assert!(
            calls_edge_exists(&index, "reexport_caller", "helper"),
            "incremental pass must keep the edge through the unchanged re-export"
        );
    }
}
