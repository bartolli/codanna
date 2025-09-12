/// The main library module for codanna
// Debug macro for consistent debug output
#[macro_export]
macro_rules! debug_print {
    ($self:expr, $($arg:tt)*) => {
        if $crate::config::is_global_debug_enabled() {
            eprintln!("DEBUG: {}", format!($($arg)*));
        }
    };
}

pub mod config;
pub mod display;
pub mod error;
pub mod indexing;
pub mod io;
pub mod mcp;
pub mod parsing;
pub mod project_resolver;
pub mod relationship;
pub mod retrieve;
pub mod semantic;
pub mod storage;
pub mod symbol;
pub mod types;
pub mod vector;

// Explicit exports for better API clarity
pub use config::Settings;
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
