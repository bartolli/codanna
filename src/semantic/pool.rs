//! Embedding backend abstraction — local fastembed pool or remote HTTP endpoint.
//!
//! `EmbeddingBackend` is the single type the rest of the codebase interacts with.
//! It dispatches to either:
//!   - `EmbeddingPool`   — local fastembed (parallel, thread-pool based)
//!   - `RemoteEmbedder`  — OpenAI-compatible HTTP server (async, batched)

use crate::SymbolId;
use crossbeam_channel::{Receiver, Sender, bounded};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::SemanticSearchError;
use super::remote::{RemoteEmbedder, run_async};

// ── EmbeddingBackend ───────────────────────────────────────────────────────

/// Unified embedding backend — wraps either a local fastembed pool or a remote
/// HTTP embedder. All callers use this type; the backend is chosen at startup
/// based on configuration.
pub enum EmbeddingBackend {
    Local(EmbeddingPool),
    Remote(Arc<RemoteEmbedder>),
}

impl EmbeddingBackend {
    /// Output dimension of this backend's embeddings.
    pub fn dimensions(&self) -> usize {
        match self {
            EmbeddingBackend::Local(pool) => pool.dimensions(),
            EmbeddingBackend::Remote(r) => r.dim(),
        }
    }

    /// Log usage statistics (no-op for remote backend).
    pub fn log_usage_stats(&self) {
        if let EmbeddingBackend::Local(pool) = self {
            pool.log_usage_stats();
        }
    }

    /// Model name / URL for metadata and logging.
    pub fn model_name(&self) -> &str {
        match self {
            EmbeddingBackend::Local(pool) => pool.model_name(),
            EmbeddingBackend::Remote(_) => "remote",
        }
    }

    /// Embed a single text synchronously.
    /// Remote backend blocks the calling thread via `tokio::task::block_in_place`.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>, SemanticSearchError> {
        match self {
            EmbeddingBackend::Local(pool) => pool.embed_one(text),
            EmbeddingBackend::Remote(r) => {
                let r = Arc::clone(r);
                let text = text.to_string();
                run_async(async move {
                    let results = r.embed(&[text]).await?;
                    results.into_iter().next().ok_or_else(|| {
                        SemanticSearchError::EmbeddingError("Remote embed returned empty".into())
                    })
                })
            }
        }
    }

    /// Embed multiple items in parallel (local) or batched async (remote).
    pub fn embed_parallel(
        &self,
        items: &[(SymbolId, &str, &str)],
    ) -> Vec<(SymbolId, Vec<f32>, String)> {
        match self {
            EmbeddingBackend::Local(pool) => pool.embed_parallel(items),
            EmbeddingBackend::Remote(r) => {
                let r = Arc::clone(r);
                let texts: Vec<String> = items.iter().map(|(_, t, _)| t.to_string()).collect();
                let dim = r.dim();

                let embeddings = run_async(async move { r.embed(&texts).await });

                match embeddings {
                    Ok(embs) => embs
                        .into_iter()
                        .zip(items.iter())
                        .filter_map(|(emb, (id, _, lang))| {
                            if emb.len() == dim {
                                Some((*id, emb, (*lang).to_string()))
                            } else {
                                tracing::warn!(
                                    target: "semantic",
                                    "Remote dim mismatch for {}: expected {dim}, got {}",
                                    id.to_u32(), emb.len()
                                );
                                None
                            }
                        })
                        .collect(),
                    Err(e) => {
                        tracing::error!(target: "semantic", "Remote embed_parallel failed: {e}");
                        Vec::new()
                    }
                }
            }
        }
    }
}

// ── EmbeddingPool (local fastembed) ───────────────────────────────────────

/// Model instance with an ID for tracking
struct ModelInstance {
    model: TextEmbedding,
    id: usize,
}

/// Pool of TextEmbedding models for parallel embedding generation.
///
/// Each model instance is expensive (~86MB), but having multiple allows
/// true parallel embedding generation with rayon.
pub struct EmbeddingPool {
    model_sender: Sender<ModelInstance>,
    model_receiver: Receiver<ModelInstance>,
    pool_size: usize,
    dimensions: usize,
    model_name: String,
    usage_counters: Vec<AtomicUsize>,
}

impl EmbeddingPool {
    /// Create a new embedding pool with the specified number of model instances.
    ///
    /// Each model instance uses ~86MB of memory for AllMiniLML6V2.
    pub fn new(pool_size: usize, model: EmbeddingModel) -> Result<Self, SemanticSearchError> {
        let pool_size = pool_size.max(1);
        let (sender, receiver) = bounded(pool_size);

        let cache_dir = crate::init::models_dir();
        let model_name = crate::vector::model_to_string(&model);

        tracing::info!(
            target: "semantic",
            "Initializing embedding pool: {pool_size} instances ({model_name})"
        );

        let mut dimensions = 0;
        let usage_counters: Vec<AtomicUsize> =
            (0..pool_size).map(|_| AtomicUsize::new(0)).collect();

        for i in 0..pool_size {
            let mut text_model = TextEmbedding::try_new(
                InitOptions::new(model.clone())
                    .with_cache_dir(cache_dir.clone())
                    .with_show_download_progress(i == 0),
            )
            .map_err(|e| {
                SemanticSearchError::ModelInitError(format!(
                    "Failed to initialize model instance {}: {}",
                    i + 1,
                    e
                ))
            })?;

            if i == 0 {
                let test_embedding = text_model
                    .embed(vec!["test"], None)
                    .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
                dimensions = test_embedding.into_iter().next().unwrap().len();
            }

            sender
                .send(ModelInstance {
                    model: text_model,
                    id: i,
                })
                .expect("Pool channel should not be closed");
        }

        tracing::info!(
            target: "semantic",
            "Embedding pool ready: {pool_size} instances, {dimensions} dimensions"
        );

        Ok(Self {
            model_sender: sender,
            model_receiver: receiver,
            pool_size,
            dimensions,
            model_name,
            usage_counters,
        })
    }

    /// Create a pool with default model (AllMiniLML6V2).
    pub fn with_size(pool_size: usize) -> Result<Self, SemanticSearchError> {
        Self::new(pool_size, EmbeddingModel::AllMiniLML6V2)
    }

    /// Acquire a model from the pool (blocks if none available).
    fn acquire(&self) -> ModelInstance {
        let instance = self
            .model_receiver
            .recv()
            .expect("Pool should not be empty");
        self.usage_counters[instance.id].fetch_add(1, Ordering::Relaxed);
        instance
    }

    /// Return a model to the pool.
    fn release(&self, instance: ModelInstance) {
        let _ = self.model_sender.send(instance);
    }

    /// Get the embedding dimensions.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Get the pool size.
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Generate embedding for a single text. Thread-safe via pool acquire/release.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>, SemanticSearchError> {
        if text.trim().is_empty() {
            return Err(SemanticSearchError::EmbeddingError(
                "Empty text".to_string(),
            ));
        }

        let mut instance = self.acquire();
        let result = instance
            .model
            .embed(vec![text], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()));
        self.release(instance);

        result.map(|mut v| v.remove(0))
    }

    /// Log usage statistics for all model instances.
    pub fn log_usage_stats(&self) {
        let counts: Vec<usize> = self
            .usage_counters
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect();
        let total: usize = counts.iter().sum();

        if total > 0 {
            let usage_str: Vec<String> = counts
                .iter()
                .enumerate()
                .map(|(i, c)| format!("model[{i}]={c}"))
                .collect();
            tracing::info!(
                target: "semantic",
                "Embedding pool usage: {} (total: {total})",
                usage_str.join(", ")
            );
        }
    }

    /// Generate embeddings for multiple items in parallel using rayon.
    ///
    /// Uses batched embedding (64 docs per model call) for throughput.
    /// Failed embeddings are logged and skipped.
    pub fn embed_parallel(
        &self,
        items: &[(SymbolId, &str, &str)],
    ) -> Vec<(SymbolId, Vec<f32>, String)> {
        use rayon::prelude::*;

        const BATCH_SIZE: usize = 64;

        let valid_items: Vec<_> = items
            .iter()
            .filter(|(_, doc, _)| !doc.trim().is_empty())
            .collect();

        if valid_items.is_empty() {
            return Vec::new();
        }

        let results: Vec<_> = valid_items
            .chunks(BATCH_SIZE)
            .par_bridge()
            .flat_map(|batch| {
                let texts: Vec<&str> = batch.iter().map(|(_, doc, _)| *doc).collect();

                let mut instance = self.acquire();
                let embeddings_result = instance.model.embed(texts.clone(), None);
                self.release(instance);

                match embeddings_result {
                    Ok(embeddings) => {
                        let mut results = Vec::with_capacity(batch.len());
                        for (item, embedding) in batch.iter().zip(embeddings) {
                            let (symbol_id, _, language) = *item;
                            if embedding.len() == self.dimensions {
                                results.push((*symbol_id, embedding, (*language).to_string()));
                            } else {
                                tracing::warn!(
                                    target: "semantic",
                                    "Dimension mismatch for {}: expected {}, got {}",
                                    symbol_id.to_u32(),
                                    self.dimensions,
                                    embedding.len()
                                );
                            }
                        }
                        results
                    }
                    Err(e) => {
                        tracing::warn!(target: "semantic", "Batch embedding failed: {e}");
                        Vec::new()
                    }
                }
            })
            .collect();

        self.log_usage_stats();
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored"]
    fn test_pool_creation() {
        let pool = EmbeddingPool::with_size(2).unwrap();
        assert_eq!(pool.pool_size(), 2);
        assert_eq!(pool.dimensions(), 384);
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored"]
    fn test_parallel_embedding() {
        let pool = EmbeddingPool::with_size(2).unwrap();
        let items = vec![
            (SymbolId::new(1).unwrap(), "Parse JSON data", "rust"),
            (SymbolId::new(2).unwrap(), "Connect to database", "rust"),
        ];
        let results = pool.embed_parallel(&items);
        assert_eq!(results.len(), 2);
    }
}
