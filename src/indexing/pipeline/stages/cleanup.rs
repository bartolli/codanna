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

/// Unique files owning inbound edges into `paths`, excluding `in_run`
/// members and files no longer on disk.
///
/// Relocation changes a target's path identity while the persisted edge
/// carries only resolved endpoints; replaying the pick that selected the
/// edge requires the source file itself, so its path re-enters the run.
/// Reads through the searcher: must run BEFORE any cleanup of `paths`
/// deletes the rows it walks.
pub(crate) fn inbound_caller_files(
    index: &DocumentIndex,
    paths: &[PathBuf],
    in_run: &std::collections::HashSet<PathBuf>,
) -> PipelineResult<Vec<PathBuf>> {
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut callers = Vec::new();
    for path in paths {
        let path_str = path.to_string_lossy();
        let Some((file_id, _hash, _mtime)) = index.get_file_info(&path_str)? else {
            continue;
        };
        for symbol in index.find_symbols_by_file(file_id)? {
            for kind in ALL_RELATION_KINDS {
                for (from, _to, _rel) in index.get_relationships_to(symbol.id, kind)? {
                    let Some(from_symbol) = index.find_symbol_by_id(from)? else {
                        continue;
                    };
                    let Some(from_path) = index.get_file_path(from_symbol.file_id)? else {
                        continue;
                    };
                    let from_path = PathBuf::from(from_path);
                    if in_run.contains(&from_path) || !seen.insert(from_path.clone()) {
                        continue;
                    }
                    // A caller recorded in the index but gone from disk is
                    // not reintroduced; its own change event owns it.
                    if !from_path.exists() {
                        continue;
                    }
                    callers.push(from_path);
                }
            }
        }
    }
    Ok(callers)
}

/// Containing type of a class member, when the parser recorded one.
///
/// This is the identity that tells `Alpha::make` from `Beta::make`. Symbols
/// that are not class members have no containing type and compare equal on
/// `None`, which is correct: their name and kind already identify them within
/// the file unless the file defines the same free function twice.
fn member_scope(symbol: &crate::Symbol) -> Option<String> {
    match symbol.scope_context {
        Some(crate::symbol::ScopeContext::ClassMember { ref class_name }) => {
            class_name.as_ref().map(|n| n.to_string())
        }
        _ => None,
    }
}

/// Symbols sharing `name`, `kind`, and containing scope with `target`, in
/// file order.
///
/// Position within this group is what tells apart symbols the scope cannot
/// split -- python `@overload` stubs are all module-level functions of the
/// same name in one file, so `Field` appears many times with no containing
/// type. Order survives any edit that shifts lines; it changes only when a
/// peer is added or removed, and the group-size check turns that into a
/// fail-closed.
fn peer_group<'a>(symbols: &'a [crate::Symbol], target: &crate::Symbol) -> Vec<&'a crate::Symbol> {
    let scope = member_scope(target);
    let mut peers: Vec<&crate::Symbol> = symbols
        .iter()
        .filter(|s| s.name == target.name && s.kind == target.kind && member_scope(s) == scope)
        .collect();
    peers.sort_by_key(|s| (s.range.start_line, s.range.start_column));
    peers
}

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
    /// Containing type of the target, when it is a class member. This is what
    /// tells two same-named members apart -- `Alpha::make` from `Beta::make`
    /// -- and unlike the line it survives any edit that shifts ranges.
    pub target_scope: Option<String>,
    /// Position of the target among its same-(name, kind, scope) peers in file
    /// order, with the peer count it was taken from. Discriminates symbols the
    /// scope cannot split; the count is what makes a changed peer set fail
    /// closed rather than rebind to the wrong peer.
    pub target_ordinal: usize,
    pub target_peer_count: usize,
    /// Start line as it stood before the re-index. Last-resort tie-break only;
    /// an edit above the symbol invalidates it.
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

    /// Clean up the OLD paths of relocated (renamed) files, capturing the
    /// edges that point into them with the target re-addressed to the NEW
    /// path.
    ///
    /// Pairing evidence is an exact content hash (discovery), so the
    /// replacement file's symbol layout is identical and the rebind
    /// discriminators match exactly. `co_reindexed` names the other files
    /// whose rows are also replaced this run: edges sourced there are
    /// excluded from capture -- their from-ids die with the old rows, and
    /// their own re-parse re-derives the edges.
    pub fn cleanup_files_for_relocation(
        &self,
        pairs: &[(PathBuf, PathBuf)],
        co_reindexed: &[PathBuf],
    ) -> PipelineResult<(CleanupStats, Vec<CapturedInboundEdge>)> {
        let mut in_flight: std::collections::HashSet<SymbolId> = std::collections::HashSet::new();
        for (old, _) in pairs {
            for symbol in self.symbols_of(old)? {
                in_flight.insert(symbol.id);
            }
        }
        for file in co_reindexed {
            for symbol in self.symbols_of(file)? {
                in_flight.insert(symbol.id);
            }
        }

        let mut captured = Vec::new();
        for (old, new) in pairs {
            let mut edges = self.capture_inbound_edges(old, &in_flight)?;
            for edge in &mut edges {
                edge.target_file = new.clone();
            }
            captured.extend(edges);
        }

        let old_paths: Vec<PathBuf> = pairs.iter().map(|(old, _)| old.clone()).collect();
        let (stats, _) = self.cleanup_files_inner(&old_paths, false)?;
        Ok((stats, captured))
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
            let peers = peer_group(&symbols, symbol);
            let ordinal = peers.iter().position(|s| s.id == symbol.id).unwrap_or(0);
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
                        target_scope: member_scope(symbol),
                        target_ordinal: ordinal,
                        target_peer_count: peers.len(),
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
            // Three discriminators, most durable first. Containing type
            // splits rust impl blocks and class overloads and survives any
            // range shift. Position among same-scope peers splits what the
            // scope cannot -- python `@overload` stubs share name, kind, and
            // module scope -- and survives shifts too, guarded by a peer-count
            // check so an added or removed sibling fails closed instead of
            // rebinding to its neighbour. The line is last and any edit above
            // the symbol invalidates it.
            let target: &crate::Symbol = if let [only] = candidates.as_slice() {
                only
            } else {
                let by_scope: Vec<&crate::Symbol> = candidates
                    .iter()
                    .copied()
                    .filter(|s| member_scope(s) == edge.target_scope)
                    .collect();
                let mut narrowed = if by_scope.is_empty() {
                    candidates.clone()
                } else {
                    by_scope
                };
                narrowed.sort_by_key(|s| (s.range.start_line, s.range.start_column));

                let picked = if narrowed.len() == 1 {
                    Some(narrowed[0])
                } else if narrowed.len() == edge.target_peer_count {
                    narrowed.get(edge.target_ordinal).copied()
                } else {
                    let mut exact = narrowed
                        .iter()
                        .copied()
                        .filter(|s| s.range.start_line == edge.target_line);
                    match (exact.next(), exact.next()) {
                        (Some(one), None) => Some(one),
                        _ => None,
                    }
                };
                match picked {
                    Some(one) => one,
                    None => {
                        stats.dropped += 1;
                        continue;
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
