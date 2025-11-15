//! シンボル定義モジュール
//!
//! コード内のシンボル（関数、構造体、変数など）を表現する型を提供します。
//!
//! # 主要な型
//!
//! - [`Symbol`]: シンボルの完全な情報
//! - [`CompactSymbol`]: メモリ効率の良いシンボル表現
//! - [`Visibility`]: シンボルの可視性
//! - [`ScopeContext`]: スコープ情報
//!
//! # 使用例
//!
//! ```
//! use codanna::symbol::Visibility;
//!
//! let vis = Visibility::Public;
//! assert_eq!(vis, Visibility::Public);
//! ```

pub mod context;

use crate::parsing::registry::LanguageId;
use crate::types::{CompactString, FileId, Range, SymbolId, SymbolKind, compact_string};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Visibility of a symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// Public visibility (pub)
    Public,
    /// Crate-level visibility (pub(crate))
    Crate,
    /// Module-level visibility (pub(super), pub(in path))
    Module,
    /// Private visibility (default)
    Private,
}

/// Scope context for symbol definition
///
/// This enum represents where a symbol is defined in the code structure,
/// enabling proper resolution without heuristics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScopeContext {
    /// Local to function/method/block
    Local {
        /// For JS/TS: is this hoisted to function scope?
        hoisted: bool,
        /// Name of the parent function/class this is local to
        parent_name: Option<CompactString>,
        /// Kind of the parent (Function, Class, etc.)
        parent_kind: Option<SymbolKind>,
    },
    /// Parameter of function/method
    Parameter,
    /// Class/struct/trait member
    ClassMember,
    /// Module/file level definition
    #[default]
    Module,
    /// Package/namespace level export
    Package,
    /// Global/builtin symbol
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: CompactString,
    pub kind: SymbolKind,
    pub file_id: FileId,
    pub range: Range,
    /// Clean file path without line numbers (e.g., "src/lib.rs")
    pub file_path: Box<str>,
    pub signature: Option<Box<str>>,
    /// Documentation comment extracted from source (e.g., /// or /** */ in Rust)
    pub doc_comment: Option<Box<str>>,
    /// Full module path (e.g., "crate::storage::memory" or "std::collections")
    pub module_path: Option<Box<str>>,
    /// Visibility of the symbol
    pub visibility: Visibility,
    /// Scope context where this symbol is defined
    ///
    /// This field enables proper resolution without heuristics.
    /// It's Optional during migration - will become required in future.
    pub scope_context: Option<ScopeContext>,
    /// Language identifier for the symbol
    ///
    /// This field enables language-specific filtering in searches.
    /// It's Optional for backward compatibility - existing indexes will have None.
    pub language_id: Option<LanguageId>,
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
            file_path: "<unknown>".into(),
            signature: None,
            doc_comment: None,
            module_path: None,
            visibility: Visibility::Private,
            scope_context: None, // Default to None for backward compatibility
            language_id: None,   // Default to None for backward compatibility
        }
    }

    /// Create a new symbol with scope context
    pub fn new_with_scope(
        id: SymbolId,
        name: impl Into<CompactString>,
        kind: SymbolKind,
        file_id: FileId,
        range: Range,
        scope: ScopeContext,
    ) -> Self {
        let mut symbol = Self::new(id, name, kind, file_id, range);
        symbol.scope_context = Some(scope);
        symbol
    }

    pub fn with_file_path(mut self, file_path: impl Into<Box<str>>) -> Self {
        self.file_path = file_path.into();
        self
    }

    pub fn with_signature(mut self, signature: impl Into<Box<str>>) -> Self {
        self.signature = Some(signature.into());
        self
    }

    pub fn with_doc(mut self, doc: impl Into<Box<str>>) -> Self {
        self.doc_comment = Some(doc.into());
        self
    }

    pub fn with_module_path(mut self, path: impl Into<Box<str>>) -> Self {
        self.module_path = Some(path.into());
        self
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_scope(mut self, scope: ScopeContext) -> Self {
        self.scope_context = Some(scope);
        self
    }

    pub fn with_language_id(mut self, language_id: LanguageId) -> Self {
        self.language_id = Some(language_id);
        self
    }

    /// Get the symbol name as a string slice
    pub fn as_name(&self) -> &str {
        &self.name
    }

    /// Convert the symbol into its name, consuming the symbol
    pub fn into_name(self) -> CompactString {
        self.name
    }

    /// Get a reference to the signature if present
    pub fn as_signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }

    /// Get a reference to the doc comment if present
    pub fn as_doc_comment(&self) -> Option<&str> {
        self.doc_comment.as_deref()
    }

    /// Get a reference to the module path if present
    pub fn as_module_path(&self) -> Option<&str> {
        self.module_path.as_deref()
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

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;

        if let Some(sig) = &self.signature {
            write!(f, "\n  Signature: {sig}")?;
        }

        write!(f, "\n  Kind: {:?}", self.kind)?;
        write!(f, "\n  Visibility: {:?}", self.visibility)?;
        write!(
            f,
            "\n  Location: file#{} {}:{}-{}:{}",
            self.file_id.value(),
            self.range.start_line,
            self.range.start_column,
            self.range.end_line,
            self.range.end_column
        )?;

        if let Some(module) = &self.module_path {
            write!(f, "\n  Module: {module}")?;
        }

        if let Some(doc) = &self.doc_comment {
            let truncated = if doc.len() > 100 {
                format!("{}...", &doc[..100])
            } else {
                doc.to_string()
            };
            write!(f, "\n  Doc: {truncated}")?;
        }

        Ok(())
    }
}

pub struct StringTable {
    data: Vec<u8>,
    offsets: std::collections::HashMap<String, u32>,
}

impl Default for StringTable {
    fn default() -> Self {
        Self {
            data: vec![0], // Start with null terminator
            offsets: std::collections::HashMap::new(),
        }
    }
}

impl StringTable {
    pub fn new() -> Self {
        Self::default()
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
            range: Range::new(self.start_line, self.start_col, self.end_line, self.end_col),
            file_path: "<unknown>".into(),
            signature: None,
            doc_comment: None,
            module_path: None,
            visibility: Visibility::Private,
            scope_context: None, // CompactSymbol doesn't store scope info yet
            language_id: None,   // CompactSymbol doesn't store language info yet
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

        let symbol = Symbol::new(id, "test_function", SymbolKind::Function, file_id, range);

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
        )
        .with_signature("fn add(a: i32, b: i32) -> i32");

        assert_eq!(
            symbol.signature.as_deref(),
            Some("fn add(a: i32, b: i32) -> i32")
        );
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
        let kinds = [
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
                format!("test_{i}"),
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
