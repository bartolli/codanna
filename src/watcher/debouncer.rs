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
    /// Pending removal observations: path -> last change timestamp.
    /// Removals defer like modifications: a rename arrives as
    /// remove(old) + create(new), and only a window holding both sides
    /// lets the shared discovery pair them.
    pending_removals: HashMap<PathBuf, Instant>,
    /// How long a file must be stable before processing.
    duration: Duration,
}

impl Debouncer {
    /// Create a new debouncer with the given duration in milliseconds.
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            pending: HashMap::new(),
            pending_removals: HashMap::new(),
            duration: Duration::from_millis(debounce_ms),
        }
    }

    /// Record a file change event.
    ///
    /// Resets the debounce timer for this path. Cancels a pending
    /// removal of the same path: the last observation wins.
    pub fn record(&mut self, path: PathBuf) {
        self.pending_removals.remove(&path);
        self.pending.insert(path, Instant::now());
    }

    /// Record a removal observation for this path.
    ///
    /// Cancels a pending modification of the same path: the last
    /// observation wins.
    pub fn record_removal(&mut self, path: PathBuf) {
        self.pending.remove(&path);
        self.pending_removals.insert(path, Instant::now());
    }

    /// Remove a path from pending (both flavors).
    pub fn remove(&mut self, path: &PathBuf) {
        self.pending.remove(path);
        self.pending_removals.remove(path);
    }

    /// Take all paths that have been stable for the debounce duration.
    ///
    /// Returns paths ready for processing and removes them from pending.
    pub fn take_ready(&mut self) -> Vec<PathBuf> {
        Self::take_stable(&mut self.pending, self.duration)
    }

    /// Drain the whole burst once every entry of BOTH flavors is stable.
    ///
    /// Returns `(removed, modified)`, or `None` while any entry is still
    /// inside its window: the matching create of a rename may trail the
    /// remove, and a wave missing one side deletes what actually moved.
    /// `None` when no removal is pending -- modification-only bursts take
    /// the per-path `take_ready` lane.
    pub fn take_settled_burst(&mut self) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
        let now = Instant::now();
        let stable = |pending: &HashMap<PathBuf, Instant>| {
            pending
                .values()
                .all(|last| now.duration_since(*last) >= self.duration)
        };
        if self.pending_removals.is_empty()
            || !stable(&self.pending)
            || !stable(&self.pending_removals)
        {
            return None;
        }
        let removed = self
            .pending_removals
            .drain()
            .map(|(path, _)| path)
            .collect();
        let modified = self.pending.drain().map(|(path, _)| path).collect();
        Some((removed, modified))
    }

    fn take_stable(pending: &mut HashMap<PathBuf, Instant>, duration: Duration) -> Vec<PathBuf> {
        let now = Instant::now();
        let mut ready = Vec::new();

        pending.retain(|path, last_change| {
            if now.duration_since(*last_change) >= duration {
                ready.push(path.clone());
                false // Remove from pending
            } else {
                true // Keep in pending
            }
        });

        ready
    }

    /// Check if there are any pending changes of either flavor.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.pending_removals.is_empty()
    }

    /// Check if any removal observation is pending.
    pub fn has_pending_removals(&self) -> bool {
        !self.pending_removals.is_empty()
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

    // Removals defer like modifications: a rename arrives as remove(old)
    // + create(new), and only a window holding both sides lets the shared
    // discovery pair them.
    #[test]
    fn removals_defer_for_the_debounce_window() {
        let mut debouncer = Debouncer::new(50);

        let path = PathBuf::from("/test/old.rs");
        debouncer.record_removal(path.clone());

        assert!(debouncer.take_settled_burst().is_none());
        assert!(debouncer.has_pending());
        assert!(debouncer.has_pending_removals());

        sleep(Duration::from_millis(60));

        let (removed, modified) = debouncer.take_settled_burst().unwrap();
        assert_eq!(removed, vec![path]);
        assert!(modified.is_empty());
        assert!(!debouncer.has_pending());
    }

    // The burst drains only when EVERY side is stable: a create trailing
    // the remove must not be split off into its own per-file wave, or the
    // pairing sees a deleted file with no new side.
    #[test]
    fn burst_holds_until_every_side_is_stable() {
        let mut debouncer = Debouncer::new(50);
        let old = PathBuf::from("/test/old.rs");
        let new = PathBuf::from("/test/new.rs");

        debouncer.record_removal(old.clone());
        sleep(Duration::from_millis(60));
        // The removal alone is stable, but the trailing create is not.
        debouncer.record(new.clone());
        assert!(debouncer.take_settled_burst().is_none());

        sleep(Duration::from_millis(60));
        let (removed, modified) = debouncer.take_settled_burst().unwrap();
        assert_eq!(removed, vec![old]);
        assert_eq!(modified, vec![new]);
    }

    #[test]
    fn opposite_observations_for_one_path_cancel() {
        let mut debouncer = Debouncer::new(50);
        let path = PathBuf::from("/test/file.rs");

        // Remove then create: the file is alive; only a modification fires.
        debouncer.record_removal(path.clone());
        debouncer.record(path.clone());
        sleep(Duration::from_millis(60));
        assert!(
            debouncer.take_settled_burst().is_none(),
            "no removal pending after the create cancelled it"
        );
        assert_eq!(debouncer.take_ready(), vec![path.clone()]);

        // Create then remove: the file is gone; only a removal fires.
        debouncer.record(path.clone());
        debouncer.record_removal(path.clone());
        sleep(Duration::from_millis(60));
        let (removed, modified) = debouncer.take_settled_burst().unwrap();
        assert_eq!(removed, vec![path]);
        assert!(modified.is_empty());
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
