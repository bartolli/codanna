//! Change-driven indexing: incremental runs, single-file reindex, config sync.

use super::stages::{CleanupStage, CollectStage, DiscoverStage, IndexStage, ReadStage};
use super::{
    CleanupStats, EmbedOptions, FileSource, IncrementalStats, ParseStage, Phase1Options,
    Phase2Stats, Pipeline, PipelineError, PipelineResult, ProgressSink, SingleFileStats,
    SymbolLookupCache, SyncStats, init_parser_cache,
};
use crate::FileId;
use crate::indexing::IndexStats;
use crate::io::status_line::DualProgressBar;
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

impl Pipeline {
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

            // File has changed - cleanup old data within a batch
            // Start batch for cleanup to avoid creating temporary writers
            index.start_batch()?;

            let cleanup_stage = if let Some(ref sem) = semantic {
                CleanupStage::new(Arc::clone(&index), &semantic_path).with_semantic(Arc::clone(sem))
            } else {
                CleanupStage::new(Arc::clone(&index), &semantic_path)
            };

            cleanup_stage.cleanup_files(&[normalized_path.to_path_buf()])?;

            // Commit cleanup changes before re-indexing
            index.commit_batch()?;
        }

        // Parse file
        init_parser_cache(Arc::clone(&self.settings));
        let parse_stage = ParseStage::new(Arc::clone(&self.settings));
        let parsed = parse_stage.parse(file_content)?;

        // Collect into a batch (now includes embedding candidates)
        let collect_stage = CollectStage::new(self.config.batch_size);
        let (batch, unresolved, embed_batch) =
            collect_stage.process_single(parsed, Arc::clone(&index))?;

        // Index the batch
        let index_stage = IndexStage::new(Arc::clone(&index), self.config.batches_per_commit);

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
        let phase2_stats = self.run_phase2(unresolved, symbol_cache, index)?;

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
            // Force mode: use DualProgressBar for semantic+embedding, else single bar
            let has_embedding = semantic.is_some() && embedding_pool.is_some();

            if has_embedding {
                // Dual progress: EMBED and INDEX running in parallel
                // Estimate embedding candidates = total_files (actual will vary based on symbols per file)
                let dual_bar = Arc::new(DualProgressBar::new(
                    "EMBED",
                    total_files as u64, // Estimated embedding candidates
                    "embedded",
                    "INDEX",
                    total_files as u64,
                    "files",
                ));
                let dual_status = StatusLine::new(Arc::clone(&dual_bar));

                let (stats, unresolved, cache, metrics) = self.run_phase1(
                    FileSource::Walk(root.to_path_buf()),
                    Arc::clone(&index),
                    Phase1Options {
                        progress: ProgressSink::Dual(dual_bar.clone()),
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
                eprintln!("{dual_bar}");

                let files_indexed = stats.files_indexed;
                (
                    stats,
                    unresolved,
                    cache,
                    CleanupStats::default(),
                    (files_indexed, 0, 0),
                )
            } else {
                // Single progress bar (no embedding or no semantic)
                let phase1_bar = Arc::new(ProgressBar::with_4_labels(
                    total_files as u64,
                    "files",
                    "indexed",
                    "failed",
                    "embedded",
                    bar_options,
                ));
                let phase1_status = StatusLine::new(Arc::clone(&phase1_bar));

                // has_embedding is false here, so semantic and pool are never
                // both present: the embed stage cannot run in this arm.
                let (stats, unresolved, cache, metrics) = self.run_phase1(
                    FileSource::Walk(root.to_path_buf()),
                    Arc::clone(&index),
                    Phase1Options {
                        progress: ProgressSink::Bar(phase1_bar.clone()),
                        embed: None,
                    },
                )?;

                // Drop StatusLine BEFORE logging to avoid stderr race condition
                drop(phase1_status);
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
            }
        } else {
            // Incremental mode: discover first, then create bar with actual count
            let discover_stage = DiscoverStage::new(root, self.config.discover_threads)
                .with_index(Arc::clone(&index))
                .with_workspace_root(self.settings.workspace_root.clone());
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

            let embed = match (&semantic, &embedding_pool) {
                (Some(sem), Some(pool)) => Some(EmbedOptions {
                    pool: Arc::clone(pool),
                    semantic: Arc::clone(sem),
                }),
                _ => None,
            };
            let (stats, unresolved, _run_cache, metrics) = self.run_phase1(
                FileSource::List(files_to_index),
                Arc::clone(&index),
                Phase1Options {
                    progress: ProgressSink::Bar(phase1_bar.clone()),
                    embed,
                },
            )?;

            // Drop StatusLine BEFORE logging to avoid stderr race condition
            drop(phase1_status);
            if let Some(m) = metrics {
                m.log();
            }
            eprintln!("{phase1_bar}");

            // Seed Phase 2 from the persisted index: the run-scoped cache
            // holds only this run's files, hiding unchanged files' symbols
            // and re-export aliases from resolution.
            let cache = SymbolLookupCache::from_index(&index)?;

            let counts = (
                discover_result.new_files.len(),
                discover_result.modified_files.len(),
                discover_result.deleted_files.len(),
            );
            (stats, unresolved, cache, cleanup_stats, counts)
        };

        // Run Phase 2 with separate progress bar
        let symbol_cache = Arc::new(symbol_cache);
        let phase2_stats =
            self.run_phase2_maybe_bar(unresolved, symbol_cache, Arc::clone(&index), true)?;

        // Save embeddings
        self.persist_embeddings(semantic.as_ref(), &semantic_path)?;

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
        let show_progress = progress.is_some();
        let embed = match (&semantic, &embedding_pool) {
            (Some(sem), Some(pool)) => Some(EmbedOptions {
                pool: Arc::clone(pool),
                semantic: Arc::clone(sem),
            }),
            _ => None,
        };
        let (index_stats, unresolved, _run_cache, metrics) = self.run_phase1(
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
        let phase2_stats =
            self.run_phase2_maybe_bar(unresolved, symbol_cache, Arc::clone(&index), show_progress)?;

        // Save embeddings
        self.persist_embeddings(semantic.as_ref(), &semantic_path)?;

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

        let settings = Arc::new(Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        });
        let index =
            Arc::new(DocumentIndex::new(settings.index_path.join("tantivy"), &settings).unwrap());
        let pipeline = Pipeline::with_settings(Arc::clone(&settings));

        pipeline
            .index_incremental(&root, Arc::clone(&index), None, None, false)
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
