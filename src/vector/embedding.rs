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

use crate::vector::{VectorDimension, VectorError};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Mutex;

/// Parse a model name string into an EmbeddingModel enum.
///
/// # Arguments
/// * `model_name` - String name of the model (e.g., "AllMiniLML6V2", "MultilingualE5Small")
///
/// # Returns
/// The corresponding EmbeddingModel enum variant, or an error if the model name is not recognized
///
/// # Supported Models
/// ## English Models
/// - `AllMiniLML6V2` - Sentence Transformer, 384 dimensions (default)
/// - `BGESmallENV15` - BAAI BGE English small, 384 dimensions
/// - `BGEBaseENV15` - BAAI BGE English base, 768 dimensions
/// - `BGELargeENV15` - BAAI BGE English large, 1024 dimensions
///
/// ## Multilingual Models (94 languages)
/// - `MultilingualE5Small` - intfloat E5 small, 384 dimensions (recommended for multilingual)
/// - `MultilingualE5Base` - intfloat E5 base, 768 dimensions
/// - `MultilingualE5Large` - intfloat E5 large, 1024 dimensions
///
/// ## Chinese Models
/// - `BGESmallZHV15` - BAAI BGE Chinese small, 512 dimensions
/// - `BGELargeZHV15` - BAAI BGE Chinese large, 1024 dimensions
///
/// ## Code-Specialized Models
/// - `JinaEmbeddingsV2BaseCode` - Jina code embeddings, 768 dimensions
///
/// # Example
/// ```ignore
/// let model = parse_embedding_model("MultilingualE5Small")?;
/// ```
pub fn parse_embedding_model(model_name: &str) -> Result<EmbeddingModel, VectorError> {
    match model_name {
        // English models
        "AllMiniLML6V2" => Ok(EmbeddingModel::AllMiniLML6V2),
        "AllMiniLML6V2Q" => Ok(EmbeddingModel::AllMiniLML6V2Q),
        "AllMiniLML12V2" => Ok(EmbeddingModel::AllMiniLML12V2),
        "AllMiniLML12V2Q" => Ok(EmbeddingModel::AllMiniLML12V2Q),
        "BGEBaseENV15" => Ok(EmbeddingModel::BGEBaseENV15),
        "BGEBaseENV15Q" => Ok(EmbeddingModel::BGEBaseENV15Q),
        "BGELargeENV15" => Ok(EmbeddingModel::BGELargeENV15),
        "BGELargeENV15Q" => Ok(EmbeddingModel::BGELargeENV15Q),
        "BGESmallENV15" => Ok(EmbeddingModel::BGESmallENV15),
        "BGESmallENV15Q" => Ok(EmbeddingModel::BGESmallENV15Q),
        "NomicEmbedTextV1" => Ok(EmbeddingModel::NomicEmbedTextV1),
        "NomicEmbedTextV15" => Ok(EmbeddingModel::NomicEmbedTextV15),
        "NomicEmbedTextV15Q" => Ok(EmbeddingModel::NomicEmbedTextV15Q),
        "ParaphraseMLMiniLML12V2" => Ok(EmbeddingModel::ParaphraseMLMiniLML12V2),
        "ParaphraseMLMiniLML12V2Q" => Ok(EmbeddingModel::ParaphraseMLMiniLML12V2Q),
        "ParaphraseMLMpnetBaseV2" => Ok(EmbeddingModel::ParaphraseMLMpnetBaseV2),
        "AllMpnetBaseV2" => Ok(EmbeddingModel::AllMpnetBaseV2),

        // Multilingual models (94 languages)
        "MultilingualE5Small" => Ok(EmbeddingModel::MultilingualE5Small),
        "MultilingualE5Base" => Ok(EmbeddingModel::MultilingualE5Base),
        "MultilingualE5Large" => Ok(EmbeddingModel::MultilingualE5Large),

        // Chinese models
        "BGESmallZHV15" => Ok(EmbeddingModel::BGESmallZHV15),
        "BGELargeZHV15" => Ok(EmbeddingModel::BGELargeZHV15),

        // Other specialized models
        "ModernBertEmbedLarge" => Ok(EmbeddingModel::ModernBertEmbedLarge),
        "MxbaiEmbedLargeV1" => Ok(EmbeddingModel::MxbaiEmbedLargeV1),
        "MxbaiEmbedLargeV1Q" => Ok(EmbeddingModel::MxbaiEmbedLargeV1Q),
        "GTEBaseENV15" => Ok(EmbeddingModel::GTEBaseENV15),
        "GTEBaseENV15Q" => Ok(EmbeddingModel::GTEBaseENV15Q),
        "GTELargeENV15" => Ok(EmbeddingModel::GTELargeENV15),
        "GTELargeENV15Q" => Ok(EmbeddingModel::GTELargeENV15Q),
        "ClipVitB32" => Ok(EmbeddingModel::ClipVitB32),
        "JinaEmbeddingsV2BaseCode" => Ok(EmbeddingModel::JinaEmbeddingsV2BaseCode),
        "EmbeddingGemma300M" => Ok(EmbeddingModel::EmbeddingGemma300M),

        _ => Err(VectorError::EmbeddingFailed(format!(
            "Unknown embedding model: '{model_name}'. Supported models: AllMiniLML6V2, MultilingualE5Small, MultilingualE5Base, MultilingualE5Large, BGESmallZHV15, BGELargeZHV15, JinaEmbeddingsV2BaseCode, and more. See documentation for full list."
        ))),
    }
}

/// Get the model name as a string from an EmbeddingModel enum.
///
/// This is useful for saving model information to metadata.
pub fn model_to_string(model: &EmbeddingModel) -> String {
    match model {
        EmbeddingModel::AllMiniLML6V2 => "AllMiniLML6V2",
        EmbeddingModel::AllMiniLML6V2Q => "AllMiniLML6V2Q",
        EmbeddingModel::AllMiniLML12V2 => "AllMiniLML12V2",
        EmbeddingModel::AllMiniLML12V2Q => "AllMiniLML12V2Q",
        EmbeddingModel::BGEBaseENV15 => "BGEBaseENV15",
        EmbeddingModel::BGEBaseENV15Q => "BGEBaseENV15Q",
        EmbeddingModel::BGELargeENV15 => "BGELargeENV15",
        EmbeddingModel::BGELargeENV15Q => "BGELargeENV15Q",
        EmbeddingModel::BGESmallENV15 => "BGESmallENV15",
        EmbeddingModel::BGESmallENV15Q => "BGESmallENV15Q",
        EmbeddingModel::NomicEmbedTextV1 => "NomicEmbedTextV1",
        EmbeddingModel::NomicEmbedTextV15 => "NomicEmbedTextV15",
        EmbeddingModel::NomicEmbedTextV15Q => "NomicEmbedTextV15Q",
        EmbeddingModel::ParaphraseMLMiniLML12V2 => "ParaphraseMLMiniLML12V2",
        EmbeddingModel::ParaphraseMLMiniLML12V2Q => "ParaphraseMLMiniLML12V2Q",
        EmbeddingModel::ParaphraseMLMpnetBaseV2 => "ParaphraseMLMpnetBaseV2",
        EmbeddingModel::AllMpnetBaseV2 => "AllMpnetBaseV2",
        EmbeddingModel::MultilingualE5Small => "MultilingualE5Small",
        EmbeddingModel::MultilingualE5Base => "MultilingualE5Base",
        EmbeddingModel::MultilingualE5Large => "MultilingualE5Large",
        EmbeddingModel::BGESmallZHV15 => "BGESmallZHV15",
        EmbeddingModel::BGELargeZHV15 => "BGELargeZHV15",
        EmbeddingModel::ModernBertEmbedLarge => "ModernBertEmbedLarge",
        EmbeddingModel::MxbaiEmbedLargeV1 => "MxbaiEmbedLargeV1",
        EmbeddingModel::MxbaiEmbedLargeV1Q => "MxbaiEmbedLargeV1Q",
        EmbeddingModel::GTEBaseENV15 => "GTEBaseENV15",
        EmbeddingModel::GTEBaseENV15Q => "GTEBaseENV15Q",
        EmbeddingModel::GTELargeENV15 => "GTELargeENV15",
        EmbeddingModel::GTELargeENV15Q => "GTELargeENV15Q",
        EmbeddingModel::ClipVitB32 => "ClipVitB32",
        EmbeddingModel::JinaEmbeddingsV2BaseCode => "JinaEmbeddingsV2BaseCode",
        EmbeddingModel::EmbeddingGemma300M => "EmbeddingGemma300M",
        // Handle any new models added in future fastembed versions
        other => return format!("{other:?}"),
    }
    .to_string()
}

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

/// FastEmbed implementation with configurable embedding models.
///
/// This implementation supports multiple embedding models for different use cases:
/// - English-only models: AllMiniLML6V2 (default), BGE series
/// - Multilingual models: MultilingualE5 series (94 languages)
/// - Code-specialized models: JinaEmbeddingsV2BaseCode
///
/// # Performance
/// - Batch processing: ~1-10ms per embedding on average
/// - Memory: dimension * 4 bytes per embedding
pub struct FastEmbedGenerator {
    model: Mutex<TextEmbedding>,
    dimension: VectorDimension,
    model_name: String,
}

impl FastEmbedGenerator {
    /// Create a new FastEmbed generator with AllMiniLML6V2 model (default).
    ///
    /// # Errors
    /// Returns an error if the model fails to initialize or download.
    pub fn new() -> Result<Self, VectorError> {
        Self::with_model(EmbeddingModel::AllMiniLML6V2, false)
    }

    /// Create a new generator with progress display during model download.
    ///
    /// # Errors
    /// Returns an error if the model fails to initialize or download.
    pub fn new_with_progress() -> Result<Self, VectorError> {
        Self::with_model(EmbeddingModel::AllMiniLML6V2, true)
    }

    /// Create a new generator with a specific model.
    ///
    /// # Arguments
    /// * `model` - The embedding model to use
    /// * `show_progress` - Whether to show download progress
    ///
    /// # Errors
    /// Returns an error if the model fails to initialize or download.
    pub fn with_model(model: EmbeddingModel, show_progress: bool) -> Result<Self, VectorError> {
        let model_name = model_to_string(&model);

        let mut text_model = TextEmbedding::try_new(
            InitOptions::new(model)
                .with_cache_dir(crate::init::models_dir())
                .with_show_download_progress(show_progress),
        )
        .map_err(|e| VectorError::EmbeddingFailed(
            format!("Failed to initialize embedding model '{model_name}': {e}. Ensure you have internet connection for first-time model download")
        ))?;

        // Auto-detect dimension by generating a test embedding
        let test_embedding = text_model.embed(vec!["test"], None).map_err(|e| {
            VectorError::EmbeddingFailed(format!("Failed to detect model dimensions: {e}"))
        })?;

        let dimension_size = test_embedding.into_iter().next().unwrap().len();
        let dimension = VectorDimension::new(dimension_size).map_err(|e| {
            VectorError::EmbeddingFailed(format!("Invalid dimension size {dimension_size}: {e}"))
        })?;

        Ok(Self {
            model: Mutex::new(text_model),
            dimension,
            model_name,
        })
    }

    /// Create a generator from settings.
    ///
    /// Reads the model name from settings and initializes the appropriate model.
    ///
    /// # Arguments
    /// * `model_name` - Model name from settings (e.g., "MultilingualE5Small")
    /// * `show_progress` - Whether to show download progress
    ///
    /// # Errors
    /// Returns an error if the model name is invalid or initialization fails.
    pub fn from_settings(model_name: &str, show_progress: bool) -> Result<Self, VectorError> {
        let model = parse_embedding_model(model_name)?;
        Self::with_model(model, show_progress)
    }

    /// Get the name of the model being used.
    pub fn model_name(&self) -> &str {
        &self.model_name
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
        let embeddings = self
            .model
            .lock()
            .map_err(|_| {
                VectorError::EmbeddingFailed(
                    "Failed to acquire embedding model lock - model may be poisoned".to_string(),
                )
            })?
            .embed(text_strings, None)
            .map_err(|e| {
                VectorError::EmbeddingFailed(format!("Failed to generate embeddings: {e}"))
            })?;

        // Validate dimensions
        let expected_dim = self.dimension.get();
        for embedding in embeddings.iter() {
            if embedding.len() != expected_dim {
                return Err(VectorError::DimensionMismatch {
                    expected: expected_dim,
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
impl Default for MockEmbeddingGenerator {
    fn default() -> Self {
        Self::new()
    }
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
            if (text.contains("parse") || text.contains("Parse")) && dim > 1 {
                embedding[0] = 0.9;
                embedding[1] = 0.8;
            }
            if (text.contains("json") || text.contains("JSON")) && dim > 3 {
                embedding[2] = 0.85;
                embedding[3] = 0.75;
            }
            if (text.contains("error") || text.contains("Error")) && dim > 5 {
                embedding[4] = 0.8;
                embedding[5] = 0.7;
            }
            if text.contains("async") && dim > 7 {
                embedding[6] = 0.9;
                embedding[7] = 0.85;
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
        format!("{kind_str} {name} {sig}")
    } else {
        format!("{kind_str} {name}")
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
        assert_eq!(embeddings[0].len(), generator.dimension().get());

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
            assert_eq!(embedding.len(), generator.dimension().get());
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
        assert_eq!(
            text,
            "function parse_json fn parse_json(input: &str) -> Result<Value>"
        );

        let text = create_symbol_text("Point", SymbolKind::Struct, None);
        assert_eq!(text, "struct Point");
    }
}
