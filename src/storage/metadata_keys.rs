//! Metadata keys used in Tantivy storage

use std::fmt;

/// Strongly-typed metadata keys to avoid string literals
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataKey {
    /// Counter for next file ID
    FileCounter,
    /// Counter for next symbol ID  
    SymbolCounter,
}

impl MetadataKey {
    /// Get the string key for Tantivy storage
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FileCounter => "file_counter",
            Self::SymbolCounter => "symbol_counter",
        }
    }
}

impl fmt::Display for MetadataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}