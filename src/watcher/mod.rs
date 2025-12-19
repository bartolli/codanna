//! Unified file watcher system for automatic re-indexing.
//!
//! This module provides a single file watcher that routes events to
//! pluggable handlers for code files, documents, and configuration.
//!
//! # Architecture
//!
//! ```text
//! UnifiedWatcher
//!   - Single notify::RecommendedWatcher
//!   - Shared PathRegistry (interned paths)
//!   - Shared Debouncer
//!   - Routes events to handlers
//!         |
//!    +---------+---------+
//!    |         |         |
//! CodeHandler DocHandler ConfigHandler
//! ```

mod debouncer;
mod error;
mod handler;
pub mod handlers;
mod path_registry;
mod unified;

pub use debouncer::Debouncer;
pub use error::WatchError;
pub use handler::{WatchAction, WatchHandler};
pub use path_registry::PathRegistry;
pub use unified::{UnifiedWatcher, UnifiedWatcherBuilder};
