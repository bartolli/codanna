//! Document chunking and embedding for RAG use cases.
//!
//! This module provides:
//! - Document chunking with configurable strategies
//! - Vector embeddings for document chunks
//! - Collection-based organization and filtering
//! - Semantic search within document collections

pub mod chunker;
pub mod config;
pub mod schema;
pub mod store;
pub mod types;
pub mod watcher;

pub use chunker::{Chunker, HybridChunker, RawChunk};
pub use config::{
    ChunkingConfig, ChunkingStrategy, CollectionConfig, DocumentsConfig, PreviewMode, SearchConfig,
};
pub use schema::DocumentSchema;
pub use store::{CollectionStats, DocumentStore, IndexProgress, SearchQuery, SearchResult};
pub use types::{ChunkId, CollectionId, DocumentChunk, FileState};
pub use watcher::DocumentWatcher;
