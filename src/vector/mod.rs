//! Vector search functionality for code intelligence.
//!
//! This module provides high-performance vector storage and search capabilities
//! designed to integrate with the existing Tantivy-based text search infrastructure.
//!
//! # Performance Targets
//! - Vector access: <1μs per vector
//! - Memory usage: ~100 bytes per symbol
//! - Indexing: 10,000+ files/second
//! - Search latency: <10ms for semantic search
//!
//! # Architecture
//! The vector search system uses IVFFlat (Inverted File with Flat vectors) indexing
//! with K-means clustering to achieve sub-linear search performance. Vectors are
//! stored in memory-mapped files for instant loading and minimal memory overhead.

mod clustering;
mod embedding;
mod engine;
mod storage;
mod types;

// Re-export core types for public API
pub use clustering::{assign_to_nearest_centroid, cosine_similarity, kmeans_clustering, ClusteringError, KMeansResult};
pub use embedding::{create_symbol_text, EmbeddingGenerator, FastEmbedGenerator};
#[cfg(test)]
pub use embedding::MockEmbeddingGenerator;
pub use engine::VectorSearchEngine;
pub use storage::{ConcurrentVectorStorage, MmapVectorStorage, VectorStorageError};
pub use types::{
    ClusterId, Score, SegmentOrdinal, VectorDimension, VectorError, VectorId,
    VECTOR_DIMENSION_384,
};