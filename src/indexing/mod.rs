pub mod facade;
pub mod file_info;
pub mod progress;
pub mod transaction;
pub mod walker;

// Parallel pipeline for high-performance indexing
pub mod pipeline;

// Re-exports
pub use file_info::{FileInfo, calculate_hash, get_utc_timestamp};
pub use progress::IndexStats;
pub use transaction::{FileTransaction, IndexTransaction};
pub use walker::FileWalker;

// Pipeline exports
pub use pipeline::{Pipeline, PipelineConfig};

// Facade - primary API for indexing operations
pub use facade::{FacadeResult, IndexFacade, IndexingStats, SyncStats};
