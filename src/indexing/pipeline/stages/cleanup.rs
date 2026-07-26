//! Cleanup stage - removes symbols and embeddings for files
//!
//! This stage handles cleanup for:
//! - Deleted files: Files that existed in the index but no longer exist on disk
//! - Modified files: Files that will be re-indexed (old data must be removed first)
//!
//! The cleanup order is critical for embedding sync:
//! 1. Get symbols for file
//! 2. Remove embeddings for those symbols
//! 3. Save embeddings to disk (prevents desync on crash)
//! 4. Remove file documents from Tantivy

use crate::indexing::pipeline::types::{PipelineError, PipelineResult};
use crate::relationship::{RelationKind, Relationship};
use crate::semantic::SimpleSemanticSearch;
use crate::storage::DocumentIndex;
use crate::types::{SymbolId, SymbolKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Every relation kind that can point at a symbol. Rebind must consider all
/// of them: a kind omitted here is a kind that silently keeps dying on
/// re-index.
const ALL_RELATION_KINDS: [RelationKind; 12] = [
    RelationKind::Calls,
    RelationKind::CalledBy,
    RelationKind::Extends,
    RelationKind::ExtendedBy,
    RelationKind::Implements,
    RelationKind::ImplementedBy,
    RelationKind::Uses,
    RelationKind::UsedBy,
    RelationKind::Defines,
    RelationKind::DefinedIn,
    RelationKind::References,
    RelationKind::ReferencedBy,
];

/// Statistics from cleanup operations.
#[derive(Debug, Default, Clone)]
pub struct CleanupStats {
    /// Number of files cleaned up.
    pub files_cleaned: usize,
    /// Number of symbols removed.
    pub symbols_removed: usize,
    /// Number of embeddings removed.
    pub embeddings_removed: usize,
}

/// An edge that pointed INTO a file being re-indexed, captured before the
/// file's rows are deleted so it can be rebound to the replacement symbol.
///
/// `symbol_id` is session-scoped and not stable across reindexes, so the
/// target is carried by name and kind. The line rides along as a tie-break
/// only -- keying on it would fail to rebind whenever an edit shifts ranges.
#[derive(Debug, Clone)]
pub struct CapturedInboundEdge {
    pub from: SymbolId,
    /// File the target lives in, as keyed in the index. Carried so one
    /// rebind call can serve a whole batch of re-indexed files.
    pub target_file: PathBuf,
    pub target_name: String,
    pub target_kind: SymbolKind,
    /// Start line of the target as it stood before the re-index. Used ONLY to
    /// break a (name, kind) tie -- never as part of the primary key, because
    /// an edit above a symbol shifts its range.
    pub target_line: u32,
    pub relationship: Relationship,
}

/// Outcome of rebinding captured inbound edges after a re-index.
#[derive(Debug, Default, Clone)]
pub struct RebindStats {
    /// Edges re-pointed at the replacement symbol.
    pub rebound: usize,
    /// Edges whose target no longer exists, or whose name+kind match was
    /// ambiguous. Dropped on purpose -- see `rebind_inbound_edges`.
    pub dropped: usize,
}

/// Cleanup stage for removing old symbols and embeddings.
pub struct CleanupStage {
    index: Arc<DocumentIndex>,
    semantic: Option<Arc<Mutex<SimpleSemanticSearch>>>,
    semantic_path: PathBuf,
}

impl CleanupStage {
    /// Create a new cleanup stage.
    pub fn new(index: Arc<DocumentIndex>, semantic_path: impl Into<PathBuf>) -> Self {
        Self {
            index,
            semantic: None,
            semantic_path: semantic_path.into(),
        }
    }

    /// Add semantic search for embedding cleanup.
    pub fn with_semantic(mut self, semantic: Arc<Mutex<SimpleSemanticSearch>>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Clean up files before re-indexing or deletion.
    ///
    /// This removes:
    /// - All symbols associated with the files
    /// - All embeddings for those symbols
    /// - File registrations from the index
    ///
    /// After cleanup, embeddings are saved to disk immediately to prevent desync.
    pub fn cleanup_files(&self, files: &[PathBuf]) -> PipelineResult<CleanupStats> {
        self.cleanup_files_inner(files, false)
            .map(|(stats, _)| stats)
    }

    /// Clean up files that are about to be RE-INDEXED, capturing the edges
    /// that point into them.
    ///
    /// Deleting a file's rows also deletes every edge targeting its symbols
    /// -- correct in isolation, since the replacements get fresh ids, but the
    /// re-index only re-derives the file's OWN outgoing edges. Edges owned by
    /// unchanged files would be lost. The caller must pass the returned
    /// captures to `rebind_inbound_edges` once the replacements are committed.
    ///
    /// Use `cleanup_files` for genuine deletion: there the inbound edges
    /// SHOULD die with their target.
    pub fn cleanup_files_for_reindex(
        &self,
        files: &[PathBuf],
    ) -> PipelineResult<(CleanupStats, Vec<CapturedInboundEdge>)> {
        self.cleanup_files_inner(files, true)
    }

    fn cleanup_files_inner(
        &self,
        files: &[PathBuf],
        capture_inbound: bool,
    ) -> PipelineResult<(CleanupStats, Vec<CapturedInboundEdge>)> {
        let mut stats = CleanupStats::default();
        let mut captured: Vec<CapturedInboundEdge> = Vec::new();

        // Capture before the batch opens: this reads through the searcher,
        // which cannot see staged deletes anyway, and a read failure here
        // must not leave a batch dangling.
        if capture_inbound {
            // Symbols across the WHOLE change set, not just the file being
            // captured: a source file re-indexed in the same run gets fresh
            // ids too, so an edge captured from it would be rebound against a
            // dead from-id. Its own re-parse re-derives that edge anyway.
            let mut in_flight: std::collections::HashSet<SymbolId> =
                std::collections::HashSet::new();
            for file in files {
                for symbol in self.symbols_of(file)? {
                    in_flight.insert(symbol.id);
                }
            }
            for file in files {
                captured.extend(self.capture_inbound_edges(file, &in_flight)?);
            }
        }

        // Start batch for delete operations
        self.index.start_batch().map_err(|e| PipelineError::Parse {
            path: PathBuf::new(),
            reason: format!("Failed to start batch: {e}"),
        })?;

        // Embedding removal is deferred until after the Tantivy commit so a
        // rollback cannot leave in-memory semantic state ahead of the index.
        let mut pending_embedding_removals: Vec<SymbolId> = Vec::new();
        for file in files {
            match self.cleanup_single_file(file) {
                Ok((symbols_removed, symbol_ids)) => {
                    stats.files_cleaned += 1;
                    stats.symbols_removed += symbols_removed;
                    pending_embedding_removals.extend(symbol_ids);
                }
                Err(e) => {
                    // Discard staged deletes; leaving them in the shared
                    // writer lets a later commit drop symbols for files
                    // that were never reprocessed.
                    if let Err(rollback_err) = self.index.rollback_batch() {
                        tracing::warn!(
                            target: "pipeline",
                            "Rollback after cleanup failure also failed: {rollback_err}"
                        );
                    }
                    return Err(e);
                }
            }
        }

        // Commit batch after all deletions
        self.index
            .commit_batch()
            .map_err(|e| PipelineError::Parse {
                path: PathBuf::new(),
                reason: format!("Failed to commit batch: {e}"),
            })?;

        // Tantivy state is durable; now mutate and persist semantic state.
        if let Some(ref semantic) = self.semantic {
            let mut semantic_guard = semantic.lock().map_err(|_| PipelineError::Parse {
                path: PathBuf::new(),
                reason: "Failed to lock semantic search".to_string(),
            })?;

            semantic_guard.remove_embeddings(&pending_embedding_removals);
            stats.embeddings_removed = pending_embedding_removals.len();

            semantic_guard
                .save(&self.semantic_path)
                .map_err(|e| PipelineError::Parse {
                    path: self.semantic_path.clone(),
                    reason: format!("Failed to save embeddings: {e}"),
                })?;
        }

        Ok((stats, captured))
    }

    /// Symbols currently indexed for `path`; empty when the file is unknown.
    fn symbols_of(&self, path: &Path) -> PipelineResult<Vec<crate::Symbol>> {
        let path_str = path.to_string_lossy();
        match self.index.get_file_info(&path_str)? {
            Some((file_id, _hash, _mtime)) => Ok(self.index.find_symbols_by_file(file_id)?),
            None => Ok(Vec::new()),
        }
    }

    /// Edges pointing into `path` from symbols OUTSIDE the change set.
    ///
    /// `in_flight` holds every symbol of every file being re-indexed in this
    /// run. Edges sourced there are excluded: their own file is re-parsed, so
    /// the re-index re-derives them against the new ids. Capturing them
    /// instead would rebind a DEAD from-id and persist an orphan edge.
    fn capture_inbound_edges(
        &self,
        path: &Path,
        in_flight: &std::collections::HashSet<SymbolId>,
    ) -> PipelineResult<Vec<CapturedInboundEdge>> {
        let symbols = self.symbols_of(path)?;

        let mut captured = Vec::new();
        for symbol in &symbols {
            for kind in ALL_RELATION_KINDS {
                for (from, _to, relationship) in self.index.get_relationships_to(symbol.id, kind)? {
                    if in_flight.contains(&from) {
                        continue;
                    }
                    captured.push(CapturedInboundEdge {
                        from,
                        target_file: path.to_path_buf(),
                        target_name: symbol.name.to_string(),
                        target_kind: symbol.kind,
                        target_line: symbol.range.start_line,
                        relationship,
                    });
                }
            }
        }
        Ok(captured)
    }

    /// Re-point captured edges at the replacements now living in `file_id`.
    ///
    /// Every captured edge is either rebound or dropped -- none survives
    /// uninspected. That is what makes deleting them during cleanup safe: a
    /// symbol genuinely removed or renamed by the edit has no replacement, so
    /// its inbound edges stay dead rather than being resurrected against a
    /// stale target.
    ///
    /// The match key is (file, name, kind). Line is excluded: the common edit
    /// shifts ranges, and a line-exact key would fail to rebind on exactly
    /// those edits. An ambiguous match (same name AND kind more than once in
    /// the file) is dropped rather than guessed -- rebinding to the wrong
    /// overload would trade a recall gap for a wrong edge.
    pub fn rebind_inbound_edges(
        &self,
        captured: &[CapturedInboundEdge],
    ) -> PipelineResult<RebindStats> {
        let mut stats = RebindStats::default();
        if captured.is_empty() {
            return Ok(stats);
        }

        // One symbol read per re-indexed file, not per edge.
        let mut replacements_by_file: std::collections::HashMap<PathBuf, Vec<crate::Symbol>> =
            std::collections::HashMap::new();
        for edge in captured {
            if replacements_by_file.contains_key(&edge.target_file) {
                continue;
            }
            let path_str = edge.target_file.to_string_lossy();
            let symbols = match self.index.get_file_info(&path_str)? {
                Some((file_id, _, _)) => self.index.find_symbols_by_file(file_id)?,
                None => Vec::new(),
            };
            replacements_by_file.insert(edge.target_file.clone(), symbols);
        }

        self.index.start_batch().map_err(|e| PipelineError::Parse {
            path: PathBuf::new(),
            reason: format!("Failed to start batch: {e}"),
        })?;

        for edge in captured {
            let replacements = replacements_by_file
                .get(&edge.target_file)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let candidates: Vec<_> = replacements
                .iter()
                .filter(|s| s.name.as_ref() == edge.target_name && s.kind == edge.target_kind)
                .collect();
            let target = match candidates.as_slice() {
                [only] => *only,
                // A file routinely holds several symbols sharing name and
                // kind (rust impl blocks each defining `new`, overload sets).
                // The line breaks the tie when the edit did not move it;
                // codanna's own types/mod.rs alone carries 556 inbound edges
                // to three different `new`. Ranges that DID shift stay
                // ambiguous and fail closed.
                _ => {
                    let mut exact = candidates
                        .iter()
                        .filter(|s| s.range.start_line == edge.target_line);
                    match (exact.next(), exact.next()) {
                        (Some(one), None) => *one,
                        _ => {
                            stats.dropped += 1;
                            continue;
                        }
                    }
                }
            };

            if let Err(e) = self
                .index
                .store_relationship(edge.from, target.id, &edge.relationship)
            {
                if let Err(rollback_err) = self.index.rollback_batch() {
                    tracing::warn!(
                        target: "pipeline",
                        "Rollback after rebind failure also failed: {rollback_err}"
                    );
                }
                return Err(PipelineError::Parse {
                    path: PathBuf::new(),
                    reason: format!("Failed to rebind inbound edge: {e}"),
                });
            }
            stats.rebound += 1;
        }

        self.index
            .commit_batch()
            .map_err(|e| PipelineError::Parse {
                path: PathBuf::new(),
                reason: format!("Failed to commit rebind batch: {e}"),
            })?;

        if stats.dropped > 0 {
            tracing::info!(
                target: "pipeline",
                "Dropped {} inbound edge(s) with no unambiguous target after re-index",
                stats.dropped
            );
        }

        Ok(stats)
    }

    /// Clean up a single file's Tantivy documents.
    ///
    /// Returns (symbols_removed, symbol ids whose embeddings the caller
    /// removes after the batch commits).
    fn cleanup_single_file(&self, path: &Path) -> PipelineResult<(usize, Vec<SymbolId>)> {
        let path_str = path.to_string_lossy();

        // Step 1: Get file_id from path
        let file_info = self.index.get_file_info(&path_str)?;
        let Some((file_id, _hash, _mtime)) = file_info else {
            // File not in index, nothing to clean
            return Ok((0, Vec::new()));
        };

        // Step 2: Get all symbols for this file
        let symbols = self.index.find_symbols_by_file(file_id)?;
        let symbol_ids: Vec<SymbolId> = symbols.iter().map(|s| s.id).collect();
        let symbol_count = symbol_ids.len();

        // Step 3: Remove relationships (both outgoing and incoming)
        // This garbage-collects orphaned refs when a symbol is renamed/deleted
        for symbol_id in &symbol_ids {
            self.index.delete_relationships_for_symbol(*symbol_id)?;
        }

        // Step 4: Remove file documents from Tantivy
        self.index.remove_file_documents(&path_str)?;

        Ok((symbol_count, symbol_ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use tempfile::TempDir;

    #[test]
    fn test_cleanup_stage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let semantic_path = temp_dir.path().join("semantic");

        let stage = CleanupStage::new(index, semantic_path);

        // Cleanup empty list should succeed
        let result = stage.cleanup_files(&[]);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.files_cleaned, 0);
        assert_eq!(stats.symbols_removed, 0);
    }

    #[test]
    fn test_cleanup_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let semantic_path = temp_dir.path().join("semantic");

        let stage = CleanupStage::new(index, semantic_path);

        // Cleanup file not in index should succeed (no-op)
        let result = stage.cleanup_files(&[PathBuf::from("nonexistent.rs")]);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.files_cleaned, 1);
        assert_eq!(stats.symbols_removed, 0);
    }
}
