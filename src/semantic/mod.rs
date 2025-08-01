//! Semantic search functionality for documentation comments
//! 
//! This module provides a simple API for semantic search on documentation,
//! designed to integrate with the existing indexing system.

mod simple;

pub use simple::{SimpleSemanticSearch, SemanticSearchError};

// Re-export key types
pub use fastembed::{EmbeddingModel, TextEmbedding};

/// Similarity threshold recommendations based on testing
pub mod thresholds {
    /// Threshold for very similar documents (e.g., same concept, different wording)
    pub const VERY_SIMILAR: f32 = 0.85;
    
    /// Threshold for similar documents (e.g., related concepts)
    pub const SIMILAR: f32 = 0.70;
    
    /// Threshold for somewhat related documents
    pub const RELATED: f32 = 0.50;
    
    /// Default threshold for semantic search
    pub const DEFAULT: f32 = SIMILAR;
}