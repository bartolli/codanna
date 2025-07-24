use crate::types::{SymbolId, FileId, Range, SymbolKind, CompactString, compact_string};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: CompactString,
    pub kind: SymbolKind,
    pub file_id: FileId,
    pub range: Range,
    pub signature: Option<Box<str>>,
}

#[repr(C, align(32))]
#[derive(Debug, Clone, Copy)]
pub struct CompactSymbol {
    pub name_offset: u32,
    pub kind: u8,
    pub flags: u8,
    pub file_id: u16,
    pub start_line: u32,
    pub start_col: u16,
    pub end_line: u32,
    pub end_col: u16,
    pub symbol_id: u32,
    _padding: [u8; 2],
}

impl Symbol {
    pub fn new(
        id: SymbolId,
        name: impl Into<CompactString>,
        kind: SymbolKind,
        file_id: FileId,
        range: Range,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            file_id,
            range,
            signature: None,
        }
    }

    pub fn with_signature(mut self, signature: impl Into<Box<str>>) -> Self {
        self.signature = Some(signature.into());
        self
    }

    pub fn to_compact(&self, string_table: &mut StringTable) -> CompactSymbol {
        let name_offset = string_table.intern(&self.name);
        
        CompactSymbol {
            name_offset,
            kind: self.kind as u8,
            flags: 0,
            file_id: self.file_id.value() as u16,
            start_line: self.range.start_line,
            start_col: self.range.start_column,
            end_line: self.range.end_line,
            end_col: self.range.end_column,
            symbol_id: self.id.value(),
            _padding: [0; 2],
        }
    }
}

pub struct StringTable {
    data: Vec<u8>,
    offsets: std::collections::HashMap<String, u32>,
}

impl StringTable {
    pub fn new() -> Self {
        Self {
            data: vec![0], // Start with null terminator
            offsets: std::collections::HashMap::new(),
        }
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&offset) = self.offsets.get(s) {
            return offset;
        }

        let offset = self.data.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0); // Null terminator
        self.offsets.insert(s.to_string(), offset);
        offset
    }

    pub fn get(&self, offset: u32) -> Option<&str> {
        let start = offset as usize;
        if start >= self.data.len() {
            return None;
        }

        let end = self.data[start..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| start + pos)?;

        std::str::from_utf8(&self.data[start..end]).ok()
    }
}

impl CompactSymbol {
    pub fn from_symbol(symbol: &Symbol, string_table: &StringTable) -> Option<Self> {
        let name_offset = string_table.offsets.get(symbol.name.as_ref())?;
        
        Some(CompactSymbol {
            name_offset: *name_offset,
            kind: symbol.kind as u8,
            flags: 0,
            file_id: symbol.file_id.value() as u16,
            start_line: symbol.range.start_line,
            start_col: symbol.range.start_column,
            end_line: symbol.range.end_line,
            end_col: symbol.range.end_column,
            symbol_id: symbol.id.value(),
            _padding: [0; 2],
        })
    }

    pub fn to_symbol(&self, string_table: &StringTable) -> Option<Symbol> {
        let name = string_table.get(self.name_offset)?;
        let kind = match self.kind {
            0 => SymbolKind::Function,
            1 => SymbolKind::Method,
            2 => SymbolKind::Struct,
            3 => SymbolKind::Enum,
            4 => SymbolKind::Trait,
            5 => SymbolKind::Interface,
            6 => SymbolKind::Class,
            7 => SymbolKind::Module,
            8 => SymbolKind::Variable,
            9 => SymbolKind::Constant,
            10 => SymbolKind::Field,
            11 => SymbolKind::Parameter,
            12 => SymbolKind::TypeAlias,
            13 => SymbolKind::Macro,
            _ => return None,
        };

        Some(Symbol {
            id: SymbolId::new(self.symbol_id)?,
            name: compact_string(name),
            kind,
            file_id: FileId::new(self.file_id as u32)?,
            range: Range::new(
                self.start_line,
                self.start_col,
                self.end_line,
                self.end_col,
            ),
            signature: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_symbol_creation() {
        let id = SymbolId::new(1).unwrap();
        let file_id = FileId::new(10).unwrap();
        let range = Range::new(5, 10, 5, 20);
        
        let symbol = Symbol::new(
            id,
            "test_function",
            SymbolKind::Function,
            file_id,
            range,
        );
        
        assert_eq!(symbol.id, id);
        assert_eq!(symbol.name.as_ref(), "test_function");
        assert_eq!(symbol.kind, SymbolKind::Function);
        assert_eq!(symbol.file_id, file_id);
        assert_eq!(symbol.range, range);
        assert!(symbol.signature.is_none());
    }

    #[test]
    fn test_symbol_with_signature() {
        let symbol = Symbol::new(
            SymbolId::new(1).unwrap(),
            "add",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(1, 0, 3, 1),
        ).with_signature("fn add(a: i32, b: i32) -> i32");
        
        assert_eq!(symbol.signature.as_deref(), Some("fn add(a: i32, b: i32) -> i32"));
    }

    #[test]
    fn test_compact_symbol_size() {
        assert_eq!(mem::size_of::<CompactSymbol>(), 32);
        assert_eq!(mem::align_of::<CompactSymbol>(), 32);
    }

    #[test]
    fn test_string_table() {
        let mut table = StringTable::new();
        
        let offset1 = table.intern("hello");
        let offset2 = table.intern("world");
        let offset3 = table.intern("hello"); // Should reuse
        
        assert_eq!(offset1, 1);
        assert_ne!(offset1, offset2);
        assert_eq!(offset1, offset3);
        
        assert_eq!(table.get(offset1), Some("hello"));
        assert_eq!(table.get(offset2), Some("world"));
        assert_eq!(table.get(999), None);
    }

    #[test]
    fn test_symbol_to_compact_and_back() {
        let mut string_table = StringTable::new();
        
        let original = Symbol::new(
            SymbolId::new(42).unwrap(),
            "test_method",
            SymbolKind::Method,
            FileId::new(7).unwrap(),
            Range::new(10, 5, 15, 20),
        );
        
        let compact = original.to_compact(&mut string_table);
        let restored = compact.to_symbol(&string_table).unwrap();
        
        assert_eq!(original.id, restored.id);
        assert_eq!(original.name, restored.name);
        assert_eq!(original.kind, restored.kind);
        assert_eq!(original.file_id, restored.file_id);
        assert_eq!(original.range, restored.range);
    }

    #[test]
    fn test_all_symbol_kinds_conversion() {
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
        
        let mut string_table = StringTable::new();
        
        for (i, kind) in kinds.iter().enumerate() {
            let symbol = Symbol::new(
                SymbolId::new((i + 1) as u32).unwrap(),
                format!("test_{}", i),
                *kind,
                FileId::new(1).unwrap(),
                Range::new(1, 0, 1, 10),
            );
            
            let compact = symbol.to_compact(&mut string_table);
            let restored = compact.to_symbol(&string_table).unwrap();
            
            assert_eq!(symbol.kind, restored.kind);
        }
    }
}