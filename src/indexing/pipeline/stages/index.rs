//! Index stage - Tantivy batch writes
//!
//! [PIPELINE API] Part of the new parallel indexing pipeline.
//!
//! Single-threaded stage that:
//! - Receives IndexBatch from COLLECT stage
//! - Writes symbols, imports, file registrations to Tantivy
//! - Generates embeddings for symbols with doc_comments
//! - Accumulates UnresolvedRelationships for Phase 2
//! - Builds SymbolLookupCache for O(1) Phase 2 resolution
//! - Commits every N batches for efficient I/O

use crate::indexing::IndexStats;
use crate::indexing::pipeline::types::{
    IndexBatch, PipelineError, PipelineResult, SymbolLookupCache, UnresolvedRelationship,
};
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use crate::types::FileId;
use crossbeam_channel::Receiver;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Index stage for Tantivy writes.
///
/// [PIPELINE API] Receives batches from COLLECT and writes to Tantivy efficiently.
/// Commits are batched to reduce fsync overhead.
///
/// Error handling uses proper #[from] conversion - StorageError -> PipelineError.
pub struct IndexStage {
    index: Arc<DocumentIndex>,
    batches_per_commit: usize,
    /// Optional semantic search for embedding generation.
    semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
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
            semantic: None,
        }
    }

    /// Add semantic search for embedding generation.
    pub fn with_semantic(mut self, semantic: Arc<Mutex<SimpleSemanticSearch>>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Run the index stage.
    ///
    /// Returns (stats, accumulated_relationships, symbol_cache) for Phase 2.
    /// The symbol cache enables O(1) lookups during resolution instead of Tantivy queries.
    pub fn run(
        &self,
        receiver: Receiver<IndexBatch>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>, SymbolLookupCache)> {
        let mut stats = IndexStats::new();
        let mut pending_relationships: Vec<UnresolvedRelationship> = Vec::new();
        let mut batch_count = 0;

        // Pre-allocate cache based on expected symbols (will grow if needed)
        let symbol_cache = SymbolLookupCache::with_capacity(10_000);

        // Start initial batch - StorageError converts to PipelineError via #[from]
        self.index.start_batch()?;

        for batch in receiver {
            self.process_batch(&batch, &mut stats, &symbol_cache)?;

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

        Ok((stats, pending_relationships, symbol_cache))
    }

    /// Process a single batch.
    ///
    /// Writes to Tantivy, generates embeddings, and accumulates symbols in cache.
    fn process_batch(
        &self,
        batch: &IndexBatch,
        stats: &mut IndexStats,
        symbol_cache: &SymbolLookupCache,
    ) -> PipelineResult<()> {
        // Build file_id -> language_id mapping from registrations
        let file_languages: HashMap<FileId, &str> = batch
            .file_registrations
            .iter()
            .map(|r| (r.file_id, r.language_id.as_str()))
            .collect();

        // Write file registrations
        for registration in &batch.file_registrations {
            self.index.store_file_registration(registration)?;
            stats.files_indexed += 1;
        }

        // Write symbols, generate embeddings, and accumulate in cache
        for (symbol, path) in &batch.symbols {
            self.index.index_symbol(symbol, &path.to_string_lossy())?;

            // Generate embedding if semantic search is enabled and symbol has doc_comment
            if let (Some(semantic), Some(doc)) = (&self.semantic, &symbol.doc_comment) {
                // Get language for this symbol's file
                let language = file_languages
                    .get(&symbol.file_id)
                    .copied()
                    .unwrap_or("unknown");

                let mut semantic_guard = semantic.lock().map_err(|_| PipelineError::Parse {
                    path: PathBuf::new(),
                    reason: "Failed to lock semantic search".to_string(),
                })?;

                // Ignore embedding errors to avoid blocking indexing
                if let Err(e) =
                    semantic_guard.index_doc_comment_with_language(symbol.id, doc, language)
                {
                    tracing::warn!(target: "pipeline", "Failed to generate embedding for {}: {e}", symbol.name);
                }
            }

            // Insert into cache for O(1) Phase 2 resolution
            symbol_cache.insert(symbol.clone());
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

    /// Index a single batch (for single-file indexing).
    ///
    /// [PIPELINE API] Used by `Pipeline::index_file_single()` for watcher reindex.
    /// Caller must handle start_batch/commit_batch.
    pub fn index_batch(&self, batch: IndexBatch) -> PipelineResult<()> {
        let symbol_cache = SymbolLookupCache::new();
        let mut stats = IndexStats::new();
        self.process_batch(&batch, &mut stats, &symbol_cache)
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
            content_hash: "abc123def456".to_string(),
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
        let (stats, rels, symbol_cache) = result.unwrap();

        println!(
            "Indexed {} files, {} symbols, cache has {} entries",
            stats.files_indexed,
            stats.symbols_found,
            symbol_cache.len()
        );

        assert_eq!(stats.files_indexed, 1);
        assert_eq!(stats.symbols_found, 3);
        assert!(rels.is_empty());
        assert_eq!(symbol_cache.len(), 3);
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
        let (stats, _, symbol_cache) = result.unwrap();

        println!(
            "Indexed {} files, {} symbols with batches_per_commit=2, cache has {} entries",
            stats.files_indexed,
            stats.symbols_found,
            symbol_cache.len()
        );

        assert_eq!(stats.files_indexed, 5);
        assert_eq!(stats.symbols_found, 10);
        assert_eq!(symbol_cache.len(), 10);
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
        let (stats, rels, symbol_cache) = result.unwrap();

        println!(
            "Accumulated {} relationships for Phase 2, cache has {} symbols",
            rels.len(),
            symbol_cache.len()
        );

        assert_eq!(stats.files_indexed, 1);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].from_name.as_ref(), "caller");
        assert_eq!(rels[0].to_name.as_ref(), "callee");
        assert_eq!(symbol_cache.len(), 2);
    }

    #[test]
    fn test_symbol_cache_lookup_by_name() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());

        let (batch_tx, batch_rx) = bounded(10);

        // Create batch with specific symbol names
        let mut batch = IndexBatch::new();
        batch.file_registrations.push(FileRegistration {
            path: PathBuf::from("test.rs"),
            file_id: FileId::new(1).unwrap(),
            content_hash: "abc123def456".to_string(),
            language_id: LanguageId::new("rust"),
            timestamp: 1700000000,
        });

        // Add symbols with known names
        let sym1 = make_test_symbol(1, "process_data", 1);
        let sym2 = make_test_symbol(2, "process_data", 1); // Duplicate name, different ID
        let sym3 = make_test_symbol(3, "validate_input", 1);

        batch.symbols.push((sym1, PathBuf::from("test.rs")));
        batch.symbols.push((sym2, PathBuf::from("test.rs")));
        batch.symbols.push((sym3, PathBuf::from("test.rs")));

        batch_tx.send(batch).unwrap();
        drop(batch_tx);

        let stage = IndexStage::new(Arc::clone(&index), 10);
        let (_, _, symbol_cache) = stage.run(batch_rx).unwrap();

        // Verify lookup by name returns correct candidates
        let candidates = symbol_cache.lookup_candidates("process_data");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.contains(&SymbolId::new(1).unwrap()));
        assert!(candidates.contains(&SymbolId::new(2).unwrap()));

        let validate_candidates = symbol_cache.lookup_candidates("validate_input");
        assert_eq!(validate_candidates.len(), 1);
        assert!(validate_candidates.contains(&SymbolId::new(3).unwrap()));

        // Non-existent name returns empty
        let missing = symbol_cache.lookup_candidates("nonexistent");
        assert!(missing.is_empty());

        // Verify direct ID lookup
        let sym = symbol_cache.get(SymbolId::new(1).unwrap());
        assert!(sym.is_some());
        assert_eq!(&*sym.unwrap().name, "process_data");

        println!(
            "Cache: {} symbols, {} unique names",
            symbol_cache.len(),
            symbol_cache.unique_names()
        );
    }
}
