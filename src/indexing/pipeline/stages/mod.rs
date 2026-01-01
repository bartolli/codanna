//! Pipeline stages
//!
//! Each stage is a separate module that can be tested independently.
//!
//! Phase 1 stages: DISCOVER → READ → PARSE → COLLECT → INDEX
//! Phase 2 stages: CONTEXT → RESOLVE → WRITE
//! Pre-phase: CLEANUP (for incremental mode)

pub mod cleanup;
pub mod collect;
pub mod context;
pub mod discover;
pub mod embed;
pub mod index;
pub mod parse;
pub mod read;
pub mod resolve;
pub mod write;

// Pre-phase stages (incremental mode)
pub use cleanup::{CleanupStage, CleanupStats};

// Phase 1 stages
pub use collect::CollectStage;
pub use discover::DiscoverStage;
pub use index::IndexStage;
pub use parse::{ParseStage, compute_hash, init_parser_cache, parse_file};
pub use read::ReadStage;

// Phase 2 stages
pub use context::{ContextStage, ContextStats};
pub use resolve::{ResolveStage, ResolveStats};
pub use write::{WriteStage, WriteStats};

// Embedding (separate from main pipeline)
pub use embed::{EmbedStage, EmbedStats};

// Re-export types from parent module for convenience
pub use super::types::SymbolLookupCache;
