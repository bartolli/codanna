//! エラー型定義モジュール
//!
//! このモジュールは、コードベースインテリジェンスシステムで使用される
//! 構造化されたエラー型を提供します。`thiserror`クレートを使用して、
//! より良いエラーハンドリングと実用的なエラーメッセージを実現しています。
//!
//! # 主なエラー型
//!
//! - [`IndexError`]: インデックス操作に関するエラー
//! - [`ParseError`]: パース操作に関するエラー
//! - [`StorageError`]: ストレージ操作に関するエラー
//! - [`McpError`]: MCP操作に関するエラー
//!
//! # 使用例
//!
//! ```
//! use codanna::error::{IndexError, IndexResult};
//! use std::path::PathBuf;
//!
//! fn example_operation() -> IndexResult<()> {
//!     // エラーを返す例
//!     Err(IndexError::ConfigError {
//!         reason: "設定ファイルが見つかりません".to_string()
//!     })
//! }
//! ```

use crate::{FileId, SymbolId};
use std::path::PathBuf;
use thiserror::Error;

/// インデックス操作のメインエラー型
///
/// インデックス作成、読み込み、永続化などの操作で発生する
/// 各種エラーを表現します。
///
/// # 使用例
///
/// ```
/// use codanna::error::IndexError;
/// use std::path::PathBuf;
///
/// let error = IndexError::ConfigError {
///     reason: "無効な設定".to_string()
/// };
///
/// // エラーコードを取得
/// assert_eq!(error.status_code(), "CONFIG_ERROR");
///
/// // リカバリの提案を取得
/// let suggestions = error.recovery_suggestions();
/// ```
#[derive(Error, Debug)]
pub enum IndexError {
    /// ファイルシステムエラー - ファイル読み込み失敗
    #[error("Failed to read file '{path}': {source}")]
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },

    /// ファイルシステムエラー - ファイル書き込み失敗
    #[error("Failed to write file '{path}': {source}")]
    FileWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    /// パースエラー - 言語固有のパース失敗
    #[error("Failed to parse {language} file '{path}': {reason}")]
    ParseError {
        path: PathBuf,
        language: String,
        reason: String,
    },

    /// サポートされていないファイルタイプ
    #[error(
        "Unsupported file type '{extension}' for file '{path}'. Supported types: .rs, .go, .py, .js, .ts, .java"
    )]
    UnsupportedFileType { path: PathBuf, extension: String },

    /// ストレージエラー - インデックスの永続化失敗
    #[error("Failed to persist index to '{path}': {source}")]
    PersistenceError {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// ストレージエラー - インデックスの読み込み失敗
    #[error("Failed to load index from '{path}': {source}")]
    LoadError {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// シンボル解決エラー - シンボルが見つからない
    #[error("Symbol '{name}' not found. Did you mean to index the file first?")]
    SymbolNotFound { name: String },

    /// シンボル解決エラー - ファイルIDが見つからない
    #[error("File ID {id:?} not found in index. The file may have been removed or not indexed.")]
    FileNotFound { id: FileId },

    /// インデックス状態エラー - ファイルIDの上限到達
    #[error("Failed to create file ID: maximum file count reached")]
    FileIdExhausted,

    /// インデックス状態エラー - シンボルIDの上限到達
    #[error("Failed to create symbol ID: maximum symbol count reached")]
    SymbolIdExhausted,

    /// 設定エラー - 無効な設定
    #[error("Invalid configuration: {reason}")]
    ConfigError { reason: String },

    /// Tantivy固有のエラー
    #[error("Tantivy operation failed during {operation}: {cause}")]
    TantivyError { operation: String, cause: String },

    /// トランザクションエラー - トランザクション失敗
    #[error("Transaction failed after operations: {operations:?}. Cause: {cause}")]
    TransactionFailed {
        operations: Vec<String>,
        cause: String,
    },

    /// Mutexポイズンエラー - 別スレッドでのパニックによる
    #[error("Internal mutex was poisoned, likely due to panic in another thread")]
    MutexPoisoned,

    /// 破損したインデックスエラー
    #[error("Index appears to be corrupted: {reason}")]
    IndexCorrupted { reason: String },

    /// 一般的なエラー - 既存の動作を保持するため
    #[error("{0}")]
    General(String),
}

impl IndexError {
    /// このエラー型の安定したステータスコードを取得します
    ///
    /// プログラマティックなエラーハンドリングのためにJSON レスポンスで
    /// 使用できる文字列識別子を返します。
    ///
    /// # 戻り値
    ///
    /// エラーの種類を表す文字列（例: "CONFIG_ERROR", "FILE_NOT_FOUND"）
    ///
    /// # 使用例
    ///
    /// ```
    /// use codanna::error::IndexError;
    ///
    /// let error = IndexError::ConfigError {
    ///     reason: "無効な設定".to_string()
    /// };
    /// assert_eq!(error.status_code(), "CONFIG_ERROR");
    /// ```
    pub fn status_code(&self) -> String {
        match self {
            Self::FileRead { .. } => "FILE_READ_ERROR",
            Self::FileWrite { .. } => "FILE_WRITE_ERROR",
            Self::ParseError { .. } => "PARSE_ERROR",
            Self::UnsupportedFileType { .. } => "UNSUPPORTED_FILE_TYPE",
            Self::PersistenceError { .. } => "PERSISTENCE_ERROR",
            Self::LoadError { .. } => "LOAD_ERROR",
            Self::SymbolNotFound { .. } => "SYMBOL_NOT_FOUND",
            Self::FileNotFound { .. } => "FILE_NOT_FOUND",
            Self::FileIdExhausted => "FILE_ID_EXHAUSTED",
            Self::SymbolIdExhausted => "SYMBOL_ID_EXHAUSTED",
            Self::ConfigError { .. } => "CONFIG_ERROR",
            Self::TantivyError { .. } => "TANTIVY_ERROR",
            Self::TransactionFailed { .. } => "TRANSACTION_FAILED",
            Self::MutexPoisoned => "MUTEX_POISONED",
            Self::IndexCorrupted { .. } => "INDEX_CORRUPTED",
            Self::General(_) => "GENERAL_ERROR",
        }
        .to_string()
    }

    /// このエラーのリカバリ提案を取得します
    ///
    /// エラーから回復するための具体的な提案のリストを返します。
    ///
    /// # 戻り値
    ///
    /// 回復方法を説明する文字列のベクタ
    ///
    /// # 使用例
    ///
    /// ```
    /// use codanna::error::IndexError;
    ///
    /// let error = IndexError::IndexCorrupted {
    ///     reason: "破損したデータ".to_string()
    /// };
    /// let suggestions = error.recovery_suggestions();
    /// assert!(!suggestions.is_empty());
    /// ```
    pub fn recovery_suggestions(&self) -> Vec<&'static str> {
        match self {
            Self::TantivyError { .. } => vec![
                "Try running 'codanna index --force' to rebuild the index",
                "Check disk space and permissions in the index directory",
            ],
            Self::TransactionFailed { .. } => vec![
                "The operation was rolled back, your index is in a consistent state",
                "Try the operation again, it may succeed on retry",
            ],
            Self::MutexPoisoned => vec![
                "Restart the application to clear the poisoned state",
                "If the problem persists, run 'codanna index --force'",
            ],
            Self::IndexCorrupted { .. } => vec![
                "Run 'codanna index --force' to rebuild from scratch",
                "Check for disk errors or filesystem corruption",
            ],
            Self::LoadError { .. } | Self::PersistenceError { .. } => vec![
                "The index will be loaded from Tantivy on next start",
                "Run 'codanna index --force' if you continue to have issues",
            ],
            Self::FileRead { .. } => vec![
                "Check that the file exists and you have read permissions",
                "Ensure the file is not locked by another process",
            ],
            Self::UnsupportedFileType { .. } => vec![
                "Currently only Rust files (.rs) are supported",
                "Support for other languages is coming soon",
            ],
            _ => vec![],
        }
    }
}

/// パース操作固有のエラー
///
/// ソースコードのパース中に発生するエラーを表現します。
///
/// # 使用例
///
/// ```
/// use codanna::error::ParseError;
///
/// let error = ParseError::ParserInit {
///     language: "Rust".to_string(),
///     reason: "パーサーの初期化に失敗".to_string(),
/// };
/// ```
#[derive(Error, Debug)]
pub enum ParseError {
    /// パーサー初期化エラー
    #[error("Failed to initialize {language} parser: {reason}")]
    ParserInit { language: String, reason: String },

    /// 構文エラー - 特定の行と列で発生
    #[error("Failed to parse code at line {line}, column {column}: {reason}")]
    SyntaxError {
        line: u32,
        column: u32,
        reason: String,
    },

    /// 無効なUTF-8エンコーディング
    #[error("Invalid UTF-8 in source file")]
    InvalidUtf8,
}

/// ストレージ操作固有のエラー
///
/// インデックスの永続化や検索エンジン操作で発生するエラーを表現します。
///
/// # 使用例
///
/// ```
/// use codanna::error::StorageError;
///
/// let error = StorageError::DatabaseError(
///     "接続エラー".to_string()
/// );
/// ```
#[derive(Error, Debug)]
pub enum StorageError {
    /// Tantivyインデックスエラー
    #[error("Tantivy index error: {0}")]
    TantivyError(#[from] tantivy::TantivyError),

    /// データベースエラー
    // Removed bincode error variant - no longer needed with Tantivy-only architecture
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// ドキュメント未検出エラー
    #[error("Document not found for symbol {id:?}")]
    DocumentNotFound { id: SymbolId },
}

/// MCP操作固有のエラー
///
/// Model Context Protocol (MCP) サーバーやクライアント操作で発生するエラーを表現します。
///
/// # 使用例
///
/// ```
/// use codanna::error::McpError;
///
/// let error = McpError::ServerInitError {
///     reason: "ポートが使用中です".to_string()
/// };
/// ```
#[derive(Error, Debug)]
pub enum McpError {
    /// MCPサーバー初期化エラー
    #[error("Failed to initialize MCP server: {reason}")]
    ServerInitError { reason: String },

    /// MCPクライアントエラー
    #[error("MCP client error: {reason}")]
    ClientError { reason: String },

    /// 無効なツール引数エラー
    #[error("Invalid tool arguments: {reason}")]
    InvalidArguments { reason: String },
}

/// インデックス操作用の Result 型エイリアス
///
/// # 使用例
///
/// ```
/// use codanna::error::{IndexResult, IndexError};
///
/// fn example_function() -> IndexResult<String> {
///     Ok("成功".to_string())
/// }
/// ```
pub type IndexResult<T> = Result<T, IndexError>;

/// パース操作用の Result 型エイリアス
pub type ParseResult<T> = Result<T, ParseError>;

/// ストレージ操作用の Result 型エイリアス
pub type StorageResult<T> = Result<T, StorageError>;

/// MCP操作用の Result 型エイリアス
pub type McpResult<T> = Result<T, McpError>;

/// エラーにコンテキストを追加するためのヘルパートレイト
///
/// エラーメッセージにコンテキスト情報を追加して、より詳細なエラー情報を提供します。
///
/// # 使用例
///
/// ```
/// use codanna::error::{ErrorContext, IndexResult};
/// use std::fs;
///
/// fn read_config() -> IndexResult<String> {
///     fs::read_to_string("/path/to/config.toml")
///         .context("設定ファイルの読み込みに失敗しました")
/// }
/// ```
pub trait ErrorContext<T> {
    /// エラーにコンテキストメッセージを追加します
    ///
    /// # 引数
    ///
    /// * `msg` - 追加するコンテキストメッセージ
    fn context(self, msg: &str) -> Result<T, IndexError>;

    /// エラーにパス情報を追加します
    ///
    /// # 引数
    ///
    /// * `path` - エラーに関連するファイルパス
    fn with_path(self, path: &std::path::Path) -> Result<T, IndexError>;
}

impl<T, E> ErrorContext<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, msg: &str) -> Result<T, IndexError> {
        self.map_err(|e| IndexError::General(format!("{msg}: {e}")))
    }

    fn with_path(self, path: &std::path::Path) -> Result<T, IndexError> {
        self.map_err(|e| {
            IndexError::General(format!("Error processing '{}': {}", path.display(), e))
        })
    }
}
