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
mod full;
mod incremental;
pub mod metrics;
mod phase1;
mod phase2;
pub mod stages;
mod stats;
pub mod types;
mod workers;

pub use config::PipelineConfig;
pub use metrics::{PipelineMetrics, StageMetrics, StageTracker};
pub use stages::cleanup::{CleanupStage, CleanupStats};
pub use stages::context::{ContextStage, ContextStats};
pub use stages::embed::{EmbedStage, EmbedStats};
pub use stages::parse::{ParseStage, init_parser_cache, parse_file};
pub use stages::resolve::{ResolveStage, ResolveStats};
pub use stages::semantic_embed::{SemanticEmbedStage, SemanticEmbedStats};
pub use stages::write::{WriteStage, WriteStats};
pub use stats::{IncrementalStats, Phase2Stats, PipelineStats, StageTimings, SyncStats};
pub use types::{
    DiscoverResult, EmbedOptions, EmbeddingBatch, FileBindings, FileContent, FileRegistration,
    FileSource, IndexBatch, ParsedFile, Phase1Options, PipelineError, PipelineResult, ProgressSink,
    RawImport, RawRelationship, RawSymbol, ResolutionContext, ResolvedBatch, ResolvedRelationship,
    SingleFileStats, SymbolLookupCache, UnresolvedRelationship, VariableBinding,
};

use crate::Settings;
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

    // ─────────────────────────────────────────────────────────────────────────────
    // Helper methods for consistent data flow
    // ─────────────────────────────────────────────────────────────────────────────

    /// Query starting ID counters from the index.
    ///
    /// Must be called BEFORE spawning threads to avoid ID collisions
    /// in multi-directory or incremental indexing.
    fn get_start_counters(&self, index: &DocumentIndex) -> PipelineResult<(u32, u32)> {
        let file_id = index.get_next_file_id()?.saturating_sub(1);
        let symbol_id = index.get_next_symbol_id()?.saturating_sub(1);
        Ok((file_id, symbol_id))
    }

    /// Save final counter values to metadata.
    ///
    /// Must be called AFTER all stages complete to persist counters
    /// for the next indexing run.
    fn save_final_counters(
        &self,
        index: &DocumentIndex,
        file_count: u32,
        symbol_count: u32,
    ) -> PipelineResult<()> {
        use crate::storage::MetadataKey;
        index.start_batch()?;
        index.store_metadata(MetadataKey::FileCounter, u64::from(file_count))?;
        index.store_metadata(MetadataKey::SymbolCounter, u64::from(symbol_count))?;
        index.commit_batch()?;
        Ok(())
    }

    /// Save embeddings to disk.
    ///
    /// One policy for every composition path: log at error and propagate.
    /// A poisoned lock propagates the same way. No-op without semantic search.
    fn persist_embeddings(
        &self,
        semantic: Option<&Arc<Mutex<SimpleSemanticSearch>>>,
        semantic_path: &Path,
    ) -> PipelineResult<()> {
        let Some(sem) = semantic else {
            return Ok(());
        };
        let guard = sem.lock().map_err(|_| PipelineError::Parse {
            path: PathBuf::new(),
            reason: "Failed to lock semantic search".to_string(),
        })?;
        guard.save(semantic_path).map_err(|e| {
            tracing::error!(target: "pipeline", "Failed to save embeddings: {e}");
            PipelineError::Parse {
                path: semantic_path.to_path_buf(),
                reason: format!("Failed to save embeddings: {e}"),
            }
        })
    }
}
