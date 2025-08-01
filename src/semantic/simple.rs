//! Simple semantic search implementation for documentation comments

use std::collections::HashMap;
use std::sync::Mutex;
use std::path::Path;
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use crate::SymbolId;

/// Error type for semantic search operations
#[derive(Debug, thiserror::Error)]
pub enum SemanticSearchError {
    #[error("Failed to initialize embedding model: {0}")]
    ModelInitError(String),
    
    #[error("Failed to generate embedding: {0}")]
    EmbeddingError(String),
    
    #[error("No embeddings available for search")]
    NoEmbeddings,
    
    #[error("Storage error: {message}\nSuggestion: {suggestion}")]
    StorageError {
        message: String,
        suggestion: String,
    },
    
    #[error("Dimension mismatch: expected {expected}, got {actual}\nSuggestion: {suggestion}")]
    DimensionMismatch {
        expected: usize,
        actual: usize,
        suggestion: String,
    },
    
    #[error("Invalid ID: {id}\nSuggestion: {suggestion}")]
    InvalidId {
        id: u32,
        suggestion: String,
    },
}

/// Advanced semantic search engine for documentation analysis
/// 
/// This implementation uses state-of-the-art embeddings to find
/// semantically similar documentation across the entire codebase,
/// enabling natural language queries for code discovery.
/// Updated: Final test - embedding cleanup working correctly!
pub struct SimpleSemanticSearch {
    /// Embeddings indexed by symbol ID
    embeddings: HashMap<SymbolId, Vec<f32>>,
    
    /// The embedding model (wrapped in Mutex for interior mutability)
    model: Mutex<TextEmbedding>,
    
    /// Model dimensions for validation
    dimensions: usize,
}

impl std::fmt::Debug for SimpleSemanticSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleSemanticSearch")
            .field("embeddings_count", &self.embeddings.len())
            .field("dimensions", &self.dimensions)
            .field("model", &"<TextEmbedding>")
            .finish()
    }
}

impl SimpleSemanticSearch {
    /// Create a new semantic search instance
    /// 
    /// Uses AllMiniLML6V2 model based on our testing results
    pub fn new() -> Result<Self, SemanticSearchError> {
        Self::with_model(EmbeddingModel::AllMiniLML6V2)
    }
    
    /// Create with a specific model (for testing)
    pub fn with_model(model: EmbeddingModel) -> Result<Self, SemanticSearchError> {
        let mut text_model = TextEmbedding::try_new(
            InitOptions::new(model).with_show_download_progress(true)
        ).map_err(|e| SemanticSearchError::ModelInitError(e.to_string()))?;
        
        // Get dimensions by generating a test embedding
        let test_embedding = text_model.embed(vec!["test"], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
        let dimensions = test_embedding.into_iter().next().unwrap().len();
        
        Ok(Self {
            embeddings: HashMap::new(),
            model: Mutex::new(text_model),
            dimensions,
        })
    }
    
    /// Index a documentation comment for a symbol
    pub fn index_doc_comment(&mut self, symbol_id: SymbolId, doc: &str) -> Result<(), SemanticSearchError> {
        // Skip empty docs
        if doc.trim().is_empty() {
            return Ok(());
        }
        
        // Generate embedding
        let embeddings = self.model.lock().unwrap()
            .embed(vec![doc], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
        
        let embedding = embeddings.into_iter().next().unwrap();
        
        // Validate dimensions
        if embedding.len() != self.dimensions {
            return Err(SemanticSearchError::EmbeddingError(
                format!("Embedding dimension mismatch: expected {}, got {}", 
                        self.dimensions, embedding.len())
            ));
        }
        
        self.embeddings.insert(symbol_id, embedding);
        Ok(())
    }
    
    /// Search for similar documentation using a natural language query
    /// 
    /// Returns symbol IDs with their similarity scores, sorted by score descending
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        if self.embeddings.is_empty() {
            return Err(SemanticSearchError::NoEmbeddings);
        }
        
        // Generate query embedding
        let query_embeddings = self.model.lock().unwrap()
            .embed(vec![query], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
        let query_embedding = query_embeddings.into_iter().next().unwrap();
        
        // Calculate similarities
        let mut similarities: Vec<(SymbolId, f32)> = self.embeddings
            .iter()
            .map(|(id, embedding)| {
                let similarity = cosine_similarity(&query_embedding, embedding);
                (*id, similarity)
            })
            .collect();
        
        // Sort by similarity descending
        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Return top results
        similarities.truncate(limit);
        Ok(similarities)
    }
    
    /// Search with a similarity threshold
    pub fn search_with_threshold(&self, query: &str, limit: usize, threshold: f32) 
        -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        let results = self.search(query, limit)?;
        Ok(results.into_iter()
            .filter(|(_, score)| *score >= threshold)
            .collect())
    }
    
    /// Get the number of indexed embeddings
    pub fn embedding_count(&self) -> usize {
        self.embeddings.len()
    }
    
    /// Clear all embeddings
    pub fn clear(&mut self) {
        self.embeddings.clear();
    }
    
    /// Remove embeddings for specific symbols
    /// 
    /// This is used when re-indexing files to remove embeddings for symbols
    /// that no longer exist.
    pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) {
        for id in symbol_ids {
            self.embeddings.remove(id);
        }
    }
    
    /// Save embeddings to disk using the efficient vector storage
    /// 
    /// # Arguments
    /// * `path` - Path where semantic data should be stored
    pub fn save(&self, path: &Path) -> Result<(), SemanticSearchError> {
        use crate::semantic::{SemanticVectorStorage, SemanticMetadata};
        use crate::vector::VectorDimension;
        
        // Ensure the directory exists
        std::fs::create_dir_all(path)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to create semantic directory: {}", e),
                suggestion: "Check directory permissions".to_string(),
            })?;
        
        // Save metadata
        let metadata = SemanticMetadata::new(
            "AllMiniLML6V2".to_string(), // TODO: Make this configurable
            self.dimensions,
            self.embeddings.len()
        );
        metadata.save(path)?;
        
        // Create storage with our dimension
        let dimension = VectorDimension::new(self.dimensions)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Invalid dimension: {}", e),
                suggestion: "Dimension must be between 1 and 4096".to_string(),
            })?;
        
        let mut storage = SemanticVectorStorage::new(path, dimension)?;
        
        // Convert HashMap to Vec for batch save
        let embeddings: Vec<(SymbolId, Vec<f32>)> = self.embeddings
            .iter()
            .map(|(id, embedding)| (*id, embedding.clone()))
            .collect();
        
        // Save all embeddings
        storage.save_batch(&embeddings)?;
        
        Ok(())
    }
    
    /// Load embeddings from disk
    /// 
    /// # Arguments
    /// * `path` - Path where semantic data is stored
    pub fn load(path: &Path) -> Result<Self, SemanticSearchError> {
        use crate::semantic::{SemanticVectorStorage, SemanticMetadata};
        
        // Load metadata first
        let metadata = SemanticMetadata::load(path)?;
        
        // Verify model compatibility (for now we only support AllMiniLML6V2)
        if metadata.model_name != "AllMiniLML6V2" {
            return Err(SemanticSearchError::StorageError {
                message: format!("Unsupported model: {}", metadata.model_name),
                suggestion: "Only AllMiniLML6V2 is currently supported".to_string(),
            });
        }
        
        // Open existing storage
        let mut storage = SemanticVectorStorage::open(path)?;
        
        // Verify dimension matches
        if storage.dimension().get() != metadata.dimension {
            return Err(SemanticSearchError::DimensionMismatch {
                expected: metadata.dimension,
                actual: storage.dimension().get(),
                suggestion: "Metadata and storage dimension mismatch. The index may be corrupted.".to_string(),
            });
        }
        
        // Load all embeddings
        let embeddings_vec = storage.load_all()?;
        
        // Verify count matches metadata
        if embeddings_vec.len() != metadata.embedding_count {
            eprintln!("WARNING: Expected {} embeddings but found {}", 
                     metadata.embedding_count, embeddings_vec.len());
        }
        
        // Convert to HashMap
        let mut embeddings = HashMap::with_capacity(embeddings_vec.len());
        for (id, embedding) in embeddings_vec {
            embeddings.insert(id, embedding);
        }
        
        // Create new instance with same model
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false)
        ).map_err(|e| SemanticSearchError::ModelInitError(e.to_string()))?;
        
        Ok(Self {
            embeddings,
            model: Mutex::new(model),
            dimensions: metadata.dimension,
        })
    }
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (magnitude_a * magnitude_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_remove_embeddings() {
        let mut search = SimpleSemanticSearch::new().unwrap();
        
        // Add some embeddings with distinct content
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        
        search.index_doc_comment(id1, "Parse JSON data from file").unwrap();
        search.index_doc_comment(id2, "Connect to database server").unwrap();
        search.index_doc_comment(id3, "Calculate hash of string").unwrap();
        
        assert_eq!(search.embedding_count(), 3);
        
        // Remove specific embeddings
        search.remove_embeddings(&[id1, id3]);
        
        assert_eq!(search.embedding_count(), 1);
        
        // Verify correct embedding was kept - search for database content
        let results = search.search("database connection", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id2);
        
        // Verify we can't find removed content with good similarity
        let json_results = search.search_with_threshold("parse JSON", 10, 0.6).unwrap();
        assert!(json_results.is_empty(), "Should not find removed JSON parsing doc");
        
        let hash_results = search.search_with_threshold("calculate hash", 10, 0.6).unwrap();
        assert!(hash_results.is_empty(), "Should not find removed hash calculation doc");
    }
    
    #[test]
    fn test_save_and_load() {
        use tempfile::TempDir;
        
        let temp_dir = TempDir::new().unwrap();
        
        // Create and populate search instance
        let mut search = SimpleSemanticSearch::new().unwrap();
        
        // Index some test data
        search.index_doc_comment(
            SymbolId::new(1).unwrap(),
            "This function parses JSON data"
        ).unwrap();
        
        search.index_doc_comment(
            SymbolId::new(2).unwrap(),
            "Authenticates a user with credentials"
        ).unwrap();
        
        let original_count = search.embedding_count();
        
        // Save to disk
        search.save(temp_dir.path()).unwrap();
        
        // Load from disk
        let loaded = SimpleSemanticSearch::load(temp_dir.path()).unwrap();
        
        // Verify same number of embeddings
        assert_eq!(loaded.embedding_count(), original_count);
        
        // Verify search still works
        let results = loaded.search("parse JSON", 10).unwrap();
        assert!(!results.is_empty());
        
        // The first result should be our JSON parsing function
        assert_eq!(results[0].0, SymbolId::new(1).unwrap());
    }
    
    #[test]
    fn test_load_missing_file() {
        use tempfile::TempDir;
        
        let temp_dir = TempDir::new().unwrap();
        
        // Try to load from non-existent path
        let result = SimpleSemanticSearch::load(temp_dir.path());
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SemanticSearchError::StorageError { .. } => {},
            _ => panic!("Expected StorageError"),
        }
    }
    
    #[test]
    fn test_semantic_search_basic() {
        let mut search = SimpleSemanticSearch::new().unwrap();
        
        // Index some doc comments
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        
        search.index_doc_comment(id1, "Parse JSON data from a string").unwrap();
        search.index_doc_comment(id2, "Serialize data structure to JSON").unwrap();
        search.index_doc_comment(id3, "Calculate factorial of a number").unwrap();
        
        // Search for JSON-related functions
        let results = search.search("parse JSON", 3).unwrap();
        
        // First two should be JSON-related
        assert!(results[0].1 > 0.7); // High similarity
        assert!(results[1].1 > 0.5); // Moderate similarity
        assert!(results[2].1 < 0.3); // Low similarity (factorial)
        
        // The parse function should be most similar
        assert_eq!(results[0].0, id1);
    }
    
    #[test]
    fn test_similarity_threshold() {
        let mut search = SimpleSemanticSearch::new().unwrap();
        
        // Index test data
        search.index_doc_comment(SymbolId::new(1).unwrap(), "Authentication and authorization").unwrap();
        search.index_doc_comment(SymbolId::new(2).unwrap(), "User login and authentication").unwrap();
        search.index_doc_comment(SymbolId::new(3).unwrap(), "Matrix multiplication algorithm").unwrap();
        
        // Search with threshold
        let results = search.search_with_threshold("user authentication", 10, 0.5).unwrap();
        
        // Should only return auth-related results
        assert_eq!(results.len(), 2);
        for (_, score) in &results {
            assert!(*score >= 0.5);
        }
    }
    
    #[test]
    fn test_cosine_similarity() {
        // Identical vectors
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v1, &v2) - 1.0).abs() < 0.001);
        
        // Orthogonal vectors
        let v3 = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&v1, &v3) - 0.0).abs() < 0.001);
        
        // Opposite vectors
        let v4 = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v1, &v4) - (-1.0)).abs() < 0.001);
    }
}