//! Cleanup stage - removes symbols and embeddings for files
//!
//! This stage handles cleanup for:
//! - Deleted files: Files that existed in the index but no longer exist on disk
//! - Modified files: Files that will be re-indexed (old data must be removed first)
//!
//! The cleanup order is critical for embedding sync:
//! 1. Get symbols for file
//! 2. Remove embeddings for those symbols
//! 3. Save embeddings to disk (prevents desync on crash)
//! 4. Remove file documents from Tantivy

use crate::indexing::pipeline::types::{PipelineError, PipelineResult};
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use crate::types::SymbolId;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Statistics from cleanup operations.
#[derive(Debug, Default, Clone)]
pub struct CleanupStats {
    /// Number of files cleaned up.
    pub files_cleaned: usize,
    /// Number of symbols removed.
    pub symbols_removed: usize,
    /// Number of embeddings removed.
    pub embeddings_removed: usize,
}

/// Cleanup stage for removing old symbols and embeddings.
pub struct CleanupStage {
    index: Arc<DocumentIndex>,
    semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
    semantic_path: PathBuf,
}

impl CleanupStage {
    /// Create a new cleanup stage.
    pub fn new(index: Arc<DocumentIndex>, semantic_path: impl Into<PathBuf>) -> Self {
        Self {
            index,
            semantic: None,
            semantic_path: semantic_path.into(),
        }
    }

    /// Add semantic search for embedding cleanup.
    pub fn with_semantic(mut self, semantic: Arc<Mutex<SimpleSemanticSearch>>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Clean up files before re-indexing or deletion.
    ///
    /// This removes:
    /// - All symbols associated with the files
    /// - All embeddings for those symbols
    /// - File registrations from the index
    ///
    /// After cleanup, embeddings are saved to disk immediately to prevent desync.
    pub fn cleanup_files(&self, files: &[PathBuf]) -> PipelineResult<CleanupStats> {
        let mut stats = CleanupStats::default();

        for file in files {
            let file_stats = self.cleanup_single_file(file)?;
            stats.files_cleaned += 1;
            stats.symbols_removed += file_stats.0;
            stats.embeddings_removed += file_stats.1;
        }

        // Save embeddings to disk after all removals (critical for sync)
        if let Some(ref semantic) = self.semantic {
            let semantic_guard = semantic.lock().map_err(|_| PipelineError::Parse {
                path: PathBuf::new(),
                reason: "Failed to lock semantic search".to_string(),
            })?;

            semantic_guard
                .save(&self.semantic_path)
                .map_err(|e| PipelineError::Parse {
                    path: self.semantic_path.clone(),
                    reason: format!("Failed to save embeddings: {e}"),
                })?;
        }

        Ok(stats)
    }

    /// Clean up a single file.
    ///
    /// Returns (symbols_removed, embeddings_removed).
    fn cleanup_single_file(&self, path: &Path) -> PipelineResult<(usize, usize)> {
        let path_str = path.to_string_lossy();

        // Step 1: Get file_id from path
        let file_info = self.index.get_file_info(&path_str)?;
        let Some((file_id, _hash)) = file_info else {
            // File not in index, nothing to clean
            return Ok((0, 0));
        };

        // Step 2: Get all symbols for this file
        let symbols = self.index.find_symbols_by_file(file_id)?;
        let symbol_ids: Vec<SymbolId> = symbols.iter().map(|s| s.id).collect();
        let symbol_count = symbol_ids.len();

        // Step 3: Remove embeddings (if semantic search is enabled)
        let embedding_count = if let Some(ref semantic) = self.semantic {
            let mut semantic_guard = semantic.lock().map_err(|_| PipelineError::Parse {
                path: path.to_path_buf(),
                reason: "Failed to lock semantic search".to_string(),
            })?;

            semantic_guard.remove_embeddings(&symbol_ids);
            symbol_ids.len()
        } else {
            0
        };

        // Step 4: Remove relationships (both outgoing and incoming)
        // This garbage-collects orphaned refs when a symbol is renamed/deleted
        for symbol_id in &symbol_ids {
            self.index.delete_relationships_for_symbol(*symbol_id)?;
        }

        // Step 5: Remove file documents from Tantivy
        self.index.remove_file_documents(&path_str)?;

        Ok((symbol_count, embedding_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use tempfile::TempDir;

    #[test]
    fn test_cleanup_stage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let semantic_path = temp_dir.path().join("semantic");

        let stage = CleanupStage::new(index, semantic_path);

        // Cleanup empty list should succeed
        let result = stage.cleanup_files(&[]);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.files_cleaned, 0);
        assert_eq!(stats.symbols_removed, 0);
    }

    #[test]
    fn test_cleanup_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let semantic_path = temp_dir.path().join("semantic");

        let stage = CleanupStage::new(index, semantic_path);

        // Cleanup file not in index should succeed (no-op)
        let result = stage.cleanup_files(&[PathBuf::from("nonexistent.rs")]);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.files_cleaned, 1);
        assert_eq!(stats.symbols_removed, 0);
    }
}
