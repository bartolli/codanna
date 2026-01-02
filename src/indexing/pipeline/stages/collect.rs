//! Collect stage - ID assignment and batching
//!
//! Single-threaded stage that:
//! - Assigns FileId and SymbolId sequentially
//! - Converts RawSymbol -> Symbol
//! - Converts RawImport -> Import
//! - Converts RawRelationship -> UnresolvedRelationship (resolving from_id)
//! - Batches output for efficient Tantivy writes

use crate::indexing::pipeline::types::{
    FileRegistration, IndexBatch, ParsedFile, PipelineResult, RawRelationship, RawSymbol,
    UnresolvedRelationship,
};
use crate::symbol::Symbol;
use crate::types::{FileId, Range, SymbolId};
use crate::utils::get_utc_timestamp;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Collect stage for ID assignment and batching.
pub struct CollectStage {
    batch_size: usize,
}

/// Ephemeral caches for relationship reconnection.
///
/// Design from decisions.md:
/// - `symbol_lookup`: (name, file_id, range) -> SymbolId for disambiguation
/// - `name_in_file`: Fallback when range doesn't match exactly
/// - `file_ids`: PathBuf -> FileId for Phase 2 cross-file resolution
///
/// Uses Arc<str> for zero-copy name sharing.
struct CollectorCaches {
    /// Primary lookup: (name, file_id, range) -> SymbolId
    symbol_lookup: HashMap<(Arc<str>, FileId, Range), SymbolId>,
    /// Fallback: (name, file_id) -> SymbolId for when range doesn't match exactly
    name_in_file: HashMap<(Arc<str>, FileId), SymbolId>,
    /// Path to FileId mapping for Phase 2 resolution
    file_ids: HashMap<PathBuf, FileId>,
}

impl CollectorCaches {
    fn new() -> Self {
        Self {
            symbol_lookup: HashMap::new(),
            name_in_file: HashMap::new(),
            file_ids: HashMap::new(),
        }
    }

    /// Register a file path -> FileId mapping for Phase 2 resolution.
    fn insert_file(&mut self, path: PathBuf, file_id: FileId) {
        self.file_ids.insert(path, file_id);
    }

    /// Lookup FileId by path (for Phase 2 cross-file resolution).
    #[allow(dead_code)] // Used in Phase 2
    fn lookup_file(&self, path: &PathBuf) -> Option<FileId> {
        self.file_ids.get(path).copied()
    }

    /// Insert a symbol into the cache. Uses Arc::clone for zero-copy.
    fn insert(&mut self, name: Arc<str>, file_id: FileId, range: Range, symbol_id: SymbolId) {
        self.symbol_lookup
            .insert((Arc::clone(&name), file_id, range), symbol_id);
        self.name_in_file.insert((name, file_id), symbol_id);
    }

    /// Lookup by exact (name, file_id, range) match.
    fn lookup(&self, name: &Arc<str>, file_id: FileId, range: Range) -> Option<SymbolId> {
        self.symbol_lookup
            .get(&(Arc::clone(name), file_id, range))
            .copied()
    }

    /// Fallback lookup by (name, file_id) when range doesn't match.
    fn lookup_by_name_in_file(&self, name: &Arc<str>, file_id: FileId) -> Option<SymbolId> {
        self.name_in_file.get(&(Arc::clone(name), file_id)).copied()
    }
}

/// State for the collector.
struct CollectorState {
    file_counter: u32,
    symbol_counter: u32,
    caches: CollectorCaches,
    current_batch: IndexBatch,
    batch_size: usize,
}

impl CollectorState {
    fn new(batch_size: usize) -> Self {
        Self {
            file_counter: 0,
            symbol_counter: 0,
            caches: CollectorCaches::new(),
            current_batch: IndexBatch::new(),
            batch_size,
        }
    }

    fn next_file_id(&mut self) -> FileId {
        self.file_counter += 1;
        FileId::new(self.file_counter).expect("FileId overflow")
    }

    fn next_symbol_id(&mut self) -> SymbolId {
        self.symbol_counter += 1;
        SymbolId::new(self.symbol_counter).expect("SymbolId overflow")
    }

    fn should_flush(&self) -> bool {
        self.current_batch.symbol_count() >= self.batch_size
    }

    fn take_batch(&mut self) -> IndexBatch {
        std::mem::take(&mut self.current_batch)
    }
}

impl CollectStage {
    /// Create a new collect stage.
    /// Minimum batch size is 1 (for testing). Production default is 5000.
    pub fn new(batch_size: usize) -> Self {
        Self {
            batch_size: batch_size.max(1),
        }
    }

    /// Create with default batch size (5000 symbols).
    pub fn default_batch_size() -> Self {
        Self::new(5000)
    }

    /// Process a single parsed file (for single-file indexing).
    ///
    /// [PIPELINE API] Used by `Pipeline::index_file_single()` for watcher reindex.
    /// Assigns FileId and SymbolIds using the next available IDs from DocumentIndex.
    ///
    /// Returns (IndexBatch, `Vec<UnresolvedRelationship>`) for indexing.
    pub fn process_single(
        &self,
        parsed: ParsedFile,
        index: Arc<crate::storage::DocumentIndex>,
    ) -> PipelineResult<(IndexBatch, Vec<UnresolvedRelationship>)> {
        // Get next available IDs from the index
        let next_file_id = index.get_next_file_id()?;
        let next_symbol_id = index.get_next_symbol_id()?;

        let mut state = CollectorState::new(self.batch_size);
        // Set counters to continue from existing index
        state.file_counter = next_file_id.saturating_sub(1);
        state.symbol_counter = next_symbol_id.saturating_sub(1);

        // Process the file
        self.process_file(&mut state, parsed);

        // Extract relationships from batch
        let unresolved = std::mem::take(&mut state.current_batch.unresolved_relationships);

        Ok((state.current_batch, unresolved))
    }

    /// Run the collect stage.
    ///
    /// Returns (total_files, total_symbols).
    pub fn run(
        &self,
        receiver: Receiver<ParsedFile>,
        sender: Sender<IndexBatch>,
    ) -> PipelineResult<(usize, usize)> {
        let mut state = CollectorState::new(self.batch_size);

        for parsed in receiver {
            self.process_file(&mut state, parsed);

            // Flush batch if full
            if state.should_flush() {
                let batch = state.take_batch();
                if sender.send(batch).is_err() {
                    break; // Channel closed
                }
            }
        }

        // Flush remaining batch
        if !state.current_batch.is_empty() {
            let _ = sender.send(state.take_batch());
        }

        Ok((state.file_counter as usize, state.symbol_counter as usize))
    }

    /// Process a single parsed file.
    fn process_file(&self, state: &mut CollectorState, parsed: ParsedFile) {
        let file_id = state.next_file_id();
        let file_path: Box<str> = parsed.path.to_string_lossy().into();

        // Cache path -> FileId for Phase 2 resolution (per decisions.md)
        state.caches.insert_file(parsed.path.clone(), file_id);

        // Register file
        state
            .current_batch
            .file_registrations
            .push(FileRegistration {
                path: parsed.path.clone(),
                file_id,
                content_hash: parsed.content_hash,
                language_id: parsed.language_id,
                timestamp: get_utc_timestamp(),
            });

        // Process symbols
        for raw_sym in parsed.raw_symbols {
            let symbol_id = state.next_symbol_id();

            // Cache for relationship resolution
            let name: Arc<str> = raw_sym.name.as_ref().into();
            state
                .caches
                .insert(name.clone(), file_id, raw_sym.range, symbol_id);

            // Create Symbol
            let symbol = create_symbol(
                symbol_id,
                &raw_sym,
                file_id,
                file_path.clone(),
                parsed.module_path.as_deref(),
                parsed.language_id,
            );

            state
                .current_batch
                .symbols
                .push((symbol, parsed.path.clone()));
        }

        // Process imports
        for raw_import in parsed.raw_imports {
            let import = raw_import.into_import(file_id);
            state.current_batch.imports.push(import);
        }

        // Process relationships
        for raw_rel in parsed.raw_relationships {
            let unresolved = create_unresolved_relationship(&state.caches, raw_rel, file_id);
            state
                .current_batch
                .unresolved_relationships
                .push(unresolved);
        }
    }
}

/// Create a Symbol from RawSymbol.
fn create_symbol(
    id: SymbolId,
    raw: &RawSymbol,
    file_id: FileId,
    file_path: Box<str>,
    module_path: Option<&str>,
    language_id: crate::parsing::LanguageId,
) -> Symbol {
    let mut symbol = Symbol::new(id, raw.name.clone(), raw.kind, file_id, raw.range)
        .with_file_path(file_path)
        .with_visibility(raw.visibility)
        .with_language_id(language_id);

    if let Some(sig) = &raw.signature {
        symbol = symbol.with_signature(sig.clone());
    }
    if let Some(doc) = &raw.doc_comment {
        symbol = symbol.with_doc(doc.clone());
    }
    if let Some(path) = module_path {
        symbol = symbol.with_module_path(path);
    }
    if let Some(scope) = raw.scope_context.clone() {
        symbol = symbol.with_scope(scope);
    }

    symbol
}

/// Create an UnresolvedRelationship from RawRelationship.
/// Uses Arc references for zero-copy lookup.
fn create_unresolved_relationship(
    caches: &CollectorCaches,
    raw: RawRelationship,
    file_id: FileId,
) -> UnresolvedRelationship {
    // Try to resolve from_id using the cache (zero-copy: pass Arc reference)
    let from_id = caches
        .lookup(&raw.from_name, file_id, raw.from_range)
        .or_else(|| caches.lookup_by_name_in_file(&raw.from_name, file_id));

    UnresolvedRelationship {
        from_id,
        from_name: raw.from_name,
        to_name: raw.to_name,
        file_id,
        kind: raw.kind,
        metadata: raw.metadata,
        to_range: Some(raw.to_range),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::pipeline::types::RawImport;
    use crate::parsing::LanguageId;
    use crate::{RelationKind, SymbolKind};
    use crossbeam_channel::bounded;

    fn make_raw_symbol(name: &str, kind: SymbolKind, line: u32) -> RawSymbol {
        RawSymbol::new(name, kind, Range::new(line, 0, line, 10))
    }

    fn make_parsed_file(name: &str, symbols: Vec<RawSymbol>) -> ParsedFile {
        ParsedFile {
            path: PathBuf::from(name),
            content_hash: "abc123def456".to_string(),
            language_id: LanguageId::new("rust"),
            module_path: None,
            raw_symbols: symbols,
            raw_imports: Vec::new(),
            raw_relationships: Vec::new(),
        }
    }

    #[test]
    fn test_collect_assigns_sequential_ids() {
        let (parsed_tx, parsed_rx) = bounded(100);
        let (batch_tx, batch_rx) = bounded(100);

        // Send 2 files with 3 symbols total
        parsed_tx
            .send(make_parsed_file(
                "file1.rs",
                vec![
                    make_raw_symbol("foo", SymbolKind::Function, 1),
                    make_raw_symbol("bar", SymbolKind::Function, 2),
                ],
            ))
            .unwrap();
        parsed_tx
            .send(make_parsed_file(
                "file2.rs",
                vec![make_raw_symbol("baz", SymbolKind::Struct, 1)],
            ))
            .unwrap();
        drop(parsed_tx);

        let stage = CollectStage::new(100);
        let result = stage.run(parsed_rx, batch_tx);

        assert!(result.is_ok());
        let (files, symbols) = result.unwrap();

        let batches: Vec<_> = batch_rx.iter().collect();

        println!("Processed {files} files, {symbols} symbols");
        for batch in &batches {
            for (sym, path) in &batch.symbols {
                println!(
                    "  - {} (id={}, file_id={}) in {}",
                    sym.name,
                    sym.id.value(),
                    sym.file_id.value(),
                    path.display()
                );
            }
        }

        assert_eq!(files, 2, "Should process 2 files");
        assert_eq!(symbols, 3, "Should process 3 symbols");

        // Collect all symbol IDs
        let all_ids: Vec<u32> = batches
            .iter()
            .flat_map(|b| b.symbols.iter().map(|(s, _)| s.id.value()))
            .collect();

        assert_eq!(all_ids, vec![1, 2, 3], "IDs should be sequential 1, 2, 3");
    }

    #[test]
    fn test_collect_batches_by_symbol_count() {
        let (parsed_tx, parsed_rx) = bounded(100);
        let (batch_tx, batch_rx) = bounded(100);

        // Create files with many symbols to trigger batching
        for i in 0..5 {
            let symbols: Vec<_> = (0..10)
                .map(|j| make_raw_symbol(&format!("sym_{i}_{j}"), SymbolKind::Function, j as u32))
                .collect();
            parsed_tx
                .send(make_parsed_file(&format!("file{i}.rs"), symbols))
                .unwrap();
        }
        drop(parsed_tx);

        // Small batch size to force multiple batches
        let stage = CollectStage::new(15);
        let result = stage.run(parsed_rx, batch_tx);

        assert!(result.is_ok());
        let (files, symbols) = result.unwrap();

        let batches: Vec<_> = batch_rx.iter().collect();

        println!(
            "Processed {files} files, {symbols} symbols in {} batches",
            batches.len()
        );
        for (i, batch) in batches.iter().enumerate() {
            println!("  Batch {i}: {} symbols", batch.symbol_count());
        }

        assert_eq!(files, 5);
        assert_eq!(symbols, 50);
        assert!(batches.len() > 1, "Should create multiple batches");
    }

    #[test]
    fn test_collect_resolves_relationship_from_id() {
        let (parsed_tx, parsed_rx) = bounded(100);
        let (batch_tx, batch_rx) = bounded(100);

        // Create file with symbols and relationships
        let mut parsed = make_parsed_file(
            "test.rs",
            vec![
                make_raw_symbol("caller", SymbolKind::Function, 1),
                make_raw_symbol("callee", SymbolKind::Function, 5),
            ],
        );

        // Add relationship: caller -> callee
        parsed.raw_relationships.push(RawRelationship::new(
            "caller",
            Range::new(1, 0, 1, 10), // from_range = caller's definition
            "callee",
            Range::new(2, 4, 2, 12), // to_range = call site
            RelationKind::Calls,
        ));

        parsed_tx.send(parsed).unwrap();
        drop(parsed_tx);

        let stage = CollectStage::new(100);
        let _ = stage.run(parsed_rx, batch_tx);

        let batches: Vec<_> = batch_rx.iter().collect();

        println!("Relationships:");
        for batch in &batches {
            for rel in &batch.unresolved_relationships {
                println!(
                    "  {} (from_id={:?}) -> {} ({:?})",
                    rel.from_name, rel.from_id, rel.to_name, rel.kind
                );
            }
        }

        // Check that from_id was resolved
        let rel = &batches[0].unresolved_relationships[0];
        assert!(rel.from_id.is_some(), "from_id should be resolved");
        assert_eq!(rel.from_id.unwrap().value(), 1, "caller should have id=1");
        assert_eq!(rel.from_name.as_ref(), "caller");
        assert_eq!(rel.to_name.as_ref(), "callee");
    }

    #[test]
    fn test_collect_converts_imports() {
        let (parsed_tx, parsed_rx) = bounded(100);
        let (batch_tx, batch_rx) = bounded(100);

        let mut parsed = make_parsed_file("test.rs", vec![]);
        parsed
            .raw_imports
            .push(RawImport::new("std::collections::HashMap"));
        parsed
            .raw_imports
            .push(RawImport::new("crate::module::Thing").with_alias("Thing".to_string()));

        parsed_tx.send(parsed).unwrap();
        drop(parsed_tx);

        let stage = CollectStage::new(100);
        let _ = stage.run(parsed_rx, batch_tx);

        let batches: Vec<_> = batch_rx.iter().collect();

        println!("Imports:");
        for batch in &batches {
            for imp in &batch.imports {
                println!(
                    "  {} (file_id={}, alias={:?})",
                    imp.path,
                    imp.file_id.value(),
                    imp.alias
                );
            }
        }

        assert_eq!(batches[0].imports.len(), 2);

        let imp1 = &batches[0].imports[0];
        assert_eq!(imp1.path, "std::collections::HashMap");
        assert_eq!(imp1.file_id.value(), 1);

        let imp2 = &batches[0].imports[1];
        assert_eq!(imp2.alias.as_deref(), Some("Thing"));
    }

    #[test]
    fn test_collect_tracks_file_registrations() {
        let (parsed_tx, parsed_rx) = bounded(100);
        let (batch_tx, batch_rx) = bounded(100);

        // Send 3 files
        parsed_tx
            .send(make_parsed_file(
                "src/main.rs",
                vec![make_raw_symbol("main", SymbolKind::Function, 1)],
            ))
            .unwrap();
        parsed_tx
            .send(make_parsed_file(
                "src/lib.rs",
                vec![make_raw_symbol("lib_fn", SymbolKind::Function, 1)],
            ))
            .unwrap();
        parsed_tx
            .send(make_parsed_file(
                "src/utils.rs",
                vec![make_raw_symbol("helper", SymbolKind::Function, 1)],
            ))
            .unwrap();
        drop(parsed_tx);

        let stage = CollectStage::new(100);
        let _ = stage.run(parsed_rx, batch_tx);

        let batches: Vec<_> = batch_rx.iter().collect();
        let registrations = &batches[0].file_registrations;

        println!("File registrations:");
        for reg in registrations {
            println!(
                "  {} -> file_id={}, hash={}, timestamp={}",
                reg.path.display(),
                reg.file_id.value(),
                reg.content_hash,
                reg.timestamp
            );
        }

        assert_eq!(registrations.len(), 3, "Should register 3 files");

        // Verify sequential FileIds
        assert_eq!(registrations[0].file_id.value(), 1);
        assert_eq!(registrations[0].path, PathBuf::from("src/main.rs"));

        assert_eq!(registrations[1].file_id.value(), 2);
        assert_eq!(registrations[1].path, PathBuf::from("src/lib.rs"));

        assert_eq!(registrations[2].file_id.value(), 3);
        assert_eq!(registrations[2].path, PathBuf::from("src/utils.rs"));

        // Verify timestamps are set (after 2020-01-01)
        for reg in registrations {
            assert!(reg.timestamp > 1577836800, "Timestamp should be after 2020");
        }
    }

    #[test]
    fn test_collect_preserves_doc_comment() {
        let (parsed_tx, parsed_rx) = bounded(100);
        let (batch_tx, batch_rx) = bounded(100);

        // Create symbols with doc_comments
        let sym_with_doc = RawSymbol::new(
            "documented_fn",
            SymbolKind::Function,
            Range::new(1, 0, 5, 1),
        )
        .with_signature("fn documented_fn() -> Result<()>")
        .with_doc_comment("This function does important work.\n\nIt handles the core logic.");

        let sym_without_doc =
            RawSymbol::new("plain_fn", SymbolKind::Function, Range::new(10, 0, 12, 1))
                .with_signature("fn plain_fn()");

        let sym_with_short_doc =
            RawSymbol::new("helper", SymbolKind::Function, Range::new(20, 0, 22, 1))
                .with_doc_comment("Helper utility");

        let parsed = ParsedFile {
            path: PathBuf::from("src/lib.rs"),
            content_hash: "abc123def456".to_string(),
            language_id: LanguageId::new("rust"),
            module_path: Some("mylib".to_string()),
            raw_symbols: vec![sym_with_doc, sym_without_doc, sym_with_short_doc],
            raw_imports: Vec::new(),
            raw_relationships: Vec::new(),
        };

        parsed_tx.send(parsed).unwrap();
        drop(parsed_tx);

        let stage = CollectStage::new(100);
        let result = stage.run(parsed_rx, batch_tx);
        assert!(result.is_ok());

        let batches: Vec<_> = batch_rx.iter().collect();
        assert_eq!(batches.len(), 1);

        let symbols = &batches[0].symbols;
        assert_eq!(symbols.len(), 3);

        // Verify doc_comment is preserved on Symbol
        let (sym1, _) = &symbols[0];
        assert_eq!(sym1.name.as_ref(), "documented_fn");
        assert!(
            sym1.doc_comment.is_some(),
            "doc_comment should be preserved"
        );
        assert_eq!(
            sym1.doc_comment.as_deref(),
            Some("This function does important work.\n\nIt handles the core logic.")
        );

        let (sym2, _) = &symbols[1];
        assert_eq!(sym2.name.as_ref(), "plain_fn");
        assert!(
            sym2.doc_comment.is_none(),
            "Symbol without doc should have None"
        );

        let (sym3, _) = &symbols[2];
        assert_eq!(sym3.name.as_ref(), "helper");
        assert_eq!(sym3.doc_comment.as_deref(), Some("Helper utility"));

        println!("doc_comment preservation verified:");
        for (sym, path) in symbols {
            println!(
                "  {} (id={}) in {} doc={:?}",
                sym.name,
                sym.id.value(),
                path.display(),
                sym.doc_comment.as_ref().map(|d| if d.len() > 30 {
                    format!("{}...", &d[..30])
                } else {
                    d.to_string()
                })
            );
        }
    }
}
