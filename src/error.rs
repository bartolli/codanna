//! Error types for the codebase intelligence system
//!
//! This module provides structured error types using thiserror for better
//! error handling and actionable error messages.

use std::path::PathBuf;
use thiserror::Error;
use crate::{SymbolId, FileId};

/// Main error type for indexing operations
#[derive(Error, Debug)]
pub enum IndexError {
    /// File system errors
    #[error("Failed to read file '{path}': {source}")]
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },
    
    #[error("Failed to write file '{path}': {source}")]
    FileWrite {
        path: PathBuf,
        source: std::io::Error,
    },
    
    /// Parsing errors
    #[error("Failed to parse {language} file '{path}': {reason}")]
    ParseError {
        path: PathBuf,
        language: String,
        reason: String,
    },
    
    #[error("Unsupported file type '{extension}' for file '{path}'. Supported types: .rs, .go, .py, .js, .ts, .java")]
    UnsupportedFileType {
        path: PathBuf,
        extension: String,
    },
    
    /// Storage errors
    #[error("Failed to persist index to '{path}': {source}")]
    PersistenceError {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    
    #[error("Failed to load index from '{path}': {source}")]
    LoadError {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    
    /// Symbol resolution errors
    #[error("Symbol '{name}' not found. Did you mean to index the file first?")]
    SymbolNotFound {
        name: String,
    },
    
    #[error("File ID {id:?} not found in index. The file may have been removed or not indexed.")]
    FileNotFound {
        id: FileId,
    },
    
    /// Index state errors
    #[error("Failed to create file ID: maximum file count reached")]
    FileIdExhausted,
    
    #[error("Failed to create symbol ID: maximum symbol count reached")]
    SymbolIdExhausted,
    
    /// Configuration errors
    #[error("Invalid configuration: {reason}")]
    ConfigError {
        reason: String,
    },
    
    /// Tantivy-specific errors
    #[error("Tantivy operation failed during {operation}: {cause}")]
    TantivyError {
        operation: String,
        cause: String,
    },
    
    /// Transaction errors
    #[error("Transaction failed after operations: {operations:?}. Cause: {cause}")]
    TransactionFailed {
        operations: Vec<String>,
        cause: String,
    },
    
    /// Mutex poisoned error
    #[error("Internal mutex was poisoned, likely due to panic in another thread")]
    MutexPoisoned,
    
    /// Corrupted index error
    #[error("Index appears to be corrupted: {reason}")]
    IndexCorrupted {
        reason: String,
    },
    
    /// General errors for cases where we need to preserve existing behavior
    #[error("{0}")]
    General(String),
}

impl IndexError {
    /// Get recovery suggestions for this error
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

/// Errors specific to parsing operations
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Failed to initialize {language} parser: {reason}")]
    ParserInit {
        language: String,
        reason: String,
    },
    
    #[error("Failed to parse code at line {line}, column {column}: {reason}")]
    SyntaxError {
        line: u32,
        column: u32,
        reason: String,
    },
    
    #[error("Invalid UTF-8 in source file")]
    InvalidUtf8,
}

/// Errors specific to storage operations
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Tantivy index error: {0}")]
    TantivyError(#[from] tantivy::TantivyError),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] bincode::Error),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Document not found for symbol {id:?}")]
    DocumentNotFound {
        id: SymbolId,
    },
}

/// Errors specific to MCP operations
#[derive(Error, Debug)]
pub enum McpError {
    #[error("Failed to initialize MCP server: {reason}")]
    ServerInitError {
        reason: String,
    },
    
    #[error("MCP client error: {reason}")]
    ClientError {
        reason: String,
    },
    
    #[error("Invalid tool arguments: {reason}")]
    InvalidArguments {
        reason: String,
    },
}

/// Result type alias for index operations
pub type IndexResult<T> = Result<T, IndexError>;

/// Result type alias for parse operations
pub type ParseResult<T> = Result<T, ParseError>;

/// Result type alias for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

/// Result type alias for MCP operations
pub type McpResult<T> = Result<T, McpError>;

/// Helper trait for adding context to errors
pub trait ErrorContext<T> {
    /// Add context to an error
    fn context(self, msg: &str) -> Result<T, IndexError>;
    
    /// Add context with a path
    fn with_path(self, path: &std::path::Path) -> Result<T, IndexError>;
}

impl<T, E> ErrorContext<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, msg: &str) -> Result<T, IndexError> {
        self.map_err(|e| IndexError::General(format!("{}: {}", msg, e)))
    }
    
    fn with_path(self, path: &std::path::Path) -> Result<T, IndexError> {
        self.map_err(|e| IndexError::General(format!("Error processing '{}': {}", path.display(), e)))
    }
}