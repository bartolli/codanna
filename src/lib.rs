//! # Codanna
//!
//! Codannaは、コードベースの解析とインデックス化を行うためのライブラリです。
//! 複数のプログラミング言語に対応し、シンボル解析、セマンティック検索、
//! ベクトル埋め込みなどの機能を提供します。
//!
//! ## 主な機能
//!
//! - **マルチ言語パーサー**: Rust, Python, TypeScript, Kotlin, Go などをサポート
//! - **シンボル解析**: 関数、構造体、クラスなどのシンボルを抽出
//! - **セマンティック検索**: コードの意味的な検索機能
//! - **ベクトル埋め込み**: コードのベクトル表現を生成
//! - **プラグインシステム**: 拡張可能なアーキテクチャ
//!
//! ## 使用例
//!
//! ```no_run
//! use codanna::{Settings, SimpleIndexer};
//!
//! // 設定を読み込む
//! let settings = Settings::default();
//!
//! // インデクサーを作成
//! let indexer = SimpleIndexer::new(&settings);
//! ```

// Alias for tree-sitter-kotlin dependency
// When upstream publishes 0.3.9+, change Cargo.toml and update this line:
// extern crate tree_sitter_kotlin;
extern crate tree_sitter_kotlin_codanna as tree_sitter_kotlin;

/// デバッグ出力を行うマクロ
///
/// グローバルなデバッグフラグが有効な場合のみ、標準エラー出力にメッセージを出力します。
///
/// # 引数
///
/// * `$self` - デバッグ出力を行うコンテキスト（未使用だが互換性のため保持）
/// * `$($arg:tt)*` - フォーマット文字列と引数
///
/// # 使用例
///
/// ```
/// # #[macro_use] extern crate codanna;
/// # fn main() {
/// # let context = ();
/// debug_print!(context, "変数の値: {}", 42);
/// # }
/// ```
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
pub mod init;
pub mod io;
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
