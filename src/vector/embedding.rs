//! Embedding generation module for vector search functionality.
//!
//! This module provides the trait and implementations for generating
//! vector embeddings from code symbols. It uses fastembed for efficient
//! embedding generation with the AllMiniLML6V2 model.
//!
//! # SimpleIndexer Integration Design
//!
//! ## Integration Points in SimpleIndexer
//!
//! Based on analysis of `src/indexing/simple.rs`, here are the key integration points:
//!
//! ### 1. Add Vector Engine Field (line ~50)
//! ```rust,ignore
//! pub struct SimpleIndexer {
//!     // ... existing fields ...
//!     /// Optional vector search engine for semantic search
//!     vector_engine: Option<Arc<VectorSearchEngine>>,
//!     /// Embedding generator for creating vectors from symbols
//!     embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
//!     /// Pending symbols for batch embedding
//!     pending_symbols: Vec<(SymbolId, String)>,
//! }
//! ```
//!
//! ### 2. Constructor Extension (line ~56)
//! Add a method to enable vector search:
//! ```rust,ignore
//! impl SimpleIndexer {
//!     pub fn with_vector_search(mut self, vector_path: PathBuf) -> Result<Self, VectorError> {
//!         let engine = Arc::new(VectorSearchEngine::new(vector_path, VectorDimension::dimension_384())?);
//!         let generator = Arc::new(FastEmbedGenerator::new()?);
//!         self.vector_engine = Some(engine);
//!         self.embedding_generator = Some(generator);
//!         self.pending_symbols = Vec::new();
//!         Ok(self)
//!     }
//! }
//! ```
//!
//! ### 3. Symbol Processing Hook (line ~336)
//! In `extract_and_store_symbols`, after storing each symbol:
//! ```rust,ignore
//! // After self.store_symbol(symbol, path_str)?;
//! if self.vector_engine.is_some() {
//!     let symbol_text = create_symbol_text(&symbol.name, symbol.kind, symbol.signature.as_deref());
//!     self.pending_symbols.push((symbol.id, symbol_text));
//! }
//! ```
//!
//! ### 4. Batch Commit Hook (line ~118)
//! In `commit_tantivy_batch`, trigger vector indexing:
//! ```rust,ignore
//! pub fn commit_tantivy_batch(&self) -> IndexResult<()> {
//!     // First commit Tantivy changes
//!     self.document_index.commit_batch()?;
//!     
//!     // Then process pending vectors if enabled
//!     if let (Some(engine), Some(generator)) = (&self.vector_engine, &self.embedding_generator) {
//!         if !self.pending_symbols.is_empty() {
//!             self.process_pending_vectors(engine, generator)?;
//!         }
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ### 5. Vector Processing Method
//! ```rust,ignore
//! fn process_pending_vectors(
//!     &mut self,
//!     engine: &Arc<VectorSearchEngine>,
//!     generator: &Arc<dyn EmbeddingGenerator>,
//! ) -> IndexResult<()> {
//!     // Extract texts for embedding
//!     let texts: Vec<&str> = self.pending_symbols.iter()
//!         .map(|(_, text)| text.as_str())
//!         .collect();
//!     
//!     // Generate embeddings in batch
//!     let embeddings = generator.generate_embeddings(&texts)
//!         .map_err(|e| IndexError::General(format!("Embedding generation failed: {}", e)))?;
//!     
//!     // Create (VectorId, Vec<f32>) pairs
//!     let mut vector_pairs = Vec::new();
//!     for ((symbol_id, _), embedding) in self.pending_symbols.iter().zip(embeddings.iter()) {
//!         // Map SymbolId to VectorId (same value, different type)
//!         let vector_id = VectorId::new(symbol_id.0)?;
//!         vector_pairs.push((vector_id, embedding.clone()));
//!     }
//!     
//!     // Index vectors with clustering
//!     engine.index_vectors(&vector_pairs)
//!         .map_err(|e| IndexError::General(format!("Vector indexing failed: {}", e)))?;
//!     
//!     // Clear pending symbols
//!     self.pending_symbols.clear();
//!     
//!     Ok(())
//! }
//! ```
//!
//! ### 6. SymbolId to VectorId Mapping
//! Since both SymbolId and VectorId wrap u32 values, we can use a 1:1 mapping:
//! - SymbolId(42) → VectorId(42)
//! - This preserves the relationship between symbols and their vectors
//! - No additional mapping table needed
//!
//! ### 7. Batch Size Configuration
//! Add to Settings or use a constant:
//! ```rust,ignore
//! const VECTOR_BATCH_SIZE: usize = 1000; // Process vectors in batches of 1000
//! 
//! // In process_pending_vectors:
//! if self.pending_symbols.len() >= VECTOR_BATCH_SIZE {
//!     self.process_pending_vectors(engine, generator)?;
//! }
//! ```
//!
//! ### 8. File Removal Hook (line ~248)
//! When removing file symbols, also remove vectors:
//! ```rust,ignore
//! fn remove_file_symbols(&mut self, file_id: FileId) -> IndexResult<()> {
//!     let symbols = self.document_index.find_symbols_by_file(file_id)?;
//!     
//!     // Remove vectors if engine is enabled
//!     if let Some(engine) = &self.vector_engine {
//!         let vector_ids: Vec<VectorId> = symbols.iter()
//!             .filter_map(|s| VectorId::new(s.id.0))
//!             .collect();
//!         // TODO: Add remove_vectors method to VectorSearchEngine
//!     }
//!     
//!     // ... existing symbol removal code ...
//! }
//! ```
//!
//! ## Summary
//! 
//! The integration follows these principles:
//! 1. **Optional Enhancement**: Vector search is opt-in via `with_vector_search()`
//! 2. **Batch Processing**: Symbols accumulate and process in batches for efficiency
//! 3. **Atomic Commits**: Vectors index after Tantivy commits for consistency
//! 4. **Simple Mapping**: Direct SymbolId → VectorId mapping (no lookup table)
//! 5. **Clean Separation**: Vector logic isolated in vector module

use crate::vector::{VectorError, VectorDimension, VECTOR_DIMENSION_384};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Mutex;

/// Trait for generating embeddings from text.
///
/// Implementations of this trait should be thread-safe and
/// capable of handling batch processing efficiently.
pub trait EmbeddingGenerator: Send + Sync {
    /// Generate embeddings for multiple texts.
    ///
    /// # Arguments
    /// * `texts` - Slice of text strings to generate embeddings for
    ///
    /// # Returns
    /// A vector of embeddings, one for each input text, or an error
    fn generate_embeddings(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, VectorError>;
    
    /// Get the dimension of embeddings produced by this generator.
    #[must_use]
    fn dimension(&self) -> VectorDimension;
}

/// FastEmbed implementation using AllMiniLML6V2 model.
///
/// This implementation produces 384-dimensional embeddings optimized
/// for semantic similarity of code snippets.
///
/// # Performance
/// - Batch processing: ~1-10ms per embedding on average
/// - Memory: 384 * 4 bytes = 1536 bytes per embedding
pub struct FastEmbedGenerator {
    model: Mutex<TextEmbedding>,
    dimension: VectorDimension,
}

impl FastEmbedGenerator {
    /// Create a new FastEmbed generator with AllMiniLML6V2 model.
    ///
    /// # Errors
    /// Returns an error if the model fails to initialize or download.
    pub fn new() -> Result<Self, VectorError> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false),
        )
        .map_err(|e| VectorError::EmbeddingFailed(
            format!("Failed to initialize embedding model: {}. Ensure you have internet connection for first-time model download", e)
        ))?;
        
        Ok(Self {
            model: Mutex::new(model),
            dimension: VectorDimension::dimension_384(),
        })
    }
    
    /// Create a new generator with progress display during model download.
    ///
    /// # Errors
    /// Returns an error if the model fails to initialize or download.
    pub fn new_with_progress() -> Result<Self, VectorError> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(true),
        )
        .map_err(|e| VectorError::EmbeddingFailed(
            format!("Failed to initialize embedding model: {}. Ensure you have internet connection for first-time model download", e)
        ))?;
        
        Ok(Self {
            model: Mutex::new(model),
            dimension: VectorDimension::dimension_384(),
        })
    }
}

impl EmbeddingGenerator for FastEmbedGenerator {
    fn generate_embeddings(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, VectorError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        
        // fastembed expects Vec<String> for the embed method
        // TODO: Future optimization - investigate if fastembed can accept &[&str] directly
        // to avoid these allocations
        let text_strings: Vec<String> = texts.iter().map(|&s| s.to_string()).collect();
        
        // Generate embeddings
        let embeddings = self.model
            .lock()
            .map_err(|_| VectorError::EmbeddingFailed(
                "Failed to acquire embedding model lock - model may be poisoned".to_string()
            ))?
            .embed(text_strings, None)
            .map_err(|e| VectorError::EmbeddingFailed(
                format!("Failed to generate embeddings: {}", e)
            ))?;
        
        // Validate dimensions
        for embedding in embeddings.iter() {
            if embedding.len() != VECTOR_DIMENSION_384 {
                return Err(VectorError::DimensionMismatch {
                    expected: VECTOR_DIMENSION_384,
                    actual: embedding.len(),
                });
            }
        }
        
        Ok(embeddings)
    }
    
    fn dimension(&self) -> VectorDimension {
        self.dimension
    }
}

/// Mock embedding generator for testing.
///
/// This implementation generates deterministic embeddings based on
/// text content, useful for unit tests and integration testing.
#[cfg(test)]
pub struct MockEmbeddingGenerator {
    dimension: VectorDimension,
}

#[cfg(test)]
impl MockEmbeddingGenerator {
    /// Create a new mock generator with standard 384 dimensions.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dimension: VectorDimension::dimension_384(),
        }
    }
    
    /// Create a generator with custom dimension for testing.
    #[must_use]
    pub fn with_dimension(dimension: VectorDimension) -> Self {
        Self { dimension }
    }
}

#[cfg(test)]
impl EmbeddingGenerator for MockEmbeddingGenerator {
    fn generate_embeddings(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, VectorError> {
        let dim = self.dimension.get();
        let mut embeddings = Vec::new();
        
        for text in texts {
            // Create deterministic embeddings based on text content
            let mut embedding = vec![0.1; dim];
            
            // Add patterns based on common code terms for testing
            if text.contains("parse") || text.contains("Parse") {
                if dim > 1 {
                    embedding[0] = 0.9;
                    embedding[1] = 0.8;
                }
            }
            if text.contains("json") || text.contains("JSON") {
                if dim > 3 {
                    embedding[2] = 0.85;
                    embedding[3] = 0.75;
                }
            }
            if text.contains("error") || text.contains("Error") {
                if dim > 5 {
                    embedding[4] = 0.8;
                    embedding[5] = 0.7;
                }
            }
            if text.contains("async") {
                if dim > 7 {
                    embedding[6] = 0.9;
                    embedding[7] = 0.85;
                }
            }
            
            // Normalize to unit length (like real embeddings)
            let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if magnitude > 0.0 {
                for val in &mut embedding {
                    *val /= magnitude;
                }
            }
            
            embeddings.push(embedding);
        }
        
        Ok(embeddings)
    }
    
    fn dimension(&self) -> VectorDimension {
        self.dimension
    }
}

/// Helper to create symbol text for embedding.
///
/// Combines symbol name, kind, and signature into a single text
/// representation optimized for semantic search.
///
/// # Example
/// ```ignore
/// let text = create_symbol_text("parse_json", SymbolKind::Function, Some("fn parse_json(input: &str) -> Result<Value>"));
/// // Returns: "function parse_json fn parse_json(input: &str) -> Result<Value>"
/// ```
#[must_use]
pub fn create_symbol_text(
    name: &str,
    kind: crate::types::SymbolKind,
    signature: Option<&str>,
) -> String {
    let kind_str = match kind {
        crate::types::SymbolKind::Function => "function",
        crate::types::SymbolKind::Method => "method",
        crate::types::SymbolKind::Struct => "struct",
        crate::types::SymbolKind::Enum => "enum",
        crate::types::SymbolKind::Trait => "trait",
        crate::types::SymbolKind::TypeAlias => "type_alias",
        crate::types::SymbolKind::Variable => "variable",
        crate::types::SymbolKind::Constant => "constant",
        crate::types::SymbolKind::Module => "module",
        crate::types::SymbolKind::Macro => "macro",
        crate::types::SymbolKind::Interface => "interface",
        crate::types::SymbolKind::Class => "class",
        crate::types::SymbolKind::Field => "field",
        crate::types::SymbolKind::Parameter => "parameter",
    };
    
    if let Some(sig) = signature {
        format!("{} {} {}", kind_str, name, sig)
    } else {
        format!("{} {}", kind_str, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mock_embedding_generator() {
        let generator = MockEmbeddingGenerator::new();
        
        // Test single embedding
        let texts = vec!["fn parse_json(input: &str) -> Result<Value>"];
        let embeddings = generator.generate_embeddings(&texts).unwrap();
        
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), VECTOR_DIMENSION_384);
        
        // Verify normalization
        let magnitude: f32 = embeddings[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.01);
    }
    
    #[test]
    fn test_mock_batch_embeddings() {
        let generator = MockEmbeddingGenerator::new();
        
        let texts = vec![
            "fn parse_json(input: &str) -> Result<Value>",
            "struct JsonError { message: String }",
            "async fn fetch_data() -> Result<Data>",
        ];
        
        let embeddings = generator.generate_embeddings(&texts).unwrap();
        
        assert_eq!(embeddings.len(), 3);
        for embedding in &embeddings {
            assert_eq!(embedding.len(), VECTOR_DIMENSION_384);
        }
    }
    
    #[test]
    fn test_create_symbol_text() {
        use crate::types::SymbolKind;
        
        let text = create_symbol_text(
            "parse_json",
            SymbolKind::Function,
            Some("fn parse_json(input: &str) -> Result<Value>"),
        );
        assert_eq!(text, "function parse_json fn parse_json(input: &str) -> Result<Value>");
        
        let text = create_symbol_text("Point", SymbolKind::Struct, None);
        assert_eq!(text, "struct Point");
    }
}