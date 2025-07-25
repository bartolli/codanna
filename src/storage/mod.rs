pub mod memory;
pub mod graph;
pub mod persistence;
pub mod metadata;
pub mod tantivy;
pub mod error;
pub mod metadata_keys;

// pub use memory::SymbolStore; // No longer used with Tantivy-only architecture
// pub use graph::DependencyGraph; // No longer used with Tantivy-only architecture
pub use persistence::IndexPersistence;
pub use metadata::{IndexMetadata, DataSource};
pub use tantivy::{DocumentIndex, SearchResult};
pub use error::{StorageError, StorageResult};
pub use metadata_keys::MetadataKey;