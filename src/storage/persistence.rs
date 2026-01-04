//! Simplified persistence layer for Tantivy-only storage
//!
//! This module manages metadata and ensures Tantivy index exists.
//! All actual data is stored in Tantivy.

use crate::indexing::facade::IndexFacade;
use crate::storage::{DataSource, IndexMetadata};
use crate::{IndexError, IndexResult, Settings};
use std::path::PathBuf;
use std::sync::Arc;

/// Manages persistence of the index
#[derive(Debug)]
pub struct IndexPersistence {
    base_path: PathBuf,
}

impl IndexPersistence {
    /// Create a new persistence manager
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Get path for semantic search data
    fn semantic_path(&self) -> PathBuf {
        self.base_path.join("semantic")
    }

    // =========================================================================
    // IndexFacade Persistence Methods
    // =========================================================================

    /// Load an IndexFacade from disk
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load_facade(&self, settings: Arc<Settings>) -> IndexResult<IndexFacade> {
        // Load metadata to understand data sources
        let metadata = IndexMetadata::load(&self.base_path).ok();

        // Check if Tantivy index exists
        let tantivy_path = self.base_path.join("tantivy");
        if !tantivy_path.join("meta.json").exists() {
            return Err(IndexError::FileRead {
                path: tantivy_path,
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Tantivy index not found",
                ),
            });
        }

        // Create IndexFacade - it will open the existing Tantivy index
        let mut facade = IndexFacade::new(settings)?;

        // Display source info with fresh counts
        if let Some(ref meta) = metadata {
            let fresh_symbol_count = facade.symbol_count();
            let fresh_file_count = facade.file_count();

            match &meta.data_source {
                DataSource::Tantivy {
                    path, doc_count, ..
                } => {
                    tracing::info!(
                        "[persistence] loaded facade from Tantivy index: {} ({} documents)",
                        path.display(),
                        doc_count
                    );
                }
                DataSource::Fresh => {
                    tracing::info!("[persistence] created fresh facade");
                }
            }
            tracing::info!(
                "[persistence] facade contains {fresh_symbol_count} symbols from {fresh_file_count} files"
            );
        }

        // Load semantic search if available
        let semantic_path = self.semantic_path();
        tracing::debug!(
            "[persistence] semantic path computed as: {}",
            semantic_path.display()
        );
        match facade.load_semantic_search(&semantic_path) {
            Ok(true) => {
                tracing::debug!("[persistence] loaded semantic search for facade");
            }
            Ok(false) => {
                tracing::debug!("[persistence] no semantic data found (this is optional)");
            }
            Err(e) => {
                tracing::warn!("[persistence] failed to load semantic search: {e}");
            }
        }

        // Restore indexed_paths from metadata
        if let Some(ref meta) = metadata {
            if let Some(ref stored_paths) = meta.indexed_paths {
                facade.set_indexed_paths(stored_paths.clone());
                tracing::debug!(
                    "[persistence] restored {} indexed paths from metadata",
                    stored_paths.len()
                );
            }
        }

        Ok(facade)
    }

    /// Save metadata for an IndexFacade
    #[must_use = "Save errors should be handled to ensure data is persisted"]
    pub fn save_facade(&self, facade: &IndexFacade) -> IndexResult<()> {
        // Update metadata
        let mut metadata =
            IndexMetadata::load(&self.base_path).unwrap_or_else(|_| IndexMetadata::new());

        metadata.update_counts(facade.symbol_count() as u32, facade.file_count());

        // Update indexed paths for sync detection on next load
        let indexed_paths: Vec<PathBuf> = facade.get_indexed_paths().iter().cloned().collect();
        tracing::debug!(
            "[persistence] saving {} indexed paths to metadata",
            indexed_paths.len()
        );
        metadata.update_indexed_paths(indexed_paths);

        // Update metadata to reflect Tantivy
        metadata.data_source = DataSource::Tantivy {
            path: self.base_path.join("tantivy"),
            doc_count: facade.document_count().unwrap_or(0),
            timestamp: crate::indexing::get_utc_timestamp(),
        };

        metadata.save(&self.base_path)?;

        // Update project registry with latest metadata
        if let Err(err) = self.update_project_registry(&metadata) {
            tracing::debug!(
                target: "persistence",
                "Skipped project registry update: {err}"
            );
        }

        // Save semantic search if enabled
        if facade.has_semantic_search() {
            let semantic_path = self.semantic_path();
            std::fs::create_dir_all(&semantic_path).map_err(|e| {
                IndexError::General(format!("Failed to create semantic directory: {e}"))
            })?;

            facade
                .save_semantic_search(&semantic_path)
                .map_err(|e| IndexError::General(format!("Failed to save semantic search: {e}")))?;
        }

        Ok(())
    }

    /// Check if an index exists
    pub fn exists(&self) -> bool {
        // Check if Tantivy index exists
        let tantivy_path = self.base_path.join("tantivy");
        tantivy_path.join("meta.json").exists()
    }

    /// Delete the persisted index
    pub fn clear(&self) -> Result<(), std::io::Error> {
        let tantivy_path = self.base_path.join("tantivy");
        if tantivy_path.exists() {
            // On Windows, we may need multiple attempts due to file locking
            let mut attempts = 0;
            const MAX_ATTEMPTS: u32 = 3;

            loop {
                match std::fs::remove_dir_all(&tantivy_path) {
                    Ok(()) => break,
                    Err(e) if attempts < MAX_ATTEMPTS => {
                        attempts += 1;

                        // Retry logic for file locking issues
                        #[cfg(windows)]
                        {
                            // Windows-specific: Check for permission denied (code 5)
                            if e.kind() == std::io::ErrorKind::PermissionDenied {
                                eprintln!(
                                    "Attempt {attempts}/{MAX_ATTEMPTS}: Windows permission denied ({e}), retrying after delay..."
                                );

                                // Force garbage collection to release any handles
                                std::hint::black_box(());

                                // Brief delay to allow file handles to close
                                std::thread::sleep(std::time::Duration::from_millis(200));
                                continue;
                            }
                        }

                        // On non-Windows or non-permission errors, log and retry with delay
                        eprintln!(
                            "Attempt {attempts}/{MAX_ATTEMPTS}: Failed to remove directory ({e}), retrying..."
                        );
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            // Recreate the empty tantivy directory after clearing
            std::fs::create_dir_all(&tantivy_path)?;

            // On Windows, add extra delay after recreating directory to ensure filesystem is ready
            #[cfg(windows)]
            {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        Ok(())
    }

    /// Update the project registry with latest metadata
    fn update_project_registry(&self, metadata: &IndexMetadata) -> IndexResult<()> {
        // Try to read the project ID file
        let local_dir = crate::init::local_dir_name();
        let project_id_path = PathBuf::from(local_dir).join(".project-id");

        if !project_id_path.exists() {
            // No project ID file means project wasn't registered during init
            // This is fine for legacy projects
            return Ok(());
        }

        let project_id =
            std::fs::read_to_string(&project_id_path).map_err(|e| IndexError::FileRead {
                path: project_id_path.clone(),
                source: e,
            })?;

        // Load the registry
        let mut registry = crate::init::ProjectRegistry::load()
            .map_err(|e| IndexError::General(format!("Failed to load project registry: {e}")))?;

        // Update the project metadata
        if let Some(project) = registry.find_project_by_id_mut(&project_id) {
            project.symbol_count = metadata.symbol_count;
            project.file_count = metadata.file_count;
            project.last_modified = metadata.last_modified;

            // Get doc count from data source
            if let DataSource::Tantivy { doc_count, .. } = &metadata.data_source {
                project.doc_count = *doc_count;
            }

            // Save the updated registry
            registry.save().map_err(|e| {
                IndexError::General(format!("Failed to save project registry: {e}"))
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Check if semantic data exists (test helper)
    fn has_semantic_data(persistence: &IndexPersistence) -> bool {
        // Check if metadata exists - that's the definitive indicator
        persistence.semantic_path().join("metadata.json").exists()
    }

    #[test]
    fn test_exists() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = IndexPersistence::new(temp_dir.path().to_path_buf());

        // Initially doesn't exist
        assert!(!persistence.exists());

        // Create tantivy directory with meta.json
        let tantivy_path = temp_dir.path().join("tantivy");
        std::fs::create_dir_all(&tantivy_path).unwrap();
        std::fs::write(tantivy_path.join("meta.json"), "{}").unwrap();

        // Now it exists
        assert!(persistence.exists());
    }

    #[test]
    fn test_semantic_paths() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = IndexPersistence::new(temp_dir.path().to_path_buf());

        // Test semantic_path
        let semantic_path = persistence.semantic_path();
        assert_eq!(semantic_path, temp_dir.path().join("semantic"));

        // Initially has no semantic data
        assert!(!has_semantic_data(&persistence));

        // Create semantic directory and metadata file
        std::fs::create_dir_all(&semantic_path).unwrap();
        std::fs::write(semantic_path.join("metadata.json"), "{}").unwrap();

        // Now has semantic data
        assert!(has_semantic_data(&persistence));
    }
}
