//! Simple semantic search implementation for documentation comments

use std::collections::HashMap;
use std::sync::Mutex;
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
}

/// Simple semantic search for documentation comments
/// 
/// This is a minimal implementation focused on proving the concept
/// with doc comments before expanding to full symbol search.
pub struct SimpleSemanticSearch {
    /// Embeddings indexed by symbol ID
    embeddings: HashMap<SymbolId, Vec<f32>>,
    
    /// The embedding model (wrapped in Mutex for interior mutability)
    model: Mutex<TextEmbedding>,
    
    /// Model dimensions for validation
    dimensions: usize,
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