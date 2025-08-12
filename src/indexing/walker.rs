//! File system walker for discovering source files to index
//!
//! This module provides efficient directory traversal with support for:
//! - .gitignore rules
//! - Custom ignore patterns from configuration
//! - Language filtering
//! - Hidden file handling

use crate::Settings;
use crate::parsing::Language;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Walks directories to find source files to index
#[derive(Debug)]
pub struct FileWalker {
    settings: Arc<Settings>,
}

impl FileWalker {
    /// Create a new file walker with the given settings
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }

    /// Walk a directory and return an iterator of files to index
    pub fn walk(&self, root: &Path) -> impl Iterator<Item = PathBuf> {
        let mut builder = WalkBuilder::new(root);

        // Configure the walker
        builder
            .hidden(false) // Don't traverse hidden directories by default
            .git_ignore(true) // Respect .gitignore files
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .follow_links(false) // Don't follow symlinks by default
            .max_depth(None) // No depth limit
            .require_git(false); // Allow gitignore to work in non-git directories

        // Always support .codannaignore files for custom ignore patterns (follows .gitignore pattern)
        builder.add_custom_ignore_filename(".codannaignore");

        // The ignore crate's override feature is for INCLUDING files, not excluding them.
        // To add custom ignore patterns, we need to use a different approach.
        // For now, we'll rely on .gitignore and .codannaignore files.

        // TODO: Add support for custom ignore patterns from settings
        // One approach would be to create a temporary .codanna-ignore file
        // or use the glob filtering in the iterator below

        // Get enabled languages for filtering
        let enabled_languages = self.get_enabled_languages();

        // Build and filter the walker
        builder
            .build()
            .filter_map(Result::ok) // Skip files we can't access
            .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
            .filter_map(move |entry| {
                let path = entry.path();

                // Skip hidden files (files starting with .)
                if let Some(file_name) = path.file_name() {
                    if let Some(name_str) = file_name.to_str() {
                        if name_str.starts_with('.') {
                            return None;
                        }
                    }
                }

                // Check if this is a supported and enabled language file
                if let Some(language) = Language::from_path(path) {
                    if enabled_languages.contains(&language) {
                        return Some(path.to_path_buf());
                    }
                }

                None
            })
    }

    /// Get list of enabled languages from settings
    fn get_enabled_languages(&self) -> Vec<Language> {
        vec![
            Language::Rust,
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::Php,
        ]
        .into_iter()
        .filter(|&lang| {
            self.settings
                .languages
                .get(lang.config_key())
                .map(|config| config.enabled)
                .unwrap_or(false)
        })
        .collect()
    }

    /// Count files that would be indexed (useful for dry runs)
    pub fn count_files(&self, root: &Path) -> usize {
        self.walk(root).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_settings() -> Arc<Settings> {
        let mut settings = Settings::default();
        // Enable only Rust for testing
        settings.languages.get_mut("rust").unwrap().enabled = true;
        Arc::new(settings)
    }

    #[test]
    fn test_walk_directory() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create some test files
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("lib.rs"), "pub fn lib() {}").unwrap();
        fs::write(root.join("test.py"), "def test(): pass").unwrap();
        fs::write(root.join("README.md"), "# Test").unwrap();

        let settings = create_test_settings();
        let walker = FileWalker::new(settings);

        let files: Vec<_> = walker.walk(root).collect();

        // Should find only Rust files
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|p| p.ends_with("main.rs")));
        assert!(files.iter().any(|p| p.ends_with("lib.rs")));
    }

    #[test]
    fn test_ignore_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create hidden file and visible file
        fs::write(root.join(".hidden.rs"), "fn hidden() {}").unwrap();
        fs::write(root.join("visible.rs"), "fn visible() {}").unwrap();

        let settings = create_test_settings();
        let walker = FileWalker::new(settings);

        let files: Vec<_> = walker.walk(root).collect();

        // Should only find the visible file (hidden files are filtered out)
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("visible.rs"));
    }

    #[test]
    fn test_gitignore_respected() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create .gitignore (should work without git init due to require_git(false))
        fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();

        // Create files
        fs::write(root.join("ignored.rs"), "fn ignored() {}").unwrap();
        fs::write(root.join("included.rs"), "fn included() {}").unwrap();

        let settings = create_test_settings();
        let walker = FileWalker::new(settings);

        let files: Vec<_> = walker.walk(root).collect();

        // Should only find the included file
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("included.rs"));
    }
}
