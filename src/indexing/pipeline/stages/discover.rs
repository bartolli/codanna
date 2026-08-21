//! Discover stage - parallel file system walk
//!
//! Uses the `ignore` crate's parallel walker for high-performance
//! file discovery. Filters by supported extensions.
//!
//! Supports two modes:
//! - Full: Discovers all files (for initial indexing or force re-index)
//! - Incremental: Compares disk state to index, returns new/modified/deleted

use crate::indexing::file_info::calculate_hash;
use crate::indexing::pipeline::types::{DiscoverResult, PipelineError, PipelineResult};
use crate::parsing::get_registry;
use crate::storage::DocumentIndex;
use crossbeam_channel::Sender;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Discover stage for parallel file walking.
pub struct DiscoverStage {
    root: PathBuf,
    threads: usize,
    /// Optional index for incremental mode.
    index: Option<Arc<DocumentIndex>>,
    /// Workspace root for path normalization.
    workspace_root: Option<PathBuf>,
}

impl DiscoverStage {
    /// Create a new discover stage.
    pub fn new(root: impl Into<PathBuf>, threads: usize) -> Self {
        Self {
            root: root.into(),
            threads: threads.max(1),
            index: None,
            workspace_root: None,
        }
    }

    /// Add an index for incremental mode.
    pub fn with_index(mut self, index: Arc<DocumentIndex>) -> Self {
        self.index = Some(index);
        self
    }

    /// Set workspace root for path normalization.
    pub fn with_workspace_root(mut self, root: Option<PathBuf>) -> Self {
        self.workspace_root = root;
        self
    }

    /// Normalize a path relative to workspace_root.
    fn normalize_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            if let Some(ref root) = self.workspace_root {
                path.strip_prefix(root)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| path.to_path_buf())
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        }
    }

    /// Run the discover stage, sending paths to the provided channel.
    ///
    /// Returns the number of files discovered.
    pub fn run(&self, sender: Sender<PathBuf>) -> PipelineResult<usize> {
        let extensions = get_supported_extensions()?;
        let count = Arc::new(AtomicUsize::new(0));

        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(false) // Don't auto-skip hidden directories
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .follow_links(false) // Don't follow symlinks
            .require_git(false) // Allow gitignore to work in non-git directories
            .threads(self.threads);

        // Support .codannaignore files (matches FileWalker behavior)
        builder.add_custom_ignore_filename(".codannaignore");

        let walker = builder.build_parallel();

        let count_clone = count.clone();
        let extensions = Arc::new(extensions);

        walker.run(|| {
            let sender = sender.clone();
            let extensions = extensions.clone();
            let count = count_clone.clone();

            Box::new(move |entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                // Skip directories
                if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                    return ignore::WalkState::Continue;
                }

                let path = entry.path();

                // Skip hidden files (files starting with .) - matches FileWalker behavior
                if let Some(file_name) = path.file_name() {
                    if let Some(name_str) = file_name.to_str() {
                        if name_str.starts_with('.') {
                            return ignore::WalkState::Continue;
                        }
                    }
                }

                // Filter by extension
                if !has_supported_extension(path, &extensions) {
                    return ignore::WalkState::Continue;
                }

                // Send path to channel
                count.fetch_add(1, Ordering::Relaxed);
                if sender.send(path.to_path_buf()).is_err() {
                    // Channel closed, stop walking
                    return ignore::WalkState::Quit;
                }

                ignore::WalkState::Continue
            })
        });

        Ok(count.load(Ordering::Relaxed))
    }

    /// Run incremental discovery, comparing disk state to index.
    ///
    /// Returns categorized files: new, modified, and deleted.
    /// Requires an index to be set via `with_index()`.
    pub fn run_incremental(&self) -> PipelineResult<DiscoverResult> {
        let index = self.index.as_ref().ok_or_else(|| PipelineError::Parse {
            path: self.root.clone(),
            reason: "Incremental mode requires an index".to_string(),
        })?;

        // Step 1: Collect all current files on disk, normalized to relative paths
        let disk_files = self.collect_all_files()?;
        let disk_set: HashSet<PathBuf> = disk_files
            .into_iter()
            .map(|p| self.normalize_path(&p))
            .collect();

        // Step 2: Get indexed paths from Tantivy, filtered to only those under our root
        // This prevents marking files from other indexed directories as "deleted"
        let normalized_root = self.normalize_path(&self.root);
        let indexed_paths = index.get_all_indexed_paths()?;
        let indexed_set: HashSet<PathBuf> = indexed_paths
            .into_iter()
            .filter(|p| p.starts_with(&normalized_root))
            .collect();

        tracing::debug!(
            target: "pipeline",
            "incremental: root={}, normalized_root={}, disk={}, indexed={}",
            self.root.display(),
            normalized_root.display(),
            disk_set.len(),
            indexed_set.len()
        );

        // Step 3: Categorize files
        let mut result = DiscoverResult::default();

        // New files: on disk but not in index
        for path in &disk_set {
            if !indexed_set.contains(path) {
                result.new_files.push(path.clone());
            }
        }

        // Deleted files: in index but not on disk
        for path in &indexed_set {
            if !disk_set.contains(path) {
                result.deleted_files.push(path.clone());
            }
        }

        // Modified files: in both, but hash differs
        for path in disk_set.intersection(&indexed_set) {
            if self.is_modified(path, index)? {
                result.modified_files.push(path.clone());
            }
        }

        // Step 4: pair deleted x new by exact content hash. A rename
        // surfaces as deleted(old) + new(new); the stored hash of the old
        // path against the computed hash of the new file is identity-grade
        // evidence that the file moved, so its inbound edges relocate
        // instead of dying with genuine-deletion semantics.
        if !result.new_files.is_empty() && !result.deleted_files.is_empty() {
            let mut deleted_hashed = Vec::new();
            for path in &result.deleted_files {
                let path_str = path.to_string_lossy();
                if let Some((_file_id, hash, _mtime)) = index.get_file_info(&path_str)? {
                    deleted_hashed.push((path.clone(), hash));
                }
            }
            let mut new_hashed = Vec::new();
            for path in &result.new_files {
                match fs::read_to_string(path) {
                    Ok(content) => new_hashed.push((path.clone(), calculate_hash(&content))),
                    Err(e) => {
                        // Unreadable now means unpairable now; the file is
                        // read again at parse, where failure is surfaced.
                        tracing::debug!(
                            target: "pipeline",
                            "rename pairing skipped {}: {e}",
                            path.display()
                        );
                    }
                }
            }
            let pairs = pair_relocations(&deleted_hashed, &new_hashed);
            if !pairs.is_empty() {
                {
                    let old_set: HashSet<&PathBuf> = pairs.iter().map(|(old, _)| old).collect();
                    let new_set: HashSet<&PathBuf> = pairs.iter().map(|(_, new)| new).collect();
                    result.deleted_files.retain(|p| !old_set.contains(p));
                    result.new_files.retain(|p| !new_set.contains(p));
                }
                result.renamed_files = pairs;
            }
        }

        tracing::debug!(
            target: "pipeline",
            "incremental result: new={}, modified={}, deleted={}, renamed={}",
            result.new_files.len(),
            result.modified_files.len(),
            result.deleted_files.len(),
            result.renamed_files.len()
        );

        Ok(result)
    }

    /// Collect all files on disk (synchronous, for incremental comparison).
    fn collect_all_files(&self) -> PipelineResult<Vec<PathBuf>> {
        let extensions = get_supported_extensions()?;
        let mut files = Vec::new();

        // Use sequential walker for simplicity in incremental mode
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(false) // Don't auto-skip hidden directories
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .follow_links(false) // Don't follow symlinks
            .require_git(false); // Allow gitignore to work in non-git directories

        // Support .codannaignore files (matches FileWalker behavior)
        builder.add_custom_ignore_filename(".codannaignore");

        let walker = builder.build();

        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                continue;
            }

            let path = entry.path();

            // Skip hidden files (files starting with .) - matches FileWalker behavior
            if let Some(file_name) = path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.starts_with('.') {
                        continue;
                    }
                }
            }

            if has_supported_extension(path, &extensions) {
                files.push(path.to_path_buf());
            }
        }

        Ok(files)
    }

    /// Check if a file has been modified.
    /// Uses mtime as fast heuristic - only reads file if mtime changed.
    fn is_modified(&self, path: &Path, index: &DocumentIndex) -> PipelineResult<bool> {
        let path_str = path.to_string_lossy();

        // Get stored info from index
        let stored_info = index.get_file_info(&path_str)?;
        let Some((_file_id, stored_hash, stored_mtime)) = stored_info else {
            // Not in index = treat as new
            tracing::trace!(target: "pipeline", "is_modified: {} not in index", path.display());
            return Ok(true);
        };

        // Fast path: check mtime first (stat only, no file read). Mtimes are
        // second-granular, so a rewrite inside the indexed second keeps the
        // stored value; only an mtime older than the current second proves
        // the file unchanged.
        let current_mtime = crate::indexing::file_info::get_file_mtime(path).unwrap_or(0);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if stored_mtime > 0 && current_mtime == stored_mtime && current_mtime < now_secs {
            return Ok(false);
        }

        // mtime changed or unknown - verify with hash (requires file read)
        let content = fs::read_to_string(path).map_err(|e| PipelineError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let current_hash = calculate_hash(&content);

        let modified = current_hash != stored_hash;
        if modified {
            tracing::trace!(
                target: "pipeline",
                "is_modified: {} hash changed (stored_mtime={}, current_mtime={})",
                path.display(),
                stored_mtime,
                current_mtime
            );
        }

        Ok(modified)
    }
}

/// Pair deleted x new files by exact content hash.
///
/// A pair forms only when a hash matches exactly one deleted and exactly one
/// new path. Every other multiplicity -- zero, many-to-one, one-to-many -- is
/// not identity evidence, so both sides keep genuine-deletion semantics.
/// Multiplicity decides, never candidate order: iteration order cannot mint
/// a pair.
fn pair_relocations(
    deleted: &[(PathBuf, String)],
    new: &[(PathBuf, String)],
) -> Vec<(PathBuf, PathBuf)> {
    use std::collections::HashMap;

    let mut deleted_by_hash: HashMap<&str, Vec<&PathBuf>> = HashMap::new();
    for (path, hash) in deleted {
        deleted_by_hash.entry(hash.as_str()).or_default().push(path);
    }
    let mut new_by_hash: HashMap<&str, Vec<&PathBuf>> = HashMap::new();
    for (path, hash) in new {
        new_by_hash.entry(hash.as_str()).or_default().push(path);
    }

    let mut pairs = Vec::new();
    for (hash, olds) in &deleted_by_hash {
        let news = new_by_hash.get(hash).map(Vec::as_slice).unwrap_or_default();
        if let ([old], [new_path]) = (olds.as_slice(), news) {
            pairs.push(((*old).clone(), (*new_path).clone()));
        }
    }
    pairs.sort();
    pairs
}

/// Get all supported file extensions from the language registry.
fn get_supported_extensions() -> PipelineResult<HashSet<&'static str>> {
    let registry = get_registry();
    let registry = registry.lock().map_err(|e| PipelineError::Parse {
        path: PathBuf::new(),
        reason: format!("Failed to acquire registry lock: {e}"),
    })?;

    let mut extensions = HashSet::new();
    for def in registry.iter_all() {
        for ext in def.extensions() {
            extensions.insert(*ext);
        }
    }

    Ok(extensions)
}

/// Check if a path has a supported extension.
fn has_supported_extension(path: &Path, extensions: &HashSet<&str>) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| extensions.contains(ext))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;

    #[test]
    fn test_discover_examples_directory() {
        let (sender, receiver) = bounded(1000);

        let stage = DiscoverStage::new("examples", 4);
        let result = stage.run(sender);

        assert!(result.is_ok(), "Discover should succeed");
        let count = result.unwrap();

        // Collect all discovered paths
        let paths: Vec<PathBuf> = receiver.iter().collect();

        println!("Discovered {count} files:");
        for path in &paths {
            println!("  - {}", path.display());
        }

        assert_eq!(paths.len(), count, "Count should match received paths");
        assert!(
            count > 0,
            "Should discover at least some files in examples/"
        );

        // Verify all paths have supported extensions
        let extensions = get_supported_extensions().unwrap();
        for path in &paths {
            assert!(
                has_supported_extension(path, &extensions),
                "Path {} should have supported extension",
                path.display()
            );
        }
    }

    #[test]
    fn test_discover_respects_gitignore() {
        let (sender, receiver) = bounded(1000);

        let stage = DiscoverStage::new(".", 4);
        let _count = stage.run(sender);

        let paths: Vec<PathBuf> = receiver.iter().collect();

        // Should not include target/ directory contents
        for path in &paths {
            let path_str = path.to_string_lossy();
            assert!(
                !path_str.contains("target/debug") && !path_str.contains("target/release"),
                "Should not include target/ contents: {}",
                path.display()
            );
        }
    }

    // Pinning locks for the relocation pairing gate: a pair needs a hash
    // matching exactly one deleted and exactly one new path. Multiplicity
    // decides; ambiguity keeps genuine-deletion semantics.
    #[test]
    fn pair_relocations_pairs_unique_hash_match() {
        let pairs = pair_relocations(
            &[(PathBuf::from("old/a.go"), "h1".to_string())],
            &[(PathBuf::from("new/a.go"), "h1".to_string())],
        );
        assert_eq!(
            pairs,
            vec![(PathBuf::from("old/a.go"), PathBuf::from("new/a.go"))]
        );
    }

    #[test]
    fn pair_relocations_refuses_ambiguous_multiplicities() {
        // Two deleted share the hash of one new: no pair.
        let two_olds = pair_relocations(
            &[
                (PathBuf::from("a.go"), "h1".to_string()),
                (PathBuf::from("b.go"), "h1".to_string()),
            ],
            &[(PathBuf::from("c.go"), "h1".to_string())],
        );
        assert!(two_olds.is_empty(), "many-to-one must not pair");

        // One deleted matches two new: no pair.
        let two_news = pair_relocations(
            &[(PathBuf::from("a.go"), "h1".to_string())],
            &[
                (PathBuf::from("b.go"), "h1".to_string()),
                (PathBuf::from("c.go"), "h1".to_string()),
            ],
        );
        assert!(two_news.is_empty(), "one-to-many must not pair");

        // Hash mismatch (rename plus edit): no pair.
        let edited = pair_relocations(
            &[(PathBuf::from("a.go"), "h1".to_string())],
            &[(PathBuf::from("b.go"), "h2".to_string())],
        );
        assert!(edited.is_empty(), "a content edit must fail closed");
    }

    #[test]
    fn pair_relocations_pairs_each_unique_hash_independently() {
        // A directory rename decomposes per-file: unique hashes pair,
        // the duplicate pair stays on deletion semantics.
        let pairs = pair_relocations(
            &[
                (PathBuf::from("old/a.go"), "ha".to_string()),
                (PathBuf::from("old/b.go"), "hb".to_string()),
                (PathBuf::from("old/dup1.go"), "hd".to_string()),
                (PathBuf::from("old/dup2.go"), "hd".to_string()),
            ],
            &[
                (PathBuf::from("new/a.go"), "ha".to_string()),
                (PathBuf::from("new/b.go"), "hb".to_string()),
                (PathBuf::from("new/dup1.go"), "hd".to_string()),
                (PathBuf::from("new/dup2.go"), "hd".to_string()),
            ],
        );
        assert_eq!(
            pairs,
            vec![
                (PathBuf::from("old/a.go"), PathBuf::from("new/a.go")),
                (PathBuf::from("old/b.go"), PathBuf::from("new/b.go")),
            ]
        );
    }

    #[test]
    fn test_get_supported_extensions() {
        let extensions = get_supported_extensions().unwrap();

        println!("Supported extensions: {extensions:?}");

        // Should include common extensions
        assert!(extensions.contains("rs"), "Should support .rs");
        assert!(extensions.contains("py"), "Should support .py");
        assert!(extensions.contains("ts"), "Should support .ts");
        assert!(extensions.contains("go"), "Should support .go");
    }

    fn pin_mtime(path: &Path, secs: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }

    fn register_file(index: &DocumentIndex, path: &Path, content: &str, mtime: u64) {
        index.start_batch().unwrap();
        index
            .store_file_registration(&crate::indexing::pipeline::FileRegistration {
                path: path.to_path_buf(),
                file_id: crate::FileId::new(1).unwrap(),
                content_hash: calculate_hash(content),
                language_id: crate::parsing::LanguageId::new("rust"),
                timestamp: 0,
                mtime,
            })
            .unwrap();
        index.commit_batch().unwrap();
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn run_incremental_reindexes_a_same_second_rewrite() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let settings = crate::config::Settings::default();
        let index = Arc::new(DocumentIndex::new(root.join("index"), &settings).unwrap());
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("a.rs");

        // Two seconds ahead: "not older than the current second" holds for
        // the whole test even if the wall clock crosses a boundary.
        let hot = now_secs() + 2;
        fs::write(&file, "fn a() {}\n").unwrap();
        pin_mtime(&file, hot);
        register_file(&index, &file, "fn a() {}\n", hot);

        fs::write(&file, "fn a() {}\nfn b() {}\n").unwrap();
        pin_mtime(&file, hot);

        let result = DiscoverStage::new(&src, 1)
            .with_index(Arc::clone(&index))
            .run_incremental()
            .unwrap();
        assert!(
            result.modified_files.contains(&file),
            "a rewrite in the indexed second must reach the hash compare; got {:?}",
            result.modified_files
        );
    }

    #[test]
    fn run_incremental_keeps_stat_only_skip_for_old_equal_mtime() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let settings = crate::config::Settings::default();
        let index = Arc::new(DocumentIndex::new(root.join("index"), &settings).unwrap());
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("a.rs");

        let old = now_secs() - 100;
        fs::write(&file, "fn a() {}\n").unwrap();
        pin_mtime(&file, old);
        register_file(&index, &file, "fn a() {}\n", old);

        // Content differs on disk but the mtime still reads as the indexed
        // value from a past second: the stat-only path must skip it, so a
        // hash compare would be the only way to notice this rewrite.
        fs::write(&file, "fn a() {}\nfn b() {}\n").unwrap();
        pin_mtime(&file, old);

        let result = DiscoverStage::new(&src, 1)
            .with_index(Arc::clone(&index))
            .run_incremental()
            .unwrap();
        assert!(
            result.modified_files.is_empty(),
            "an old equal mtime must be skipped without a content read; got {:?}",
            result.modified_files
        );
    }
}
