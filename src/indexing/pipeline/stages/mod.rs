//! Pipeline stages
//!
//! Each stage is a separate module that can be tested independently.

pub mod collect;
pub mod discover;
pub mod index;
pub mod parse;
pub mod read;

pub use collect::CollectStage;
pub use discover::DiscoverStage;
pub use index::IndexStage;
pub use parse::{ParseStage, compute_hash, init_parser_cache, parse_file};
pub use read::ReadStage;
