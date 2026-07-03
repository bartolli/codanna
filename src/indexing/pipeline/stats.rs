//! Statistics types for pipeline runs.

use super::stages::cleanup::CleanupStats;
use crate::indexing::IndexStats;

/// Statistics from sync_with_config operation.
#[derive(Debug, Default)]
pub struct SyncStats {
    /// Number of new directories indexed
    pub added_dirs: usize,
    /// Number of directories removed from index
    pub removed_dirs: usize,
    /// Total files indexed from new directories
    pub files_indexed: usize,
    /// Total symbols found in new directories
    pub symbols_found: usize,
    /// Files removed during cleanup
    pub files_removed: usize,
    /// Symbols removed during cleanup
    pub symbols_removed: usize,
    /// Time taken
    pub elapsed: std::time::Duration,
}

/// Statistics from incremental indexing.
#[derive(Debug, Default)]
pub struct IncrementalStats {
    /// Number of new files indexed
    pub new_files: usize,
    /// Number of modified files re-indexed
    pub modified_files: usize,
    /// Number of deleted files cleaned up
    pub deleted_files: usize,
    /// Phase 1 indexing stats
    pub index_stats: IndexStats,
    /// Cleanup stats
    pub cleanup_stats: CleanupStats,
    /// Phase 2 resolution stats
    pub phase2_stats: Phase2Stats,
    /// Total time taken
    pub elapsed: std::time::Duration,
}

/// Statistics from Phase 2 resolution.
#[derive(Debug, Default)]
pub struct Phase2Stats {
    /// Total relationships to resolve
    pub total_relationships: usize,
    /// Defines relationships resolved (Pass 1)
    pub defines_resolved: usize,
    /// Calls relationships resolved (Pass 2)
    pub calls_resolved: usize,
    /// Other relationships resolved (Pass 2)
    pub other_resolved: usize,
    /// Failed to resolve
    pub unresolved: usize,
    /// Time taken
    pub elapsed: std::time::Duration,
}

/// Statistics from pipeline execution.
#[derive(Debug, Default)]
pub struct PipelineStats {
    /// Time spent in each stage (milliseconds)
    pub stage_times: StageTimings,
    /// Number of files processed
    pub files_processed: usize,
    /// Number of files that failed to parse
    pub files_failed: usize,
    /// Total symbols extracted
    pub symbols_extracted: usize,
    /// Total relationships found
    pub relationships_found: usize,
    /// Total relationships resolved
    pub relationships_resolved: usize,
    /// Peak memory usage (bytes)
    pub peak_memory: usize,
}

/// Timing breakdown by stage.
#[derive(Debug, Default)]
pub struct StageTimings {
    pub discover_ms: u64,
    pub read_ms: u64,
    pub parse_ms: u64,
    pub collect_ms: u64,
    pub index_ms: u64,
    pub resolve_ms: u64,
}

impl StageTimings {
    pub fn total_ms(&self) -> u64 {
        self.discover_ms
            + self.read_ms
            + self.parse_ms
            + self.collect_ms
            + self.index_ms
            + self.resolve_ms
    }
}
