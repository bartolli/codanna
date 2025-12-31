//! Discover stage - parallel file system walk
//!
//! Uses the `ignore` crate's parallel walker for high-performance
//! file discovery. Filters by supported extensions.

use crate::indexing::pipeline::types::{PipelineError, PipelineResult};
use crate::parsing::get_registry;
use crossbeam_channel::Sender;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Discover stage for parallel file walking.
pub struct DiscoverStage {
    root: PathBuf,
    threads: usize,
}

impl DiscoverStage {
    /// Create a new discover stage.
    pub fn new(root: impl Into<PathBuf>, threads: usize) -> Self {
        Self {
            root: root.into(),
            threads: threads.max(1),
        }
    }

    /// Run the discover stage, sending paths to the provided channel.
    ///
    /// Returns the number of files discovered.
    pub fn run(&self, sender: Sender<PathBuf>) -> PipelineResult<usize> {
        let extensions = get_supported_extensions()?;
        let count = Arc::new(AtomicUsize::new(0));

        let walker = WalkBuilder::new(&self.root)
            .hidden(false) // Include hidden files
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .threads(self.threads)
            .build_parallel();

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
}
