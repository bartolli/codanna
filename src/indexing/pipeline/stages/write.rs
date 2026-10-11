//! Write stage - store resolved relationships to Tantivy
//!
//! Final stage of Phase 2: takes ResolvedBatch from RESOLVE stage
//! and writes relationships to DocumentIndex.
//!
//! Data flow:
//! - Receives: ResolvedBatch from RESOLVE stage
//! - Writes: Relationships to Tantivy via DocumentIndex
//! - Outputs: WriteStats with counts

use crate::indexing::pipeline::types::{ResolvedBatch, ResolvedRelationship};
use crate::relationship::Relationship;
use crate::storage::DocumentIndex;
use std::sync::Arc;

/// Write stage for storing resolved relationships.
///
/// Single-threaded: Tantivy IndexWriter is not Send.
/// Batches writes and commits periodically.
pub struct WriteStage {
    index: Arc<DocumentIndex>,
    /// Accumulated relationships before commit
    pending: Vec<ResolvedRelationship>,
    /// Commit every N relationships
    commit_threshold: usize,
    /// Whether a batch is currently active
    batch_started: bool,
    /// Whether this stage has called commit_batch at least once
    commit_attempted: bool,
}

/// Statistics from write operations.
#[derive(Debug, Default, Clone)]
pub struct WriteStats {
    /// Total relationships written
    pub written: usize,
    /// Number of commits performed
    pub commits: usize,
}

impl WriteStage {
    /// Create a new write stage with the document index.
    pub fn new(index: Arc<DocumentIndex>) -> Self {
        Self {
            index,
            pending: Vec::new(),
            commit_threshold: 10_000, // Commit every 10K relationships
            batch_started: false,
            commit_attempted: false,
        }
    }

    /// Create with custom commit threshold.
    pub fn with_commit_threshold(index: Arc<DocumentIndex>, threshold: usize) -> Self {
        Self {
            index,
            pending: Vec::new(),
            commit_threshold: threshold,
            batch_started: false,
            commit_attempted: false,
        }
    }

    /// Start a batch if none is active.
    fn ensure_batch_started(&mut self) -> Result<(), crate::storage::StorageError> {
        if !self.batch_started {
            self.index.start_batch()?;
            self.batch_started = true;
        }
        Ok(())
    }

    /// Write a batch of resolved relationships.
    ///
    /// Accumulates in memory and commits when threshold reached. The
    /// batch is acquired per store, so an empty `batch` opens nothing.
    /// A batch that cannot start, a store that fails, or a threshold
    /// commit that fails is an error: the active batch is rolled back and
    /// nothing from `batch` is reported written.
    pub fn write(
        &mut self,
        batch: ResolvedBatch,
    ) -> Result<WriteStats, crate::storage::StorageError> {
        let mut stats = WriteStats::default();

        for resolved in batch.relationships {
            if self.stage(resolved)? {
                stats.commits += 1;
            }
            stats.written += 1;
        }

        Ok(stats)
    }

    /// Write a single resolved relationship.
    pub fn write_one(
        &mut self,
        resolved: ResolvedRelationship,
    ) -> Result<(), crate::storage::StorageError> {
        self.stage(resolved)?;
        Ok(())
    }

    /// Acquire the batch, stage one relationship, and commit at the
    /// threshold. Returns whether a commit ran. A store or commit failure
    /// abandons the active batch.
    fn stage(
        &mut self,
        resolved: ResolvedRelationship,
    ) -> Result<bool, crate::storage::StorageError> {
        // Clone metadata to avoid a partial move out of `resolved`.
        let relationship = Relationship {
            kind: resolved.kind,
            weight: 1.0, // Default weight
            metadata: resolved.metadata.clone(),
        };

        self.ensure_batch_started()?;
        if let Err(e) =
            self.index
                .store_relationship(resolved.from_id, resolved.to_id, &relationship)
        {
            self.abandon_batch();
            return Err(e);
        }
        self.pending.push(resolved);

        if self.pending.len() >= self.commit_threshold {
            self.commit_internal()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Commit pending writes to disk.
    ///
    /// Call after each pass (Defines, then Calls) to ensure
    /// Pass 2 can query Pass 1 results.
    pub fn commit(&mut self) -> Result<usize, crate::storage::StorageError> {
        let count = self.pending.len();
        self.commit_internal()?;
        Ok(count)
    }

    /// Commit the active batch and release the writer; the next store
    /// acquires a fresh batch. With no batch open there is nothing to
    /// commit and no commit is attempted. A failure abandons the batch.
    fn commit_internal(&mut self) -> Result<(), crate::storage::StorageError> {
        if !self.batch_started {
            return Ok(());
        }
        self.commit_attempted = true;
        if let Err(e) = self.index.commit_batch() {
            self.abandon_batch();
            return Err(e);
        }
        self.pending.clear();
        self.batch_started = false;
        Ok(())
    }

    /// Discard the active batch after a failure: staged rows never reach
    /// a commit and the next write starts a fresh batch. commit_batch has
    /// already taken the writer when a commit fails, so the rollback is
    /// best-effort.
    fn abandon_batch(&mut self) {
        if let Err(rollback_err) = self.index.rollback_batch() {
            tracing::warn!(
                target: "pipeline",
                "Rollback after write failure also failed: {rollback_err}"
            );
        }
        self.pending.clear();
        self.batch_started = false;
    }

    /// Flush any remaining pending writes.
    ///
    /// Call at end of Phase 2 to ensure all relationships are committed.
    pub fn flush(&mut self) -> Result<WriteStats, crate::storage::StorageError> {
        let written = self.pending.len();
        if written > 0 && self.batch_started {
            self.commit_attempted = true;
            // Commit without restarting batch (we're done)
            self.index.commit_batch()?;
            self.pending.clear();
            self.batch_started = false;
        }
        Ok(WriteStats {
            written,
            commits: if written > 0 { 1 } else { 0 },
        })
    }

    /// Get count of pending (uncommitted) relationships.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Whether this stage has called commit_batch at least once. False
    /// after a failed write means nothing reached the index: no commit
    /// ran and the staged rows were rolled back. True carries no claim
    /// either way: commit_batch can fail after the rows are durable.
    pub fn commit_attempted(&self) -> bool {
        self.commit_attempted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RelationKind;
    use crate::config::Settings;
    use crate::types::SymbolId;
    use tempfile::TempDir;

    fn make_resolved(from: u32, to: u32, kind: RelationKind) -> ResolvedRelationship {
        ResolvedRelationship {
            from_id: SymbolId::new(from).unwrap(),
            to_id: SymbolId::new(to).unwrap(),
            kind,
            metadata: None,
        }
    }

    #[test]
    fn test_write_empty_batch() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let mut stage = WriteStage::new(index);
        let batch = ResolvedBatch::new();

        let stats = stage.write(batch).unwrap();

        assert_eq!(stats.written, 0);
        assert_eq!(stats.commits, 0);
    }

    #[test]
    fn test_write_batch_accumulates() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        // Use high threshold so no auto-commit
        let mut stage = WriteStage::with_commit_threshold(Arc::clone(&index), 100);

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        batch.push(make_resolved(2, 3, RelationKind::Defines));

        let stats = stage.write(batch).unwrap();

        assert_eq!(stats.written, 2);
        assert_eq!(stage.pending_count(), 2);
        assert_eq!(stats.commits, 0); // No auto-commit yet
    }

    #[test]
    fn test_commit_clears_pending() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let mut stage = WriteStage::new(Arc::clone(&index));

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));

        stage.write(batch).unwrap();
        assert_eq!(stage.pending_count(), 1);

        let count = stage.commit().unwrap();
        assert_eq!(count, 1);
        assert_eq!(stage.pending_count(), 0);
    }

    #[test]
    fn test_auto_commit_at_threshold() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        // Low threshold for testing
        let mut stage = WriteStage::with_commit_threshold(Arc::clone(&index), 2);

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        batch.push(make_resolved(2, 3, RelationKind::Defines));
        batch.push(make_resolved(3, 4, RelationKind::Calls));

        let stats = stage.write(batch).unwrap();

        // 3 written, 1 commit triggered at threshold
        assert_eq!(stats.written, 3);
        assert_eq!(stats.commits, 1);
        // 1 pending after commit (the 3rd one)
        assert_eq!(stage.pending_count(), 1);
    }

    #[test]
    fn test_flush_commits_remaining() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let mut stage = WriteStage::new(Arc::clone(&index));

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));

        stage.write(batch).unwrap();
        assert_eq!(stage.pending_count(), 1);

        let flush_stats = stage.flush().unwrap();
        assert_eq!(flush_stats.written, 1);
        assert_eq!(flush_stats.commits, 1);
        assert_eq!(stage.pending_count(), 0);
    }

    // A batch that cannot start (writer held by another handle) is an
    // error, never a counted drop.
    #[test]
    fn write_with_writer_held_elsewhere_fails_then_succeeds_after_release() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let other = DocumentIndex::new(temp_dir.path(), &settings).unwrap();
        other.start_batch().unwrap();

        let mut stage = WriteStage::new(Arc::clone(&index));
        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        assert!(stage.write(batch).is_err());
        assert!(!stage.commit_attempted());
        assert_eq!(stage.pending_count(), 0);

        other.rollback_batch().unwrap();
        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        let stats = stage.write(batch).unwrap();
        assert_eq!(stats.written, 1);
        assert_eq!(stage.pending_count(), 1);
        let flushed = stage.flush().unwrap();
        assert_eq!(flushed.written, 1);
        assert!(stage.commit_attempted());
    }

    // Control for the failed-commit case: the same two relationships
    // without a fault are durable after the threshold commit.
    #[test]
    fn threshold_commit_without_fault_is_durable() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let mut stage = WriteStage::with_commit_threshold(Arc::clone(&index), 2);

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        batch.push(make_resolved(2, 3, RelationKind::Defines));
        let stats = stage.write(batch).unwrap();

        assert_eq!(stats.written, 2);
        assert_eq!(stats.commits, 1);
        assert_eq!(stage.pending_count(), 0);
        assert_eq!(index.count_relationships().unwrap(), 2);
    }

    // S9: a threshold commit that fails is an error, nothing is reported
    // written, the batch is rolled back, and the writer is released.
    // Fault: the index directory is renamed away once the writer exists,
    // so the segment files the commit must create fail with NotFound;
    // renaming it back lifts the fault with meta.json untouched. Windows
    // refuses to rename a directory holding an open file (the writer's
    // lock file), so the fault is unix-only.
    #[cfg(unix)]
    #[test]
    fn failed_threshold_commit_is_an_error_and_releases_the_writer() {
        let temp_dir = TempDir::new().unwrap();
        let index_dir = temp_dir.path().join("index");
        let fault_dir = temp_dir.path().join("index.unreachable");
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(&index_dir, &settings).unwrap());
        let mut stage = WriteStage::with_commit_threshold(Arc::clone(&index), 2);

        // The writer exists before the fault, so the failure lands on the
        // commit rather than on start_batch.
        index.start_batch().unwrap();
        std::fs::rename(&index_dir, &fault_dir).unwrap();

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        batch.push(make_resolved(2, 3, RelationKind::Defines));
        let result = stage.write(batch);
        std::fs::rename(&fault_dir, &index_dir).unwrap();

        assert!(
            result.is_err(),
            "threshold commit under the fault must fail the write, got {result:?}"
        );
        assert_eq!(stage.pending_count(), 0);
        assert!(!stage.batch_started);

        // Writer released: another handle can start a batch; nothing durable.
        let other = DocumentIndex::new(&index_dir, &settings).unwrap();
        other.start_batch().unwrap();
        assert_eq!(other.count_relationships().unwrap(), 0);
        other.rollback_batch().unwrap();

        // The stage starts a fresh batch once the fault is lifted.
        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        let stats = stage.write(batch).unwrap();
        assert_eq!(stats.written, 1);
        let flushed = stage.flush().unwrap();
        assert_eq!(flushed.written, 1);
        assert_eq!(index.count_relationships().unwrap(), 1);
    }

    // S10: a store that fails (the shared handle's batch was rolled back
    // under the stage) is an error; the stage clears its batch state so
    // the next write starts fresh.
    #[test]
    fn failed_store_is_an_error_and_the_next_write_starts_fresh() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let mut stage = WriteStage::with_commit_threshold(Arc::clone(&index), 100);

        stage.ensure_batch_started().unwrap();
        index.rollback_batch().unwrap();

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        let result = stage.write(batch);
        assert!(
            result.is_err(),
            "store without a writer must fail the write, got {result:?}"
        );
        assert_eq!(stage.pending_count(), 0);
        assert!(!stage.batch_started);
        assert!(!stage.commit_attempted());

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        let stats = stage.write(batch).unwrap();
        assert_eq!(stats.written, 1);
        let flushed = stage.flush().unwrap();
        assert_eq!(flushed.written, 1);
        assert_eq!(index.count_relationships().unwrap(), 1);
    }

    // S2: a write with no relationships opens no batch; after flush a
    // second handle on the same directory can start one.
    #[test]
    fn empty_write_then_flush_leaves_no_batch_open() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let mut stage = WriteStage::new(Arc::clone(&index));

        let stats = stage.write(ResolvedBatch::new()).unwrap();
        assert_eq!(stats.written, 0);
        let flushed = stage.flush().unwrap();
        assert_eq!(flushed.written, 0);
        assert_eq!(flushed.commits, 0);
        assert_eq!(index.count_relationships().unwrap(), 0);

        let other = DocumentIndex::new(temp_dir.path(), &settings).unwrap();
        other
            .start_batch()
            .expect("writer must be released after an empty write and flush");
        other.rollback_batch().unwrap();
    }

    // S3: a threshold crossing inside one write reopens for the row after
    // it; flush commits that row and releases the writer.
    #[test]
    fn threshold_crossing_inside_one_write_reopens_and_flush_releases() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let mut stage = WriteStage::with_commit_threshold(Arc::clone(&index), 2);

        let mut batch = ResolvedBatch::new();
        batch.push(make_resolved(1, 2, RelationKind::Calls));
        batch.push(make_resolved(2, 3, RelationKind::Defines));
        batch.push(make_resolved(3, 4, RelationKind::Calls));
        let stats = stage.write(batch).unwrap();
        assert_eq!(stats.written, 3);
        assert_eq!(stats.commits, 1);
        assert_eq!(stage.pending_count(), 1);

        let flushed = stage.flush().unwrap();
        assert_eq!(flushed.written, 1);
        assert_eq!(flushed.commits, 1);
        assert_eq!(index.count_relationships().unwrap(), 3);

        let other = DocumentIndex::new(temp_dir.path(), &settings).unwrap();
        other
            .start_batch()
            .expect("writer must be released after flush");
        other.rollback_batch().unwrap();
    }
}
