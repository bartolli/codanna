//! Parallel pipeline indexing command.
//!
//! Uses the multi-stage parallel pipeline for indexing with two-phase resolution.
//! Supports incremental mode (detects changes) and full re-index (force mode).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::Settings;
use crate::indexing::pipeline::{IncrementalStats, Phase2Stats, Pipeline, PipelineConfig};
use crate::io::status_line::{ProgressBar, ProgressBarOptions, ProgressBarStyle};
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;

/// Arguments for the index-parallel command.
pub struct IndexParallelArgs {
    pub paths: Vec<PathBuf>,
    pub force: bool,
    pub progress: bool,
}

/// Run the parallel indexing command.
///
/// Uses the new incremental pipeline which:
/// - In force mode: full re-index of all files
/// - In incremental mode: detects new/modified/deleted files
/// - Generates embeddings for semantic search
/// - Runs two-phase relationship resolution
pub fn run(args: IndexParallelArgs, settings: &Settings) {
    let IndexParallelArgs {
        paths,
        force,
        progress,
    } = args;

    // Determine paths to index
    let paths_to_index: Vec<PathBuf> = if !paths.is_empty() {
        paths
    } else {
        settings.get_indexed_paths()
    };

    if paths_to_index.is_empty() {
        tracing::error!(target: "cli", "No paths to index. Use 'codanna index-parallel <path>' or 'codanna add-dir <path>'");
        std::process::exit(1);
    }

    // Resolve index path
    let index_path = settings.index_path.join("tantivy");
    let semantic_path = settings.index_path.join("semantic");

    // Clear existing index if force flag is set
    if force && index_path.exists() {
        tracing::info!(target: "pipeline", "Force re-indexing, clearing existing index");
        if let Err(e) = std::fs::remove_dir_all(&index_path) {
            tracing::warn!(target: "pipeline", "Failed to clear existing index: {e}");
        }
        if semantic_path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&semantic_path) {
                tracing::warn!(target: "pipeline", "Failed to clear semantic index: {e}");
            }
        }
    }

    // Create document index
    let index = match DocumentIndex::new(&index_path, settings) {
        Ok(idx) => Arc::new(idx),
        Err(e) => {
            tracing::error!(target: "pipeline", "Failed to create index: {e}");
            std::process::exit(1);
        }
    };

    // Create semantic search (for embeddings)
    let semantic = create_semantic_search(settings, &semantic_path);

    // Create pipeline
    let settings_arc = Arc::new(settings.clone());
    let config = PipelineConfig::from_settings(&settings_arc);
    let pipeline = Pipeline::new(Arc::clone(&settings_arc), config);

    let mode = if force { "force" } else { "incremental" };
    tracing::debug!(
        target: "pipeline",
        "Configuration ({mode}): parse_threads={}, read_threads={}, batch_size={}",
        pipeline.config().parse_threads,
        pipeline.config().read_threads,
        pipeline.config().batch_size
    );

    // Index each path using the incremental pipeline
    for path in &paths_to_index {
        if !path.exists() {
            tracing::error!(target: "cli", "Path does not exist: {}", path.display());
            std::process::exit(1);
        }

        if !path.is_dir() {
            tracing::error!(target: "cli", "Path is not a directory: {} (index-parallel only supports directories)", path.display());
            std::process::exit(1);
        }

        tracing::info!(target: "pipeline", "Indexing directory ({mode}): {}", path.display());

        match pipeline.index_incremental(path, Arc::clone(&index), semantic.clone(), None, force) {
            Ok(stats) => {
                display_incremental_stats(&stats, progress);
            }
            Err(e) => {
                tracing::error!(target: "pipeline", "Error indexing {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    }

    tracing::info!(target: "pipeline", "Index saved to: {}", index_path.display());
    if semantic.is_some() {
        tracing::info!(target: "pipeline", "Embeddings saved to: {}", semantic_path.display());
    }
}

/// Create semantic search instance if enabled in settings.
fn create_semantic_search(
    settings: &Settings,
    semantic_path: &Path,
) -> Option<Arc<Mutex<SimpleSemanticSearch>>> {
    if !settings.semantic_search.enabled {
        tracing::debug!(target: "pipeline", "Semantic search disabled");
        return None;
    }

    let model = &settings.semantic_search.model;

    // Try to load existing embeddings first
    if semantic_path.exists() {
        match SimpleSemanticSearch::load(semantic_path) {
            Ok(semantic) => {
                tracing::debug!(target: "pipeline", "Loaded existing embeddings from {}", semantic_path.display());
                return Some(Arc::new(Mutex::new(semantic)));
            }
            Err(e) => {
                tracing::warn!(target: "pipeline", "Failed to load embeddings: {e}");
            }
        }
    }

    // Create new semantic search instance
    match SimpleSemanticSearch::from_model_name(model) {
        Ok(semantic) => {
            tracing::debug!(target: "pipeline", "Created new semantic search with model: {model}");
            Some(Arc::new(Mutex::new(semantic)))
        }
        Err(e) => {
            tracing::warn!(target: "pipeline", "Failed to initialize semantic search: {e}");
            None
        }
    }
}

fn display_incremental_stats(stats: &IncrementalStats, with_progress: bool) {
    // Show cleanup stats if any files were cleaned up
    if stats.cleanup_stats.files_cleaned > 0 {
        tracing::info!(
            target: "pipeline",
            "Cleanup: {} files, {} symbols, {} embeddings removed",
            stats.cleanup_stats.files_cleaned,
            stats.cleanup_stats.symbols_removed,
            stats.cleanup_stats.embeddings_removed
        );
    }

    // Show Phase 1 stats with optional progress bar
    if with_progress && stats.index_stats.files_indexed > 0 {
        let options = ProgressBarOptions::default()
            .with_style(ProgressBarStyle::VerticalSolid)
            .with_width(28);
        let bar = ProgressBar::with_options(
            stats.index_stats.files_indexed as u64,
            "files",
            "indexed",
            "failed",
            options,
        );
        bar.set_progress(stats.index_stats.files_indexed as u64);
        bar.add_extra1(stats.index_stats.files_indexed as u64);
        bar.add_extra2(stats.index_stats.files_failed as u64);
        eprintln!("{bar}");
    }

    tracing::info!(
        target: "pipeline",
        "Phase 1: {} new, {} modified, {} deleted, {} indexed, {} symbols",
        stats.new_files,
        stats.modified_files,
        stats.deleted_files,
        stats.index_stats.files_indexed,
        stats.index_stats.symbols_found
    );

    if stats.index_stats.files_failed > 0 {
        tracing::warn!(target: "pipeline", "Phase 1: {} files failed", stats.index_stats.files_failed);
    }

    // Show Phase 2 stats
    display_phase2_stats(&stats.phase2_stats, with_progress);

    tracing::info!(target: "pipeline", "Total elapsed: {:?}", stats.elapsed);
}

fn display_phase2_stats(stats: &Phase2Stats, with_progress: bool) {
    let resolved = stats.defines_resolved + stats.calls_resolved + stats.other_resolved;

    if with_progress && stats.total_relationships > 0 {
        let options = ProgressBarOptions::default()
            .with_style(ProgressBarStyle::VerticalSolid)
            .with_width(28);
        let bar = ProgressBar::with_options(
            stats.total_relationships as u64,
            "relationships",
            "resolved",
            "unresolved",
            options,
        );
        bar.set_progress(stats.total_relationships as u64);
        bar.add_extra1(resolved as u64);
        bar.add_extra2(stats.unresolved as u64);
        eprintln!("{bar}");
    }

    tracing::info!(
        target: "pipeline",
        "Phase 2: {}/{} resolved ({} defines, {} calls, {} other) in {:?}",
        resolved,
        stats.total_relationships,
        stats.defines_resolved,
        stats.calls_resolved,
        stats.other_resolved,
        stats.elapsed
    );

    if stats.unresolved > 0 {
        tracing::debug!(target: "pipeline", "Phase 2: {} unresolved relationships", stats.unresolved);
    }
}
