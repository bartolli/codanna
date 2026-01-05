//! Semantic search functionality for documentation comments
//!
//! This module provides a simple API for semantic search on documentation,
//! designed to integrate with the existing indexing system.

mod metadata;
mod pool;
mod simple;
mod storage;

pub use metadata::SemanticMetadata;
pub use pool::EmbeddingPool;
pub use simple::{SemanticSearchError, SimpleSemanticSearch};
pub use storage::SemanticVectorStorage;

// Re-export key types
pub use fastembed::{EmbeddingModel, TextEmbedding};

/// Similarity threshold recommendations based on testing
pub mod thresholds {
    /// Threshold for very similar documents (e.g., same concept, different wording)
    pub const VERY_SIMILAR: f32 = 0.75;

    /// Threshold for similar documents (e.g., related concepts)
    pub const SIMILAR: f32 = 0.60;

    /// Threshold for somewhat related documents
    pub const RELATED: f32 = 0.40;

    /// Default threshold for semantic search
    pub const DEFAULT: f32 = SIMILAR;
}
