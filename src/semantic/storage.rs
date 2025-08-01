//! Storage backend for semantic search embeddings using memory-mapped files.
//!
//! This module provides efficient persistence for semantic embeddings by leveraging
//! the existing MmapVectorStorage infrastructure, achieving <1μs access times.

use std::path::Path;
use crate::vector::{MmapVectorStorage, VectorDimension, VectorId, SegmentOrdinal};
use crate::{SymbolId, semantic::SemanticSearchError};

/// Wrapper around MmapVectorStorage specifically for semantic embeddings.
///
/// Provides a semantic-search-specific API while reusing the efficient
/// vector storage infrastructure.
#[derive(Debug)]
pub struct SemanticVectorStorage {
    storage: MmapVectorStorage,
    dimension: VectorDimension,
}

impl SemanticVectorStorage {
    /// Creates a new semantic vector storage.
    ///
    /// # Arguments
    /// * `path` - Base path for storage files
    /// * `dimension` - Dimension of embeddings (must match model output)
    pub fn new(path: &Path, dimension: VectorDimension) -> Result<Self, SemanticSearchError> {
        // First, remove any existing storage file to ensure clean state
        let storage_path = path.join("segment_0.vec");
        if storage_path.exists() {
            std::fs::remove_file(&storage_path)
                .map_err(|e| SemanticSearchError::StorageError {
                    message: format!("Failed to remove old storage: {}", e),
                    suggestion: "Check file permissions".to_string(),
                })?;
        }
        
        // Use segment 0 for semantic embeddings
        let storage = MmapVectorStorage::new(path, SegmentOrdinal::new(0), dimension)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to create storage: {}", e),
                suggestion: "Ensure the directory exists and you have write permissions".to_string(),
            })?;
        
        Ok(Self { storage, dimension })
    }
    
    /// Opens existing semantic vector storage.
    ///
    /// # Arguments
    /// * `path` - Base path where storage files exist
    pub fn open(path: &Path) -> Result<Self, SemanticSearchError> {
        let storage = MmapVectorStorage::open(path, SegmentOrdinal::new(0))
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to open storage: {}", e),
                suggestion: "Check if semantic search data exists at the specified path".to_string(),
            })?;
        
        let dimension = storage.dimension();
        Ok(Self { storage, dimension })
    }
    
    /// Opens existing storage or creates new if doesn't exist.
    pub fn open_or_create(path: &Path, dimension: VectorDimension) -> Result<Self, SemanticSearchError> {
        let storage = MmapVectorStorage::open_or_create(path, SegmentOrdinal::new(0), dimension)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to open or create storage: {}", e),
                suggestion: "Check path permissions and disk space".to_string(),
            })?;
        
        Ok(Self { storage, dimension })
    }
    
    /// Saves a single embedding.
    ///
    /// # Arguments
    /// * `id` - Symbol ID to associate with embedding
    /// * `embedding` - The embedding vector
    pub fn save_embedding(&mut self, id: SymbolId, embedding: &[f32]) -> Result<(), SemanticSearchError> {
        // Validate dimension
        if embedding.len() != self.dimension.get() {
            return Err(SemanticSearchError::DimensionMismatch {
                expected: self.dimension.get(),
                actual: embedding.len(),
                suggestion: "Ensure all embeddings are generated with the same model".to_string(),
            });
        }
        
        // Convert SymbolId to VectorId (both are u32 internally)
        let vector_id = VectorId::new(id.to_u32())
            .ok_or_else(|| SemanticSearchError::InvalidId {
                id: id.to_u32(),
                suggestion: "Symbol ID must be non-zero".to_string(),
            })?;
        
        // Save using batch API for efficiency
        self.storage.write_batch(&[(vector_id, embedding)])
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to save embedding: {}", e),
                suggestion: "Check disk space and file permissions".to_string(),
            })
    }
    
    /// Loads a single embedding by ID.
    ///
    /// Returns None if the embedding doesn't exist.
    pub fn load_embedding(&mut self, id: SymbolId) -> Option<Vec<f32>> {
        let vector_id = VectorId::new(id.to_u32())?;
        self.storage.read_vector(vector_id)
    }
    
    /// Loads all embeddings from storage.
    ///
    /// Returns a vector of (SymbolId, embedding) pairs.
    pub fn load_all(&mut self) -> Result<Vec<(SymbolId, Vec<f32>)>, SemanticSearchError> {
        let vectors = self.storage.read_all_vectors()
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to load embeddings: {}", e),
                suggestion: "The storage file may be corrupted. Try rebuilding the semantic index.".to_string(),
            })?;
        
        // Convert VectorId back to SymbolId
        let mut result = Vec::with_capacity(vectors.len());
        for (vector_id, embedding) in vectors {
            let symbol_id = SymbolId::new(vector_id.get()).unwrap();
            result.push((symbol_id, embedding));
        }
        
        Ok(result)
    }
    
    /// Saves multiple embeddings in batch.
    ///
    /// More efficient than calling save_embedding repeatedly.
    pub fn save_batch(&mut self, embeddings: &[(SymbolId, Vec<f32>)]) -> Result<(), SemanticSearchError> {
        // Validate all dimensions first
        for (_, embedding) in embeddings {
            if embedding.len() != self.dimension.get() {
                return Err(SemanticSearchError::DimensionMismatch {
                    expected: self.dimension.get(),
                    actual: embedding.len(),
                    suggestion: "All embeddings must have the same dimension".to_string(),
                });
            }
        }
        
        // Convert to vector storage format
        let mut vector_batch = Vec::with_capacity(embeddings.len());
        for (symbol_id, embedding) in embeddings {
            let vector_id = VectorId::new(symbol_id.to_u32())
                .ok_or_else(|| SemanticSearchError::InvalidId {
                    id: symbol_id.to_u32(),
                    suggestion: "Symbol ID must be non-zero".to_string(),
                })?;
            vector_batch.push((vector_id, embedding.as_slice()));
        }
        
        self.storage.write_batch(&vector_batch)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to save batch: {}", e),
                suggestion: "Check disk space and file permissions".to_string(),
            })
    }
    
    /// Returns the number of embeddings stored.
    pub fn embedding_count(&self) -> usize {
        self.storage.vector_count()
    }
    
    /// Returns the embedding dimension.
    pub fn dimension(&self) -> VectorDimension {
        self.dimension
    }
    
    /// Checks if the storage file exists.
    pub fn exists(&self) -> bool {
        self.storage.exists()
    }
    
    /// Returns the size of the storage file in bytes.
    pub fn file_size(&self) -> Result<u64, SemanticSearchError> {
        self.storage.file_size()
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to get file size: {}", e),
                suggestion: "Check if the storage file exists".to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_save_and_load_single_embedding() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(4).unwrap();
        
        let mut storage = SemanticVectorStorage::new(temp_dir.path(), dimension).unwrap();
        
        // Save an embedding
        let symbol_id = SymbolId::new(42).unwrap();
        let embedding = vec![1.0, 2.0, 3.0, 4.0];
        storage.save_embedding(symbol_id, &embedding).unwrap();
        
        // Load it back
        let loaded = storage.load_embedding(symbol_id).unwrap();
        assert_eq!(loaded, embedding);
        
        // Non-existent ID returns None
        assert!(storage.load_embedding(SymbolId::new(999).unwrap()).is_none());
    }
    
    #[test]
    fn test_load_all_embeddings() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(3).unwrap();
        
        let mut storage = SemanticVectorStorage::open_or_create(temp_dir.path(), dimension).unwrap();
        
        // Save multiple embeddings
        let embeddings = vec![
            (SymbolId::new(1).unwrap(), vec![1.0, 2.0, 3.0]),
            (SymbolId::new(2).unwrap(), vec![4.0, 5.0, 6.0]),
            (SymbolId::new(3).unwrap(), vec![7.0, 8.0, 9.0]),
        ];
        
        for (id, embedding) in &embeddings {
            storage.save_embedding(*id, embedding).unwrap();
        }
        
        // Load all
        let loaded = storage.load_all().unwrap();
        assert_eq!(loaded.len(), 3);
        
        // Verify all embeddings are present
        for (original_id, original_embedding) in &embeddings {
            let found = loaded.iter()
                .find(|(id, _)| id == original_id)
                .map(|(_, embedding)| embedding);
            assert_eq!(found, Some(original_embedding));
        }
    }
    
    #[test]
    fn test_dimension_validation() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(3).unwrap();
        
        let mut storage = SemanticVectorStorage::new(temp_dir.path(), dimension).unwrap();
        
        // Wrong dimension should fail
        let result = storage.save_embedding(
            SymbolId::new(1).unwrap(), 
            &vec![1.0, 2.0] // Too short
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SemanticSearchError::DimensionMismatch { expected, actual, .. } => {
                assert_eq!(expected, 3);
                assert_eq!(actual, 2);
            }
            _ => panic!("Expected DimensionMismatch error"),
        }
    }
    
    #[test]
    fn test_batch_operations() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(2).unwrap();
        
        let mut storage = SemanticVectorStorage::new(temp_dir.path(), dimension).unwrap();
        
        // Save batch
        let embeddings = vec![
            (SymbolId::new(10).unwrap(), vec![1.0, 2.0]),
            (SymbolId::new(20).unwrap(), vec![3.0, 4.0]),
            (SymbolId::new(30).unwrap(), vec![5.0, 6.0]),
        ];
        
        storage.save_batch(&embeddings).unwrap();
        assert_eq!(storage.embedding_count(), 3);
        
        // Verify individual loads
        assert_eq!(storage.load_embedding(SymbolId::new(10).unwrap()), Some(vec![1.0, 2.0]));
        assert_eq!(storage.load_embedding(SymbolId::new(20).unwrap()), Some(vec![3.0, 4.0]));
        assert_eq!(storage.load_embedding(SymbolId::new(30).unwrap()), Some(vec![5.0, 6.0]));
    }
    
    #[test]
    fn test_persistence_across_instances() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(2).unwrap();
        
        // First instance saves
        {
            let mut storage = SemanticVectorStorage::new(temp_dir.path(), dimension).unwrap();
            storage.save_embedding(SymbolId::new(42).unwrap(), &vec![1.5, 2.5]).unwrap();
        }
        
        // Second instance loads
        {
            let mut storage = SemanticVectorStorage::open(temp_dir.path()).unwrap();
            assert_eq!(storage.dimension(), dimension);
            assert_eq!(storage.embedding_count(), 1);
            
            let loaded = storage.load_embedding(SymbolId::new(42).unwrap()).unwrap();
            assert_eq!(loaded, vec![1.5, 2.5]);
        }
    }
}