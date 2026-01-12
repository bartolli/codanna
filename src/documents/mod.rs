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

pub use chunker::{Chunker, HybridChunker, RawChunk};
pub use config::{
    ChunkingConfig, ChunkingStrategy, CollectionConfig, DocumentsConfig, PreviewMode, SearchConfig,
};
pub use schema::DocumentSchema;
pub use store::{CollectionStats, DocumentStore, IndexProgress, SearchQuery, SearchResult};
pub use types::{ChunkId, CollectionId, DocumentChunk, FileState};

use crate::config::Settings;
use crate::vector::{EmbeddingGenerator, FastEmbedGenerator};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Load document store from settings if enabled and indexed.
///
/// Returns None if documents are disabled, index doesn't exist, or loading fails.
/// The returned Arc can be shared between MCP server and file watcher.
pub fn load_from_settings(settings: &Settings) -> Option<Arc<RwLock<DocumentStore>>> {
    if !settings.documents.enabled {
        tracing::debug!(target: "documents", "document store disabled in settings");
        return None;
    }

    let doc_path = settings.index_path.join("documents");
    if !doc_path.exists() {
        tracing::debug!(target: "documents", "document index not found at {}", doc_path.display());
        return None;
    }

    let generator = match FastEmbedGenerator::from_settings(&settings.semantic_search.model, false)
    {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(target: "documents", "failed to create embedding generator: {e}");
            return None;
        }
    };

    let dimension = generator.dimension();
    let store = match DocumentStore::new(&doc_path, dimension) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "documents", "failed to open document store: {e}");
            return None;
        }
    };

    let store_with_emb = match store.with_embeddings(Box::new(generator)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "documents", "failed to attach embeddings to store: {e}");
            return None;
        }
    };

    tracing::info!(target: "documents", "loaded document store from {}", doc_path.display());
    Some(Arc::new(RwLock::new(store_with_emb)))
}
