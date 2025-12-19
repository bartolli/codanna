//! Path registry with interning and watch directory computation.
//!
//! Provides efficient path storage and lookup for the unified watcher.
//! Paths are interned (stored once) to avoid duplicate allocations.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Registry for watched paths with interning.
///
/// Stores paths efficiently and computes the minimal set of directories
/// needed to watch all tracked files.
#[derive(Debug, Default)]
pub struct PathRegistry {
    /// Interned paths - each unique path stored once.
    paths: HashSet<Arc<PathBuf>>,
    /// Computed watch directories (parent dirs of tracked files).
    watch_dirs: HashSet<PathBuf>,
}

impl PathRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add paths to the registry, returning newly added directories to watch.
    ///
    /// Returns directories that weren't previously being watched.
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        let mut new_dirs = Vec::new();

        for path in paths {
            // Intern the path
            let arc_path = Arc::new(path);
            if self.paths.insert(arc_path.clone()) {
                // New path - check if we need to watch its parent
                if let Some(parent) = arc_path.parent() {
                    let parent_path = if parent.as_os_str().is_empty() {
                        PathBuf::from(".")
                    } else {
                        parent.to_path_buf()
                    };

                    if self.watch_dirs.insert(parent_path.clone()) {
                        new_dirs.push(parent_path);
                    }
                }
            }
        }

        new_dirs
    }

    /// Remove a path from the registry.
    ///
    /// Note: Does not remove watch directories even if empty, as other
    /// handlers might still have files there.
    pub fn remove_path(&mut self, path: &Path) {
        self.paths.retain(|p| p.as_ref() != path);
    }

    /// Check if a path is in the registry.
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.iter().any(|p| p.as_ref() == path)
    }

    /// Get all tracked paths.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.paths.iter().map(|p| p.as_ref().as_path())
    }

    /// Get all watch directories.
    pub fn watch_dirs(&self) -> &HashSet<PathBuf> {
        &self.watch_dirs
    }

    /// Get count of tracked paths.
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// Get count of watch directories.
    pub fn dir_count(&self) -> usize {
        self.watch_dirs.len()
    }

    /// Clear all paths and rebuild from the given paths.
    pub fn rebuild(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.paths.clear();
        self.watch_dirs.clear();
        self.add_paths(paths);
    }

    /// Compute watch directories for a set of paths without storing them.
    ///
    /// Utility function for one-off computations.
    pub fn compute_watch_dirs(paths: &[PathBuf]) -> HashSet<PathBuf> {
        let mut dirs = HashSet::new();

        for path in paths {
            if let Some(parent) = path.parent() {
                if parent.as_os_str().is_empty() {
                    dirs.insert(PathBuf::from("."));
                } else {
                    dirs.insert(parent.to_path_buf());
                }
            }
        }

        dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_registry_basic() {
        let mut registry = PathRegistry::new();

        let paths = vec![
            PathBuf::from("/project/src/main.rs"),
            PathBuf::from("/project/src/lib.rs"),
            PathBuf::from("/project/tests/test.rs"),
        ];

        let new_dirs = registry.add_paths(paths.clone());

        // Should have 2 unique directories
        assert_eq!(new_dirs.len(), 2);
        assert!(new_dirs.contains(&PathBuf::from("/project/src")));
        assert!(new_dirs.contains(&PathBuf::from("/project/tests")));

        // All paths should be tracked
        assert_eq!(registry.path_count(), 3);
        assert!(registry.contains(Path::new("/project/src/main.rs")));
    }

    #[test]
    fn test_path_registry_interning() {
        let mut registry = PathRegistry::new();

        let path = PathBuf::from("/project/src/main.rs");

        // Add same path twice
        let dirs1 = registry.add_paths(vec![path.clone()]);
        let dirs2 = registry.add_paths(vec![path.clone()]);

        // First add should return the directory
        assert_eq!(dirs1.len(), 1);

        // Second add should return nothing (already tracked)
        assert!(dirs2.is_empty());

        // Only one path stored
        assert_eq!(registry.path_count(), 1);
    }

    #[test]
    fn test_path_registry_remove() {
        let mut registry = PathRegistry::new();

        let path = PathBuf::from("/project/src/main.rs");
        registry.add_paths(vec![path.clone()]);

        assert!(registry.contains(&path));

        registry.remove_path(&path);

        assert!(!registry.contains(&path));
        assert_eq!(registry.path_count(), 0);
    }

    #[test]
    fn test_path_registry_root_files() {
        let mut registry = PathRegistry::new();

        // File at root level
        let path = PathBuf::from("Cargo.toml");
        let dirs = registry.add_paths(vec![path]);

        // Should watch current directory
        assert_eq!(dirs.len(), 1);
        assert!(dirs.contains(&PathBuf::from(".")));
    }

    #[test]
    fn test_compute_watch_dirs() {
        let paths = vec![
            PathBuf::from("/a/b/file1.rs"),
            PathBuf::from("/a/b/file2.rs"),
            PathBuf::from("/a/c/file3.rs"),
        ];

        let dirs = PathRegistry::compute_watch_dirs(&paths);

        assert_eq!(dirs.len(), 2);
        assert!(dirs.contains(&PathBuf::from("/a/b")));
        assert!(dirs.contains(&PathBuf::from("/a/c")));
    }
}
