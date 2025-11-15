//! ベクトル検索モジュール
//!
//! コードインテリジェンスのための高性能なベクトルストレージと検索機能を提供します。
//! 既存のTantivyベースのテキスト検索インフラと統合するように設計されています。
//!
//! # パフォーマンス目標
//!
//! - ベクトルアクセス: <1μs per vector
//! - メモリ使用量: ~100 bytes per symbol
//! - インデックス作成: 10,000+ files/second
//! - 検索レイテンシ: <10ms for semantic search
//!
//! # アーキテクチャ
//!
//! ベクトル検索システムは、IVFFlat（Inverted File with Flat vectors）インデックスと
//! K-meansクラスタリングを使用して、準線形の検索パフォーマンスを実現します。
//! ベクトルはメモリマップドファイルに保存され、即座のロードと最小限のメモリオーバーヘッドを実現します。
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::vector::{VectorEngine, VectorStorage};
//! // let engine = VectorEngine::new();
//! // let results = engine.search(query_vector, top_k);
//! ```

mod clustering;
mod embedding;
mod engine;
mod storage;
mod types;

// Re-export core types for public API
pub use clustering::{
    ClusteringError, KMeansResult, assign_to_nearest_centroid, cosine_similarity, kmeans_clustering,
};
#[cfg(test)]
pub use embedding::MockEmbeddingGenerator;
pub use embedding::{
    EmbeddingGenerator, FastEmbedGenerator, create_symbol_text, model_to_string,
    parse_embedding_model,
};
pub use engine::VectorSearchEngine;
pub use storage::{ConcurrentVectorStorage, MmapVectorStorage, VectorStorageError};
pub use types::{
    ClusterId, Score, SegmentOrdinal, VECTOR_DIMENSION_384, VectorDimension, VectorError, VectorId,
};
