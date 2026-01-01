pub mod facade;
pub mod file_info;
pub mod progress;
pub mod simple;
pub mod transaction;
pub mod walker;

// Experimental parallel pipeline - completely isolated from simple indexer.
// Compare performance with: `time codanna index` vs `time codanna index-parallel`
pub mod pipeline;

#[cfg(test)]
pub mod import_resolution_proof;

// Re-exports for simple indexer (current production path)
pub use file_info::{FileInfo, calculate_hash, get_utc_timestamp};
pub use progress::IndexStats;
pub use simple::SimpleIndexer;
pub use transaction::{FileTransaction, IndexTransaction};
pub use walker::FileWalker;

// Re-exports for pipeline (experimental path) - intentionally separate namespace
pub use pipeline::{Pipeline, PipelineConfig};

// Re-export facade (bridge between SimpleIndexer API and Pipeline implementation)
pub use facade::{FacadeResult, IndexFacade, IndexingStats, SyncStats};
