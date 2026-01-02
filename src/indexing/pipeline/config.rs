//! Pipeline configuration
//!
//! Controls threading, batching, and channel sizes for the parallel pipeline.
//! Reads from Settings (.codanna/settings.toml).

use crate::Settings;

/// Configuration for the parallel indexing pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Number of threads for parallel parsing (default: CPU count - 2)
    pub parse_threads: usize,

    /// Number of threads for file reading (default: 2)
    pub read_threads: usize,

    /// Number of threads for file discovery/walking (default: 4)
    pub discover_threads: usize,

    /// Number of symbols per batch before sending to INDEX stage
    pub batch_size: usize,

    /// Channel capacity for file paths (DISCOVER → READ)
    pub path_channel_size: usize,

    /// Channel capacity for file contents (READ → PARSE)
    pub content_channel_size: usize,

    /// Channel capacity for parsed files (PARSE → COLLECT)
    pub parsed_channel_size: usize,

    /// Channel capacity for index batches (COLLECT → INDEX)
    pub batch_channel_size: usize,

    /// Number of batches between Tantivy commits
    pub batches_per_commit: usize,

    /// Enable detailed stage tracing (timing, memory, throughput)
    pub pipeline_tracing: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();
        let parse_threads = cpu_count.saturating_sub(2).max(1);

        Self {
            parse_threads,
            read_threads: 2,
            discover_threads: 4,
            batch_size: 5000,
            path_channel_size: 1000,
            content_channel_size: 100,
            parsed_channel_size: 1000,
            batch_channel_size: 20,
            batches_per_commit: 10,
            pipeline_tracing: false,
        }
    }
}

impl PipelineConfig {
    /// Create config from Settings.
    ///
    /// Reads from .codanna/settings.toml:
    /// - `indexing.parallel_threads` -> parse_threads
    /// - `indexing.read_threads` -> read_threads
    /// - `indexing.discover_threads` -> discover_threads
    /// - `indexing.batch_size` -> batch_size
    /// - `indexing.batches_per_commit` -> batches_per_commit
    /// - `indexing.pipeline_tracing` -> pipeline_tracing
    pub fn from_settings(settings: &Settings) -> Self {
        let indexing = &settings.indexing;

        // Parse threads: use parallel_threads but reserve some for I/O
        let parse_threads = indexing.parallel_threads.saturating_sub(4).max(1);

        // Channel sizes derived from thread counts
        let path_channel_size = indexing.parallel_threads * 100;
        let content_channel_size = indexing.read_threads * 50;
        let parsed_channel_size = parse_threads * 100;
        let batch_channel_size = 20;

        Self {
            parse_threads,
            read_threads: indexing.read_threads,
            discover_threads: indexing.discover_threads,
            batch_size: indexing.batch_size,
            path_channel_size,
            content_channel_size,
            parsed_channel_size,
            batch_channel_size,
            batches_per_commit: indexing.batches_per_commit,
            pipeline_tracing: indexing.pipeline_tracing,
        }
    }

    /// Create config optimized for small codebases (<1000 files)
    pub fn small() -> Self {
        Self {
            parse_threads: 4,
            read_threads: 1,
            discover_threads: 2,
            batch_size: 1000,
            path_channel_size: 500,
            content_channel_size: 50,
            parsed_channel_size: 500,
            batch_channel_size: 10,
            batches_per_commit: 5,
            pipeline_tracing: false,
        }
    }

    /// Create config optimized for large codebases (>10000 files)
    pub fn large() -> Self {
        let cpu_count = num_cpus::get();
        Self {
            parse_threads: cpu_count.saturating_sub(2).max(4),
            read_threads: 4,
            discover_threads: 4,
            batch_size: 10000,
            path_channel_size: 2000,
            content_channel_size: 200,
            parsed_channel_size: 2000,
            batch_channel_size: 50,
            batches_per_commit: 20,
            pipeline_tracing: false,
        }
    }

    /// Set parse thread count
    pub fn with_parse_threads(mut self, threads: usize) -> Self {
        self.parse_threads = threads.max(1);
        self
    }

    /// Set batch size
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size.max(100);
        self
    }

    /// Set batches per commit
    pub fn with_batches_per_commit(mut self, count: usize) -> Self {
        self.batches_per_commit = count.max(1);
        self
    }

    /// Calculate total channel buffer memory (approximate)
    pub fn estimated_memory_mb(&self) -> usize {
        // Rough estimates:
        // - Path: 100 bytes avg
        // - Content: 10KB avg
        // - Parsed: 50KB avg
        // - Batch: 500KB avg
        let path_mem = self.path_channel_size * 100;
        let content_mem = self.content_channel_size * 10_000;
        let parsed_mem = self.parsed_channel_size * 50_000;
        let batch_mem = self.batch_channel_size * 500_000;

        (path_mem + content_mem + parsed_mem + batch_mem) / 1_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PipelineConfig::default();
        assert!(config.parse_threads >= 1);
        assert_eq!(config.read_threads, 2);
        assert_eq!(config.batch_size, 5000);
    }

    #[test]
    fn test_config_builder() {
        let config = PipelineConfig::default()
            .with_parse_threads(8)
            .with_batch_size(2000);

        assert_eq!(config.parse_threads, 8);
        assert_eq!(config.batch_size, 2000);
    }

    #[test]
    fn test_from_settings() {
        let settings = Settings::default();
        let config = PipelineConfig::from_settings(&settings);

        // Should use values from settings.indexing
        assert_eq!(config.batch_size, settings.indexing.batch_size);
        assert_eq!(config.read_threads, settings.indexing.read_threads);
        assert_eq!(
            config.batches_per_commit,
            settings.indexing.batches_per_commit
        );

        // parse_threads derived from parallel_threads
        assert!(config.parse_threads >= 1);

        println!("Config from settings:");
        println!("  parse_threads: {}", config.parse_threads);
        println!("  read_threads: {}", config.read_threads);
        println!("  batch_size: {}", config.batch_size);
        println!("  batches_per_commit: {}", config.batches_per_commit);
    }

    #[test]
    fn test_memory_estimate() {
        let config = PipelineConfig::default();
        let mem = config.estimated_memory_mb();
        // Should be reasonable (< 100MB for default config)
        assert!(mem < 100);
    }
}
