//! Index stage - Tantivy batch writes
//!
//! [PIPELINE API] Part of the new parallel indexing pipeline.
//!
//! Single-threaded stage that:
//! - Receives IndexBatch from COLLECT stage
//! - Writes symbols, imports, file registrations to Tantivy
//! - Accumulates UnresolvedRelationships for Phase 2
//! - Commits every N batches for efficient I/O

use crate::indexing::IndexStats;
use crate::indexing::pipeline::types::{IndexBatch, PipelineResult, UnresolvedRelationship};
use crate::storage::DocumentIndex;
use crossbeam_channel::Receiver;
use std::sync::Arc;

/// Index stage for Tantivy writes.
///
/// [PIPELINE API] Receives batches from COLLECT and writes to Tantivy efficiently.
/// Commits are batched to reduce fsync overhead.
///
/// Error handling uses proper #[from] conversion - StorageError -> PipelineError.
pub struct IndexStage {
    index: Arc<DocumentIndex>,
    batches_per_commit: usize,
}

impl IndexStage {
    /// Create a new index stage.
    ///
    /// `batches_per_commit` controls how often we commit to Tantivy.
    /// Higher values improve throughput but increase memory usage.
    pub fn new(index: Arc<DocumentIndex>, batches_per_commit: usize) -> Self {
        Self {
            index,
            batches_per_commit: batches_per_commit.max(1),
        }
    }

    /// Run the index stage.
    ///
    /// Returns (stats, accumulated_relationships) for Phase 2.
    pub fn run(
        &self,
        receiver: Receiver<IndexBatch>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>)> {
        let mut stats = IndexStats::new();
        let mut pending_relationships: Vec<UnresolvedRelationship> = Vec::new();
        let mut batch_count = 0;

        // Start initial batch - StorageError converts to PipelineError via #[from]
        self.index.start_batch()?;

        for batch in receiver {
            self.process_batch(&batch, &mut stats)?;

            // Accumulate relationships for Phase 2
            pending_relationships.extend(batch.unresolved_relationships);

            batch_count += 1;

            // Commit every N batches
            if batch_count % self.batches_per_commit == 0 {
                self.commit_and_restart()?;
            }
        }

        // Final commit
        self.index.commit_batch()?;

        Ok((stats, pending_relationships))
    }

    /// Process a single batch.
    fn process_batch(&self, batch: &IndexBatch, stats: &mut IndexStats) -> PipelineResult<()> {
        // Write file registrations
        for registration in &batch.file_registrations {
            self.index.store_file_registration(registration)?;
            stats.files_indexed += 1;
        }

        // Write symbols
        for (symbol, path) in &batch.symbols {
            self.index.index_symbol(symbol, &path.to_string_lossy())?;
            stats.symbols_found += 1;
        }

        // Write imports
        for import in &batch.imports {
            self.index.store_import(import)?;
        }

        Ok(())
    }

    /// Commit current batch and start a new one.
    fn commit_and_restart(&self) -> PipelineResult<()> {
        self.index.commit_batch()?;
        self.index.start_batch()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolKind;
    use crate::config::Settings;
    use crate::indexing::pipeline::types::FileRegistration;
    use crate::parsing::LanguageId;
    use crate::symbol::Symbol;
    use crate::types::{FileId, Range, SymbolId};
    use crossbeam_channel::bounded;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_test_symbol(id: u32, name: &str, file_id: u32) -> Symbol {
        Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(file_id).unwrap(),
            Range::new(1, 0, 1, 10),
        )
    }

    fn make_test_batch(file_id: u32, symbol_count: usize) -> IndexBatch {
        let mut batch = IndexBatch::new();

        batch.file_registrations.push(FileRegistration {
            path: PathBuf::from(format!("test_{file_id}.rs")),
            file_id: FileId::new(file_id).unwrap(),
            content_hash: 12345,
            language_id: LanguageId::new("rust"),
            timestamp: 1700000000,
        });

        for i in 0..symbol_count {
            let sym_id = (file_id - 1) * symbol_count as u32 + i as u32 + 1;
            let symbol = make_test_symbol(sym_id, &format!("sym_{sym_id}"), file_id);
            batch
                .symbols
                .push((symbol, PathBuf::from(format!("test_{file_id}.rs"))));
        }

        batch
    }

    #[test]
    fn test_index_stage_writes_symbols() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);

        // Send a batch with symbols
        let batch = make_test_batch(1, 3);
        batch_tx.send(batch).unwrap();
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 10);
        let result = stage.run(batch_rx);

        assert!(result.is_ok());
        let (stats, rels) = result.unwrap();

        println!(
            "Indexed {} files, {} symbols",
            stats.files_indexed, stats.symbols_found
        );

        assert_eq!(stats.files_indexed, 1);
        assert_eq!(stats.symbols_found, 3);
        assert!(rels.is_empty());
    }

    #[test]
    fn test_index_stage_commits_every_n_batches() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(20);

        // Send 5 batches
        for i in 1..=5 {
            batch_tx.send(make_test_batch(i, 2)).unwrap();
        }
        drop(batch_tx);

        // Commit every 2 batches
        let stage = IndexStage::new(Arc::clone(&index), 2);
        let result = stage.run(batch_rx);

        assert!(result.is_ok());
        let (stats, _) = result.unwrap();

        println!(
            "Indexed {} files, {} symbols with batches_per_commit=2",
            stats.files_indexed, stats.symbols_found
        );

        assert_eq!(stats.files_indexed, 5);
        assert_eq!(stats.symbols_found, 10);
    }

    #[test]
    fn test_index_stage_accumulates_relationships() {
        use crate::RelationKind;
        use std::sync::Arc as StdArc;

        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);

        // Create batch with relationships
        let mut batch = make_test_batch(1, 2);
        batch.unresolved_relationships.push(UnresolvedRelationship {
            from_id: Some(SymbolId::new(1).unwrap()),
            from_name: StdArc::from("caller"),
            to_name: StdArc::from("callee"),
            file_id: FileId::new(1).unwrap(),
            kind: RelationKind::Calls,
            metadata: None,
            to_range: None,
        });

        batch_tx.send(batch).unwrap();
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 10);
        let result = stage.run(batch_rx);

        assert!(result.is_ok());
        let (stats, rels) = result.unwrap();

        println!("Accumulated {} relationships for Phase 2", rels.len());

        assert_eq!(stats.files_indexed, 1);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].from_name.as_ref(), "caller");
        assert_eq!(rels[0].to_name.as_ref(), "callee");
    }
}
