//! セマンティック検索モジュール
//!
//! ドキュメントコメントのセマンティック検索機能を提供します。
//! 既存のインデックスシステムと統合するシンプルなAPIを提供します。
//!
//! # 主要なコンポーネント
//!
//! - [`SimpleSemanticSearch`]: セマンティック検索の実装
//! - [`SemanticVectorStorage`]: ベクトルストレージ
//! - [`SemanticMetadata`]: セマンティック検索メタデータ
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::semantic::{SimpleSemanticSearch, EmbeddingModel};
//! use std::path::Path;
//!
//! // let search = SimpleSemanticSearch::new(Path::new(".codanna"), EmbeddingModel::AllMiniLML6V2);
//! // let results = search.search("find authentication functions", 10);
//! ```

mod metadata;
mod simple;
mod storage;

pub use metadata::SemanticMetadata;
pub use simple::{SemanticSearchError, SimpleSemanticSearch};
pub use storage::SemanticVectorStorage;

// Re-export key types
pub use fastembed::{EmbeddingModel, TextEmbedding};

/// Similarity threshold recommendations based on testing
pub mod thresholds {
    /// Threshold for very similar documents (e.g., same concept, different wording)
    pub const VERY_SIMILAR: f32 = 0.75;

    /// Threshold for similar documents (e.g., related concepts)
    pub const SIMILAR: f32 = 0.60;

    /// Threshold for somewhat related documents
    pub const RELATED: f32 = 0.40;

    /// Default threshold for semantic search
    pub const DEFAULT: f32 = SIMILAR;
}
