pub mod types;
pub mod symbol;
pub mod relationship;
pub mod storage;
pub mod parsing;
pub mod indexing;
pub mod config;
pub mod mcp;
pub mod error;
pub mod vector;
pub mod semantic;

// Explicit exports for better API clarity
pub use types::{SymbolId, FileId, Range, SymbolKind, IndexingResult, CompactString, compact_string};
pub use symbol::{Symbol, CompactSymbol, StringTable, Visibility};
pub use relationship::{Relationship, RelationKind, RelationshipEdge};
pub use storage::{IndexPersistence};
pub use parsing::RustParser;
pub use indexing::{SimpleIndexer, calculate_hash};
pub use config::Settings;
pub use error::{IndexError, ParseError, StorageError, McpError, IndexResult, ParseResult, StorageResult, McpResult};