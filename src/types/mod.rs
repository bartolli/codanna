use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

/// Result of an indexing operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexingResult {
    /// File was newly indexed
    Indexed(FileId),
    /// File was loaded from cache (unchanged)
    Cached(FileId),
}

impl IndexingResult {
    pub fn file_id(&self) -> FileId {
        match self {
            IndexingResult::Indexed(id) => *id,
            IndexingResult::Cached(id) => *id,
        }
    }
    
    pub fn is_cached(&self) -> bool {
        matches!(self, IndexingResult::Cached(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start_line: u32,
    pub start_column: u16,
    pub end_line: u32,
    pub end_column: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Interface,
    Class,
    Module,
    Variable,
    Constant,
    Field,
    Parameter,
    TypeAlias,
    Macro,
}

impl SymbolId {
    pub fn new(value: u32) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
    
    /// Convert to the underlying u32 value
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

impl FileId {
    pub fn new(value: u32) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
    
    /// Convert to the underlying u32 value
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

impl Range {
    pub fn new(start_line: u32, start_column: u16, end_line: u32, end_column: u16) -> Self {
        Self {
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    pub fn contains(&self, line: u32, column: u16) -> bool {
        if line < self.start_line || line > self.end_line {
            return false;
        }
        
        if line == self.start_line && column < self.start_column {
            return false;
        }
        
        if line == self.end_line && column > self.end_column {
            return false;
        }
        
        true
    }
}

impl FromStr for SymbolKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Function" => Ok(SymbolKind::Function),
            "Method" => Ok(SymbolKind::Method),
            "Struct" => Ok(SymbolKind::Struct),
            "Enum" => Ok(SymbolKind::Enum),
            "Trait" => Ok(SymbolKind::Trait),
            "Interface" => Ok(SymbolKind::Interface),
            "Class" => Ok(SymbolKind::Class),
            "Module" => Ok(SymbolKind::Module),
            "Variable" => Ok(SymbolKind::Variable),
            "Constant" => Ok(SymbolKind::Constant),
            "Field" => Ok(SymbolKind::Field),
            "Parameter" => Ok(SymbolKind::Parameter),
            "TypeAlias" => Ok(SymbolKind::TypeAlias),
            "Macro" => Ok(SymbolKind::Macro),
            _ => Err("Unknown symbol kind"),
        }
    }
}

impl SymbolKind {
    /// Parse from string with a default fallback for unknown values
    pub fn from_str_with_default(s: &str) -> Self {
        s.parse().unwrap_or(SymbolKind::Function)
    }
}

pub type CompactString = Box<str>;

pub fn compact_string(s: &str) -> CompactString {
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_id_creation() {
        assert!(SymbolId::new(0).is_none());
        
        let id = SymbolId::new(42).unwrap();
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn test_file_id_creation() {
        assert!(FileId::new(0).is_none());
        
        let id = FileId::new(100).unwrap();
        assert_eq!(id.value(), 100);
    }

    #[test]
    fn test_range_creation() {
        let range = Range::new(10, 5, 15, 20);
        assert_eq!(range.start_line, 10);
        assert_eq!(range.start_column, 5);
        assert_eq!(range.end_line, 15);
        assert_eq!(range.end_column, 20);
    }

    #[test]
    fn test_range_contains() {
        let range = Range::new(10, 5, 15, 20);
        
        // Inside range
        assert!(range.contains(12, 10));
        assert!(range.contains(10, 5)); // Start position
        assert!(range.contains(15, 20)); // End position
        
        // Outside range
        assert!(!range.contains(9, 10)); // Before start line
        assert!(!range.contains(16, 10)); // After end line
        assert!(!range.contains(10, 4)); // Before start column
        assert!(!range.contains(15, 21)); // After end column
    }

    #[test]
    fn test_symbol_kind_variants() {
        // Just ensure all variants exist and can be created
        let kinds = vec![
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Interface,
            SymbolKind::Class,
            SymbolKind::Module,
            SymbolKind::Variable,
            SymbolKind::Constant,
            SymbolKind::Field,
            SymbolKind::Parameter,
            SymbolKind::TypeAlias,
            SymbolKind::Macro,
        ];
        
        assert_eq!(kinds.len(), 14);
    }

    #[test]
    fn test_compact_string() {
        let s = compact_string("hello world");
        assert_eq!(&*s, "hello world");
    }

    #[test]
    fn test_id_equality_and_hash() {
        let id1 = SymbolId::new(42).unwrap();
        let id2 = SymbolId::new(42).unwrap();
        let id3 = SymbolId::new(43).unwrap();
        
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        
        // Test that they can be used in HashMaps
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(id1);
        assert!(set.contains(&id2));
        assert!(!set.contains(&id3));
    }
}