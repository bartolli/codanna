//! Shared debouncing logic for file change events.
//!
//! Debouncing prevents excessive re-indexing when files are saved
//! multiple times in quick succession (e.g., auto-save, IDE formatting).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Debounces file change events by path.
///
/// Records change timestamps and returns paths that have been stable
/// for the configured duration.
#[derive(Debug)]
pub struct Debouncer {
    /// Pending changes: path -> last change timestamp.
    pending: HashMap<PathBuf, Instant>,
    /// How long a file must be stable before processing.
    duration: Duration,
}

impl Debouncer {
    /// Create a new debouncer with the given duration in milliseconds.
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            pending: HashMap::new(),
            duration: Duration::from_millis(debounce_ms),
        }
    }

    /// Record a file change event.
    ///
    /// Resets the debounce timer for this path.
    pub fn record(&mut self, path: PathBuf) {
        self.pending.insert(path, Instant::now());
    }

    /// Remove a path from pending (e.g., when file is deleted).
    pub fn remove(&mut self, path: &PathBuf) {
        self.pending.remove(path);
    }

    /// Take all paths that have been stable for the debounce duration.
    ///
    /// Returns paths ready for processing and removes them from pending.
    pub fn take_ready(&mut self) -> Vec<PathBuf> {
        let now = Instant::now();
        let mut ready = Vec::new();

        self.pending.retain(|path, last_change| {
            if now.duration_since(*last_change) >= self.duration {
                ready.push(path.clone());
                false // Remove from pending
            } else {
                true // Keep in pending
            }
        });

        ready
    }

    /// Check if there are any pending changes.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Get the number of pending changes.
    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_debouncer_basic() {
        let mut debouncer = Debouncer::new(50); // 50ms debounce

        let path = PathBuf::from("/test/file.rs");
        debouncer.record(path.clone());

        // Immediately after, nothing should be ready
        assert!(debouncer.take_ready().is_empty());
        assert!(debouncer.has_pending());

        // Wait for debounce period
        sleep(Duration::from_millis(60));

        // Now it should be ready
        let ready = debouncer.take_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], path);
        assert!(!debouncer.has_pending());
    }

    #[test]
    fn test_debouncer_resets_on_new_change() {
        let mut debouncer = Debouncer::new(50);

        let path = PathBuf::from("/test/file.rs");
        debouncer.record(path.clone());

        // Wait half the debounce period
        sleep(Duration::from_millis(30));

        // Record again - should reset the timer
        debouncer.record(path.clone());

        // Wait another 30ms (total 60ms from first, but only 30ms from second)
        sleep(Duration::from_millis(30));

        // Should not be ready yet (need 50ms from last change)
        assert!(debouncer.take_ready().is_empty());

        // Wait for the remaining time
        sleep(Duration::from_millis(30));

        // Now it should be ready
        let ready = debouncer.take_ready();
        assert_eq!(ready.len(), 1);
    }

    #[test]
    fn test_debouncer_multiple_files() {
        let mut debouncer = Debouncer::new(50);

        let path1 = PathBuf::from("/test/file1.rs");
        let path2 = PathBuf::from("/test/file2.rs");

        debouncer.record(path1.clone());
        sleep(Duration::from_millis(30));
        debouncer.record(path2.clone());

        // Wait for path1 to be ready (50ms total)
        sleep(Duration::from_millis(25));

        let ready = debouncer.take_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], path1);

        // path2 should still be pending
        assert!(debouncer.has_pending());

        // Wait for path2
        sleep(Duration::from_millis(30));

        let ready = debouncer.take_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], path2);
    }

    #[test]
    fn test_debouncer_remove() {
        let mut debouncer = Debouncer::new(50);

        let path = PathBuf::from("/test/file.rs");
        debouncer.record(path.clone());
        assert!(debouncer.has_pending());

        debouncer.remove(&path);
        assert!(!debouncer.has_pending());
    }
}
