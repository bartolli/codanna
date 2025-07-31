//! Progress reporting for indexing operations

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Statistics collected during indexing
#[derive(Debug, Default)]
pub struct IndexStats {
    /// Number of files successfully indexed
    pub files_indexed: usize,
    
    /// Number of files that failed to index
    pub files_failed: usize,
    
    /// Total number of symbols found
    pub symbols_found: usize,
    
    /// Time elapsed during indexing
    pub elapsed: Duration,
    
    /// Errors encountered (limited to first N errors)
    pub errors: Vec<(PathBuf, String)>,
    
    /// Start time of indexing
    start_time: Option<Instant>,
}

impl IndexStats {
    /// Create new stats and start timing
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }
    
    /// Stop timing and record elapsed time
    pub fn stop_timing(&mut self) {
        if let Some(start) = self.start_time {
            self.elapsed = start.elapsed();
            self.start_time = None;
        }
    }
    
    /// Add an error (limited to first 100 errors)
    pub fn add_error(&mut self, path: PathBuf, error: String) {
        if self.errors.len() < 100 {
            self.errors.push((path, error));
        }
        self.files_failed += 1;
    }
    
    /// Display the statistics in a human-readable format
    pub fn display(&self) {
        println!("\nIndexing Complete:");
        println!("  Files indexed: {}", self.files_indexed);
        println!("  Files failed: {}", self.files_failed);
        println!("  Symbols found: {}", self.symbols_found);
        println!("  Time elapsed: {:.2}s", self.elapsed.as_secs_f64());
        
        if self.files_indexed > 0 {
            let files_per_sec = self.files_indexed as f64 / self.elapsed.as_secs_f64();
            println!("  Performance: {:.0} files/second", files_per_sec);
            
            let symbols_per_file = self.symbols_found as f64 / self.files_indexed as f64;
            println!("  Average symbols/file: {:.1}", symbols_per_file);
        }
        
        if !self.errors.is_empty() {
            println!("\nErrors (showing first {}):", self.errors.len().min(5));
            for (path, error) in &self.errors[..5.min(self.errors.len())] {
                println!("  {}: {}", path.display(), error);
            }
            if self.errors.len() > 5 {
                println!("  ... and {} more errors", self.errors.len() - 5);
            }
        }
    }
    
    /// Get a summary line for progress updates
    pub fn progress_line(&self, current_files: usize, total_files: usize) -> String {
        let percent = (current_files as f64 / total_files as f64 * 100.0) as u32;
        let elapsed = self.start_time
            .map(|start| start.elapsed())
            .unwrap_or(self.elapsed);
        
        if current_files > 0 && elapsed.as_secs() > 0 {
            let files_per_sec = current_files as f64 / elapsed.as_secs_f64();
            let eta_secs = ((total_files - current_files) as f64 / files_per_sec) as u64;
            let eta = Duration::from_secs(eta_secs);
            
            format!(
                "Indexing: {}/{} files ({}%) - {:.0} files/s - ETA: {}",
                current_files,
                total_files,
                percent,
                files_per_sec,
                format_duration(eta)
            )
        } else {
            format!(
                "Indexing: {}/{} files ({}%)",
                current_files,
                total_files,
                percent
            )
        }
    }
}

/// Format a duration in a human-readable way
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_stats_display() {
        let mut stats = IndexStats::new();
        stats.files_indexed = 100;
        stats.files_failed = 2;
        stats.symbols_found = 1500;
        stats.elapsed = Duration::from_secs(5);
        
        // Should not panic
        stats.display();
    }
    
    #[test]
    fn test_error_limiting() {
        let mut stats = IndexStats::new();
        
        // Add 150 errors
        for i in 0..150 {
            stats.add_error(
                PathBuf::from(format!("file{}.rs", i)),
                format!("Error {}", i)
            );
        }
        
        // Should only keep first 100
        assert_eq!(stats.errors.len(), 100);
        assert_eq!(stats.files_failed, 150);
    }
    
    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m");
    }
}