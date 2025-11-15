//! ストレージモジュール
//!
//! インデックスデータの永続化、検索エンジン (Tantivy)、メタデータ管理などを提供します。
//!
//! # 主要なコンポーネント
//!
//! - [`IndexPersistence`]: インデックスの永続化
//! - [`DocumentIndex`]: Tantivy ベースの全文検索
//! - [`IndexMetadata`]: インデックスメタデータ管理
//! - [`MetadataKey`]: メタデータキーの定義
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::storage::{IndexPersistence, IndexMetadata};
//! use std::path::Path;
//!
//! let path = Path::new(".codanna/index");
//! let persistence = IndexPersistence::new(path);
//! ```

pub mod error;
pub mod memory;
pub mod metadata;
pub mod metadata_keys;
pub mod persistence;
pub mod symbol_cache;
pub mod tantivy;
pub use error::{StorageError, StorageResult};
pub use metadata::{DataSource, IndexMetadata};
pub use metadata_keys::MetadataKey;
pub use persistence::IndexPersistence;
pub use tantivy::{DocumentIndex, SearchResult};
