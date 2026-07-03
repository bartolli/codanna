//! Force-mode composition: full walk plus resolution.

use super::{
    CleanupStats, EmbedOptions, FileSource, IncrementalStats, Phase1Options, Phase2Stats, Pipeline,
    PipelineResult, ProgressSink,
};
use crate::indexing::IndexStats;
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

impl Pipeline {
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

    /// Full index (force mode): index all files without incremental detection.
    pub(super) fn index_full(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        embedding_pool: Option<Arc<crate::semantic::EmbeddingBackend>>,
        semantic_path: &Path,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<IncrementalStats> {
        let start = Instant::now();
        let show_progress = progress.is_some();

        // Run Phase 1 with embedding iff semantic search and pool are both present
        let embed = match (&semantic, &embedding_pool) {
            (Some(sem), Some(pool)) => Some(EmbedOptions {
                pool: Arc::clone(pool),
                semantic: Arc::clone(sem),
            }),
            _ => None,
        };
        let (index_stats, unresolved, symbol_cache, metrics) = self.run_phase1(
            FileSource::Walk(root.to_path_buf()),
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

        // Run Phase 2 resolution with progress if Phase 1 had progress
        let symbol_cache = Arc::new(symbol_cache);
        let phase2_stats =
            self.run_phase2_maybe_bar(unresolved, symbol_cache, Arc::clone(&index), show_progress)?;

        // Save embeddings
        self.persist_embeddings(semantic.as_ref(), semantic_path)?;

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
}
