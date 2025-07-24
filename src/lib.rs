pub mod types;
pub mod symbol;
pub mod relationship;
pub mod storage;
pub mod parsing;
pub mod indexing;
pub mod config;
pub mod mcp;

pub use types::*;
pub use symbol::{Symbol, CompactSymbol, StringTable, Visibility};
pub use relationship::{Relationship, RelationKind, RelationshipEdge};
pub use storage::{SymbolStore, DependencyGraph, IndexPersistence, IndexData};
pub use parsing::RustParser;
pub use indexing::{SimpleIndexer, calculate_hash};
pub use config::Settings;