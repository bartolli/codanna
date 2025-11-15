//! 基本型定義モジュール
//!
//! このモジュールは、Codannaシステム全体で使用される基本的な型を定義します。
//! シンボルID、ファイルID、範囲、シンボル種別などが含まれます。
//!
//! # 使用例
//!
//! ```
//! use codanna::types::{SymbolId, FileId, Range, SymbolKind};
//!
//! // シンボルIDの作成
//! let symbol_id = SymbolId::new(42).expect("有効なID");
//!
//! // ファイルIDの作成
//! let file_id = FileId::new(1).expect("有効なID");
//!
//! // 範囲の作成
//! let range = Range::new(10, 5, 15, 20);
//! assert!(range.contains(12, 10));
//! ```

mod symbol_counter;

pub use symbol_counter::SymbolCounter;

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// シンボルの一意識別子
///
/// 各シンボル（関数、構造体、変数など）に割り当てられる一意のIDです。
/// 0は無効な値として扱われます。
///
/// # 使用例
///
/// ```
/// use codanna::types::SymbolId;
///
/// let id = SymbolId::new(42).expect("有効なID");
/// assert_eq!(id.value(), 42);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub u32);

/// ファイルの一意識別子
///
/// インデックス化された各ファイルに割り当てられる一意のIDです。
/// 0は無効な値として扱われます。
///
/// # 使用例
///
/// ```
/// use codanna::types::FileId;
///
/// let id = FileId::new(1).expect("有効なID");
/// assert_eq!(id.value(), 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

/// インデックス操作の結果
///
/// ファイルが新規にインデックス化されたか、
/// キャッシュから読み込まれたかを示します。
///
/// # 使用例
///
/// ```
/// use codanna::types::{IndexingResult, FileId};
///
/// let result = IndexingResult::Indexed(FileId::new(1).unwrap());
/// assert!(!result.is_cached());
/// assert_eq!(result.file_id(), FileId::new(1).unwrap());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexingResult {
    /// ファイルが新規にインデックス化された
    Indexed(FileId),
    /// ファイルがキャッシュから読み込まれた（変更なし）
    Cached(FileId),
}

impl IndexingResult {
    /// ファイルIDを取得します
    ///
    /// # 戻り値
    ///
    /// インデックス化されたファイルのID
    ///
    /// # 使用例
    ///
    /// ```
    /// use codanna::types::{IndexingResult, FileId};
    ///
    /// let result = IndexingResult::Indexed(FileId::new(1).unwrap());
    /// assert_eq!(result.file_id(), FileId::new(1).unwrap());
    /// ```
    pub fn file_id(&self) -> FileId {
        match self {
            IndexingResult::Indexed(id) => *id,
            IndexingResult::Cached(id) => *id,
        }
    }

    /// キャッシュから読み込まれたかどうかを判定します
    ///
    /// # 戻り値
    ///
    /// キャッシュから読み込まれた場合は`true`、新規にインデックス化された場合は`false`
    pub fn is_cached(&self) -> bool {
        matches!(self, IndexingResult::Cached(_))
    }
}

/// ソースコード内の範囲
///
/// 行と列の位置で範囲を表現します。
///
/// # 使用例
///
/// ```
/// use codanna::types::Range;
///
/// let range = Range::new(10, 5, 15, 20);
/// assert!(range.contains(12, 10));
/// assert!(!range.contains(5, 0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// 開始行
    pub start_line: u32,
    /// 開始列
    pub start_column: u16,
    /// 終了行
    pub end_line: u32,
    /// 終了列
    pub end_column: u16,
}

/// シンボルの種類
///
/// 関数、構造体、クラスなど、様々な種類のシンボルを表現します。
///
/// # 使用例
///
/// ```
/// use codanna::types::SymbolKind;
///
/// let kind = SymbolKind::Function;
/// assert_eq!(kind, SymbolKind::Function);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    /// 関数
    Function,
    /// メソッド
    Method,
    /// 構造体
    Struct,
    /// 列挙型
    Enum,
    /// トレイト
    Trait,
    /// インターフェース
    Interface,
    /// クラス
    Class,
    /// モジュール
    Module,
    /// 変数
    Variable,
    /// 定数
    Constant,
    /// フィールド
    Field,
    /// パラメータ
    Parameter,
    /// 型エイリアス
    TypeAlias,
    /// マクロ
    Macro,
}

impl SymbolId {
    pub fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
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
        if value == 0 { None } else { Some(Self(value)) }
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
