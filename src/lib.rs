/// The main library module for codanna
// Alias for tree-sitter-kotlin dependency
// When upstream publishes 0.3.9+, change Cargo.toml and update this line:
// extern crate tree_sitter_kotlin;
extern crate tree_sitter_kotlin_codanna as tree_sitter_kotlin;

pub mod cli;
pub mod config;
pub mod display;
pub mod documents;
pub mod error;
pub mod indexing;
pub mod init;
pub mod io;
pub mod logging;
pub mod mcp;
pub mod parsing;
pub mod plugins;
pub mod profiles;
pub mod project_resolver;
pub mod relationship;
pub mod retrieve;
pub mod semantic;
pub mod storage;
pub mod symbol;
pub mod types;
pub mod utils;
pub mod vector;
pub mod watcher;

// Explicit exports for better API clarity
pub use config::{LoggingConfig, Settings};
pub use error::{
    IndexError, IndexResult, McpError, McpResult, ParseError, ParseResult, StorageError,
    StorageResult,
};
pub use indexing::{SimpleIndexer, calculate_hash};
pub use parsing::RustParser;
pub use relationship::{RelationKind, Relationship, RelationshipEdge};
pub use storage::IndexPersistence;
pub use symbol::{CompactSymbol, ScopeContext, StringTable, Symbol, Visibility};
pub use types::{
    CompactString, FileId, IndexingResult, Range, SymbolId, SymbolKind, compact_string,
};
