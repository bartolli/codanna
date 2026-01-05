//! Embedding model pool for parallel embedding generation
//!
//! Provides multiple TextEmbedding instances that can be used concurrently
//! by different threads, enabling parallel embedding generation.

use crate::SymbolId;
use crossbeam_channel::{Receiver, Sender, bounded};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::SemanticSearchError;

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
    /// Channel to acquire models from the pool
    model_sender: Sender<ModelInstance>,
    model_receiver: Receiver<ModelInstance>,
    /// Number of models in the pool
    pool_size: usize,
    /// Model dimensions (all models have same dimensions)
    dimensions: usize,
    /// Model name for metadata
    model_name: String,
    /// Usage counters per model instance (for tracing)
    usage_counters: Vec<AtomicUsize>,
}

impl EmbeddingPool {
    /// Create a new embedding pool with the specified number of model instances.
    ///
    /// # Arguments
    /// * `pool_size` - Number of TextEmbedding instances to create
    /// * `model` - The embedding model to use
    ///
    /// # Note
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

        // Create usage counters for each model
        let usage_counters: Vec<AtomicUsize> =
            (0..pool_size).map(|_| AtomicUsize::new(0)).collect();

        // Create pool_size model instances
        for i in 0..pool_size {
            let mut text_model = TextEmbedding::try_new(
                InitOptions::new(model.clone())
                    .with_cache_dir(cache_dir.clone())
                    .with_show_download_progress(i == 0), // Only show progress for first model
            )
            .map_err(|e| {
                SemanticSearchError::ModelInitError(format!(
                    "Failed to initialize model instance {}: {}",
                    i + 1,
                    e
                ))
            })?;

            // Get dimensions from first model
            if i == 0 {
                let test_embedding = text_model
                    .embed(vec!["test"], None)
                    .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
                dimensions = test_embedding.into_iter().next().unwrap().len();
            }

            let instance = ModelInstance {
                model: text_model,
                id: i,
            };
            sender
                .send(instance)
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

    /// Create a pool with default model (AllMiniLML6V2)
    pub fn with_size(pool_size: usize) -> Result<Self, SemanticSearchError> {
        Self::new(pool_size, EmbeddingModel::AllMiniLML6V2)
    }

    /// Acquire a model from the pool (blocks if none available)
    fn acquire(&self) -> ModelInstance {
        let instance = self
            .model_receiver
            .recv()
            .expect("Pool should not be empty");

        // Increment usage counter for this model
        self.usage_counters[instance.id].fetch_add(1, Ordering::Relaxed);

        instance
    }

    /// Return a model to the pool
    fn release(&self, instance: ModelInstance) {
        let _ = self.model_sender.send(instance);
    }

    /// Get the embedding dimensions
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Get the pool size
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Generate embedding for a single document using a pooled model.
    ///
    /// Thread-safe: acquires model, generates embedding, returns model.
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

    /// Generate embeddings for multiple documents in parallel using rayon.
    ///
    /// Uses batched embedding (64 docs per model call) for optimal throughput.
    /// Returns a Vec of (SymbolId, embedding, language) for successful embeddings.
    /// Failed embeddings are logged and skipped.
    pub fn embed_parallel(
        &self,
        items: &[(SymbolId, &str, &str)],
    ) -> Vec<(SymbolId, Vec<f32>, String)> {
        use rayon::prelude::*;

        // Optimal batch size for embedding models (matches benchmark)
        const BATCH_SIZE: usize = 64;

        // Filter out empty docs first
        let valid_items: Vec<_> = items
            .iter()
            .filter(|(_, doc, _)| !doc.trim().is_empty())
            .collect();

        if valid_items.is_empty() {
            return Vec::new();
        }

        // Process in batches of 64, parallelized across available model instances
        let results: Vec<_> = valid_items
            .chunks(BATCH_SIZE)
            .par_bridge()
            .flat_map(|batch| {
                // Collect texts for batch embedding
                let texts: Vec<&str> = batch.iter().map(|(_, doc, _)| *doc).collect();

                // Acquire model, embed entire batch, release model
                let mut instance = self.acquire();
                let embeddings_result = instance.model.embed(texts.clone(), None);
                self.release(instance);

                // Process results
                match embeddings_result {
                    Ok(embeddings) => {
                        let mut results = Vec::with_capacity(batch.len());
                        for (item, embedding) in batch.iter().zip(embeddings.into_iter()) {
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
                        tracing::warn!(
                            target: "semantic",
                            "Batch embedding failed: {}",
                            e
                        );
                        Vec::new()
                    }
                }
            })
            .collect();

        // Log usage stats after parallel embedding
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
        assert_eq!(pool.dimensions(), 384); // AllMiniLML6V2
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored"]
    fn test_parallel_embedding() {
        let pool = EmbeddingPool::with_size(2).unwrap();

        let items = vec![
            (SymbolId::new(1).unwrap(), "Parse JSON data", "rust"),
            (SymbolId::new(2).unwrap(), "Connect to database", "rust"),
            (SymbolId::new(3).unwrap(), "Calculate hash", "rust"),
        ];

        let results = pool.embed_parallel(&items);
        assert_eq!(results.len(), 3);
    }
}
