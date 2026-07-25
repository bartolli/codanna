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
    /// Local pool exhaustion surfaces as an error; remote failures keep their
    /// existing log-and-degrade semantics.
    pub fn embed_parallel(
        &self,
        items: &[(SymbolId, &str, &str)],
    ) -> Result<Vec<(SymbolId, Vec<f32>, String)>, SemanticSearchError> {
        match self {
            EmbeddingBackend::Local(pool) => pool.embed_parallel(items),
            EmbeddingBackend::Remote(r) => {
                let r = Arc::clone(r);
                let texts: Vec<String> = items.iter().map(|(_, t, _)| t.to_string()).collect();
                let dim = r.dim();

                let embeddings = run_async(async move { r.embed(&texts).await });

                match embeddings {
                    Ok(embs) => Ok(embs
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
                        .collect()),
                    Err(e) => {
                        tracing::error!(target: "semantic", "Remote embed_parallel failed: {e}");
                        Ok(Vec::new())
                    }
                }
            }
        }
    }
}

// ── EmbeddingPool (local fastembed) ───────────────────────────────────────

/// Fixed-size pool of checkout instances. `acquire` bounds its wait and the
/// returned guard releases on drop, so a panicking holder cannot shrink the
/// pool permanently.
struct InstancePool<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
    size: usize,
}

/// Checked-out instance. Returns itself to the pool on drop, including
/// during unwind.
struct PooledInstance<T> {
    item: Option<T>,
    sender: Sender<T>,
}

impl<T> InstancePool<T> {
    fn new(items: Vec<T>) -> Self {
        let size = items.len();
        let (sender, receiver) = bounded(size);
        for item in items {
            sender
                .send(item)
                .expect("bounded(size) channel cannot be full or closed during fill");
        }
        Self {
            sender,
            receiver,
            size,
        }
    }

    fn acquire(
        &self,
        timeout: std::time::Duration,
    ) -> Result<PooledInstance<T>, SemanticSearchError> {
        use crossbeam_channel::RecvTimeoutError;

        match self.receiver.recv_timeout(timeout) {
            Ok(item) => Ok(PooledInstance {
                item: Some(item),
                sender: self.sender.clone(),
            }),
            Err(RecvTimeoutError::Timeout) => Err(SemanticSearchError::PoolExhausted {
                pool_size: self.size,
                waited: timeout,
            }),
            Err(RecvTimeoutError::Disconnected) => {
                unreachable!("pool holds a sender; the channel cannot disconnect")
            }
        }
    }
}

impl<T> std::ops::Deref for PooledInstance<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.item
            .as_ref()
            .expect("item is Some until drop takes it")
    }
}

impl<T> std::ops::DerefMut for PooledInstance<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.item
            .as_mut()
            .expect("item is Some until drop takes it")
    }
}

impl<T> Drop for PooledInstance<T> {
    fn drop(&mut self) {
        if let Some(item) = self.item.take() {
            // send fails only when the pool itself is gone; nothing to restore then
            let _ = self.sender.send(item);
        }
    }
}

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
    instances: InstancePool<ModelInstance>,
    /// Dedicated rayon pool for the embed fan-out, sized 1:1 with instances.
    /// Embed chunks must not run on the global rayon pool: they block in
    /// `acquire`, and the tokenizer inside fastembed schedules nested work on
    /// the caller's pool — parking global workers in `recv` starves that work.
    embed_workers: rayon::ThreadPool,
    dimensions: usize,
    model_name: String,
    usage_counters: Vec<AtomicUsize>,
}

/// Upper bound on waiting for a pool instance. Legitimate backpressure
/// resolves in seconds; a wait this long is a wedged or leaked embedder and
/// surfaces as `PoolExhausted` instead of an indefinite silent block.
const ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

impl EmbeddingPool {
    /// Create a new embedding pool with the specified number of model instances.
    ///
    /// Each model instance uses ~86MB of memory for AllMiniLML6V2.
    pub fn new(pool_size: usize, model: EmbeddingModel) -> Result<Self, SemanticSearchError> {
        let pool_size = pool_size.max(1);

        let cache_dir = crate::init::models_dir();
        let model_name = crate::vector::model_to_string(&model);

        tracing::info!(
            target: "semantic",
            "Initializing embedding pool: {pool_size} instances ({model_name})"
        );

        let mut dimensions = 0;
        let usage_counters: Vec<AtomicUsize> =
            (0..pool_size).map(|_| AtomicUsize::new(0)).collect();
        let mut models = Vec::with_capacity(pool_size);

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

            models.push(ModelInstance {
                model: text_model,
                id: i,
            });
        }

        let embed_workers = rayon::ThreadPoolBuilder::new()
            .num_threads(pool_size)
            .thread_name(|i| format!("codanna-embed-{i}"))
            .build()
            .map_err(|e| SemanticSearchError::ModelInitError(format!("embed worker pool: {e}")))?;

        tracing::info!(
            target: "semantic",
            "Embedding pool ready: {pool_size} instances, {dimensions} dimensions"
        );

        Ok(Self {
            instances: InstancePool::new(models),
            embed_workers,
            dimensions,
            model_name,
            usage_counters,
        })
    }

    /// Create a pool with default model (AllMiniLML6V2).
    pub fn with_size(pool_size: usize) -> Result<Self, SemanticSearchError> {
        Self::new(pool_size, EmbeddingModel::AllMiniLML6V2)
    }

    /// Acquire a model from the pool. Waits up to `ACQUIRE_TIMEOUT`; the
    /// returned guard releases on drop, including during unwind.
    fn acquire(&self) -> Result<PooledInstance<ModelInstance>, SemanticSearchError> {
        let instance = self.instances.acquire(ACQUIRE_TIMEOUT)?;
        self.usage_counters[instance.id].fetch_add(1, Ordering::Relaxed);
        Ok(instance)
    }

    /// Get the embedding dimensions.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Get the pool size.
    pub fn pool_size(&self) -> usize {
        self.instances.size
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

        let mut instance = self.acquire()?;
        let result = instance
            .model
            .embed(vec![text], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()));
        drop(instance);

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
    ) -> Result<Vec<(SymbolId, Vec<f32>, String)>, SemanticSearchError> {
        use rayon::prelude::*;

        const BATCH_SIZE: usize = 64;

        let valid_items: Vec<_> = items
            .iter()
            .filter(|(_, doc, _)| !doc.trim().is_empty())
            .collect();

        if valid_items.is_empty() {
            return Ok(Vec::new());
        }

        // Failed batches warn and skip; pool exhaustion aborts the whole call.
        let results: Result<Vec<Vec<_>>, SemanticSearchError> = self.embed_workers.install(|| {
            valid_items
                .chunks(BATCH_SIZE)
                .par_bridge()
                .map(|batch| {
                    let texts: Vec<&str> = batch.iter().map(|(_, doc, _)| *doc).collect();

                    let mut instance = self.acquire()?;
                    let embeddings_result = instance.model.embed(texts, None);
                    drop(instance);

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
                            Ok(results)
                        }
                        Err(e) => {
                            tracing::warn!(target: "semantic", "Batch embedding failed: {e}");
                            Ok(Vec::new())
                        }
                    }
                })
                .collect()
        });

        let results = results?.into_iter().flatten().collect();
        self.log_usage_stats();
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_acquire_times_out_when_all_instances_checked_out() {
        let pool = InstancePool::new(vec![(), ()]);
        let _a = pool.acquire(Duration::from_millis(10)).unwrap();
        let _b = pool.acquire(Duration::from_millis(10)).unwrap();

        let err = pool
            .acquire(Duration::from_millis(50))
            .err()
            .expect("third acquire must time out");
        let msg = err.to_string();
        assert!(msg.contains("pool size 2"), "error names pool size: {msg}");
    }

    #[test]
    fn test_dropped_guard_returns_instance_to_pool() {
        let pool = InstancePool::new(vec![()]);
        let guard = pool.acquire(Duration::from_millis(10));
        assert!(guard.is_ok());
        drop(guard);

        assert!(pool.acquire(Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn test_panicking_holder_returns_instance_to_pool() {
        let pool = InstancePool::new(vec![()]);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = pool
                .acquire(Duration::from_millis(10))
                .expect("acquire with instance available");
            panic!("simulated embed panic");
        }));
        assert!(result.is_err());

        assert!(
            pool.acquire(Duration::from_millis(50)).is_ok(),
            "instance must return to the pool during unwind"
        );
    }

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
        let results = pool.embed_parallel(&items).unwrap();
        assert_eq!(results.len(), 2);
    }
}
