//! Handler implementations for the unified watcher.

mod code;
mod config;
mod document;

pub use code::CodeFileHandler;
pub use config::ConfigFileHandler;
pub use document::DocumentFileHandler;
