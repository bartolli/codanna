//! Semantic embedding stage - parallel embedding generation
//!
//! COLLECT ─┬─> EMBED (this stage) ─> SimpleSemanticSearch
//!          └─> INDEX ─> Tantivy
//!
//! Receives EmbeddingBatch from COLLECT, generates embeddings using EmbeddingPool,
//! stores them in SimpleSemanticSearch. Runs in parallel with INDEX stage.

use crate::indexing::pipeline::types::{EmbeddingBatch, PipelineError, PipelineResult};
use crate::semantic::{EmbeddingPool, SimpleSemanticSearch};
use crossbeam_channel::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Statistics from the EMBED stage.
#[derive(Debug, Clone, Default)]
pub struct SemanticEmbedStats {
    /// Total candidates received
    pub received: usize,
    /// Successfully embedded
    pub embedded: usize,
    /// Skipped (empty doc, dimension mismatch)
    pub skipped: usize,
    /// Time waiting on input channel
    pub input_wait: Duration,
    /// Total processing time
    pub elapsed: Duration,
}

impl SemanticEmbedStats {
    /// Check if all received candidates were processed.
    pub fn is_complete(&self) -> bool {
        self.embedded + self.skipped == self.received
    }
}

/// Progress callback type for EMBED stage.
pub type EmbedProgressCallback = Arc<dyn Fn(u64) + Send + Sync>;

/// Semantic embedding stage using EmbeddingPool.
///
/// Receives EmbeddingBatch from COLLECT, generates embeddings in parallel,
/// stores them in SimpleSemanticSearch.
pub struct SemanticEmbedStage {
    pool: Arc<EmbeddingPool>,
    semantic: Arc<Mutex<SimpleSemanticSearch>>,
    progress_callback: Option<EmbedProgressCallback>,
}

impl SemanticEmbedStage {
    /// Create a new semantic embed stage.
    pub fn new(pool: Arc<EmbeddingPool>, semantic: Arc<Mutex<SimpleSemanticSearch>>) -> Self {
        Self {
            pool,
            semantic,
            progress_callback: None,
        }
    }

    /// Add a progress callback that receives the count of embeddings processed per batch.
    pub fn with_progress(mut self, callback: EmbedProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Run the embed stage.
    ///
    /// Receives EmbeddingBatch from channel, generates embeddings, stores them.
    /// Runs until channel is closed.
    pub fn run(&self, receiver: Receiver<EmbeddingBatch>) -> PipelineResult<SemanticEmbedStats> {
        const STATS_LOG_INTERVAL: Duration = Duration::from_secs(10);

        tracing::info!(target: "semantic", "EMBED stage started, waiting for batches...");

        let start = Instant::now();
        let mut stats = SemanticEmbedStats::default();
        let mut last_stats_log = Instant::now();
        let mut batches_received = 0usize;

        loop {
            let recv_start = Instant::now();
            match receiver.recv() {
                Ok(batch) => {
                    batches_received += 1;
                    stats.input_wait += recv_start.elapsed();

                    let candidate_count = batch.candidates.len();
                    stats.received += candidate_count;

                    tracing::debug!(
                        target: "semantic",
                        "EMBED batch {}: {} candidates",
                        batches_received,
                        candidate_count
                    );

                    if !batch.candidates.is_empty() {
                        let count = self.process_batch(&batch)?;
                        stats.embedded += count;
                        stats.skipped += candidate_count - count;

                        // Report progress
                        if let Some(ref callback) = self.progress_callback {
                            callback(count as u64);
                        }

                        // Log pool stats periodically
                        if last_stats_log.elapsed() >= STATS_LOG_INTERVAL {
                            self.pool.log_usage_stats();
                            last_stats_log = Instant::now();
                        }
                    }
                }
                Err(_) => break, // Channel closed
            }
        }

        // Log final pool stats
        self.pool.log_usage_stats();

        stats.elapsed = start.elapsed();

        tracing::info!(
            target: "semantic",
            "EMBED complete: {}/{} embedded in {:?} ({} batches)",
            stats.embedded,
            stats.received,
            stats.elapsed,
            batches_received
        );

        Ok(stats)
    }

    /// Process a batch of embedding candidates.
    fn process_batch(&self, batch: &EmbeddingBatch) -> PipelineResult<usize> {
        // Convert to the format expected by embed_parallel
        let items: Vec<_> = batch
            .candidates
            .iter()
            .map(|(id, doc, lang)| (*id, doc.as_ref(), lang.as_ref()))
            .collect();

        // Generate embeddings in parallel using pool
        let embeddings = self.pool.embed_parallel(&items);
        let count = embeddings.len();

        // Store in semantic search
        if !embeddings.is_empty() {
            let mut semantic = self.semantic.lock().map_err(|_| PipelineError::Parse {
                path: std::path::PathBuf::new(),
                reason: "Failed to lock semantic search".to_string(),
            })?;
            semantic.store_embeddings(embeddings);
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_embed_stats_is_complete() {
        let mut stats = SemanticEmbedStats::default();
        assert!(stats.is_complete()); // 0 received, 0 processed

        stats.received = 10;
        stats.embedded = 8;
        stats.skipped = 2;
        assert!(stats.is_complete());

        stats.skipped = 1;
        assert!(!stats.is_complete()); // 8 + 1 != 10
    }
}
