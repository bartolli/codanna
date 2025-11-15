//! クロスランゲージプロジェクト設定リゾルバ
//!
//! プロジェクトレベルの設定ファイル (tsconfig.json, pyproject.toml, go.mod など) を解決して、
//! モジュール解決ルール、インポートパス、プロジェクト固有の設定を決定します。
//!
//! これは `parsing::resolution` とは異なり、コード内のシンボル解決ではなく、
//! プロジェクト設定の解決を担当します。
//!
//! # 役割の違い
//!
//! - `project_resolver`: "このファイルにはどのtsconfig.jsonが適用されるか？"
//! - `parsing::resolution`: "このスコープで識別子 'foo' は何を参照するか？"
//!
//! # 使用例
//!
//! ```no_run
//! // use codanna::project_resolver::registry::SimpleProviderRegistry;
//! // let registry = SimpleProviderRegistry::new();
//! // let config = registry.resolve_config(file_path);
//! ```

pub mod memo;
pub mod persist;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod sha;

// Shared core types to be extended in later steps (TDD-driven)
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Sha256Hash(pub String);

impl Sha256Hash {
    /// Get the hash as a string reference
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create from byte array (for testing)
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Self(hex)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    /// Error reading/writing cache files on disk
    #[error("cache io error at '{path}': {source}")]
    CacheIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Cache format is invalid or incompatible
    #[error("invalid cache: {details}")]
    InvalidCache { details: String },
    /// I/O error with path context
    #[error("I/O error at '{path}': {cause}")]
    IoError { path: PathBuf, cause: String },
    /// Parse error for configuration files
    #[error("Parse error: {message}")]
    ParseError { message: String },
}

impl ResolutionError {
    pub fn cache_io(path: PathBuf, source: std::io::Error) -> Self {
        Self::CacheIo { path, source }
    }
    pub fn invalid_cache(details: impl Into<String>) -> Self {
        Self::InvalidCache {
            details: details.into(),
        }
    }
    pub fn suggestion(&self) -> &'static str {
        match self {
            ResolutionError::CacheIo { .. } => {
                "Check permissions and disk space; delete the cache file to rebuild."
            }
            ResolutionError::InvalidCache { .. } => {
                "Delete the on-disk cache to rebuild; ensure codanna version matches cache format."
            }
            ResolutionError::IoError { .. } => {
                "Check file permissions and ensure the path is accessible."
            }
            ResolutionError::ParseError { .. } => {
                "Check the configuration file syntax and ensure it's valid JSON/JSONC."
            }
        }
    }
    /// Stable code for programmatic handling in JSON responses
    pub fn status_code(&self) -> String {
        match self {
            ResolutionError::CacheIo { .. } => "RESOLUTION_CACHE_IO",
            ResolutionError::InvalidCache { .. } => "RESOLUTION_INVALID_CACHE",
            ResolutionError::IoError { .. } => "RESOLUTION_IO_ERROR",
            ResolutionError::ParseError { .. } => "RESOLUTION_PARSE_ERROR",
        }
        .to_string()
    }
    /// Recovery suggestions list (mirrors project error conventions)
    pub fn recovery_suggestions(&self) -> Vec<&'static str> {
        match self {
            ResolutionError::CacheIo { .. } => vec![
                "Ensure the cache directory exists and is writable",
                "Check disk space and permissions",
                "Delete the on-disk cache to force a rebuild",
            ],
            ResolutionError::InvalidCache { .. } => vec![
                "Delete the on-disk cache to force a rebuild",
                "Verify codanna version compatibility with cache format",
            ],
            ResolutionError::IoError { .. } => vec![
                "Check file permissions",
                "Ensure the file exists at the specified path",
                "Verify parent directory exists",
            ],
            ResolutionError::ParseError { .. } => vec![
                "Check JSON syntax for errors",
                "Remove trailing commas if present",
                "Ensure proper quote escaping",
            ],
        }
    }
}

pub type ResolutionResult<T> = Result<T, ResolutionError>;
