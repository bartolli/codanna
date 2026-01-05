//! Pipeline metrics collection and reporting.
//!
//! Tracks timing, throughput, channel wait times, and memory usage
//! for each pipeline stage to identify bottlenecks.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, System};

/// Metrics for a single pipeline stage.
#[derive(Debug, Default)]
pub struct StageMetrics {
    /// Stage name
    pub name: &'static str,
    /// Number of threads used
    pub threads: usize,
    /// Total wall clock time
    pub wall_time: Duration,
    /// Time spent waiting on input channel (blocked)
    pub input_wait: Duration,
    /// Time spent waiting on output channel (backpressure)
    pub output_wait: Duration,
    /// Items processed
    pub items_processed: usize,
    /// Secondary metric (e.g., bytes for READ, symbols for PARSE)
    pub secondary_count: usize,
    /// Secondary metric label
    pub secondary_label: &'static str,
}

impl StageMetrics {
    /// Calculate throughput (items per second).
    pub fn throughput(&self) -> f64 {
        let secs = self.wall_time.as_secs_f64();
        if secs > 0.0 {
            self.items_processed as f64 / secs
        } else {
            0.0
        }
    }

    /// Calculate active time (wall time minus wait times).
    pub fn active_time(&self) -> Duration {
        self.wall_time
            .saturating_sub(self.input_wait)
            .saturating_sub(self.output_wait)
    }

    /// Calculate percentage of total pipeline time.
    pub fn percentage_of(&self, total: Duration) -> f64 {
        if total.as_nanos() > 0 {
            (self.wall_time.as_nanos() as f64 / total.as_nanos() as f64) * 100.0
        } else {
            0.0
        }
    }
}

/// Thread-safe metrics collector for use during pipeline execution.
#[derive(Debug)]
pub struct StageTracker {
    name: &'static str,
    threads: usize,
    start: Instant,
    items: AtomicUsize,
    secondary: AtomicUsize,
    secondary_label: &'static str,
    input_wait_ns: AtomicU64,
    output_wait_ns: AtomicU64,
}

impl StageTracker {
    /// Create a new stage tracker.
    pub fn new(name: &'static str, threads: usize) -> Self {
        Self {
            name,
            threads,
            start: Instant::now(),
            items: AtomicUsize::new(0),
            secondary: AtomicUsize::new(0),
            secondary_label: "",
            input_wait_ns: AtomicU64::new(0),
            output_wait_ns: AtomicU64::new(0),
        }
    }

    /// Create tracker with secondary metric label.
    pub fn with_secondary(mut self, label: &'static str) -> Self {
        self.secondary_label = label;
        self
    }

    /// Record an item processed.
    pub fn record_item(&self) {
        self.items.fetch_add(1, Ordering::Relaxed);
    }

    /// Record multiple items processed.
    pub fn record_items(&self, count: usize) {
        self.items.fetch_add(count, Ordering::Relaxed);
    }

    /// Record secondary metric (bytes, symbols, etc.).
    pub fn record_secondary(&self, count: usize) {
        self.secondary.fetch_add(count, Ordering::Relaxed);
    }

    /// Record time spent waiting for input.
    pub fn record_input_wait(&self, duration: Duration) {
        self.input_wait_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Record time spent waiting for output (backpressure).
    pub fn record_output_wait(&self, duration: Duration) {
        self.output_wait_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Finalize and return metrics.
    pub fn finalize(self) -> StageMetrics {
        StageMetrics {
            name: self.name,
            threads: self.threads,
            wall_time: self.start.elapsed(),
            input_wait: Duration::from_nanos(self.input_wait_ns.load(Ordering::Relaxed)),
            output_wait: Duration::from_nanos(self.output_wait_ns.load(Ordering::Relaxed)),
            items_processed: self.items.load(Ordering::Relaxed),
            secondary_count: self.secondary.load(Ordering::Relaxed),
            secondary_label: self.secondary_label,
        }
    }
}

/// Memory snapshot from sysinfo.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemorySnapshot {
    /// Resident set size in bytes
    pub rss: u64,
    /// Virtual memory in bytes
    pub virtual_mem: u64,
}

impl MemorySnapshot {
    /// Get current process memory usage.
    pub fn current() -> Self {
        let mut sys = System::new();
        let pid = Pid::from_u32(std::process::id());
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );

        if let Some(process) = sys.process(pid) {
            Self {
                rss: process.memory(),
                virtual_mem: process.virtual_memory(),
            }
        } else {
            Self::default()
        }
    }

    /// Format RSS as human-readable string.
    pub fn rss_human(&self) -> String {
        format_bytes(self.rss)
    }
}

/// Format bytes as human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes}B")
    }
}

/// Complete pipeline metrics report.
#[derive(Debug, Default)]
pub struct PipelineReport {
    /// Directory being indexed
    pub directory: String,
    /// Stage metrics in order
    pub stages: Vec<StageMetrics>,
    /// Memory at start
    pub memory_start: MemorySnapshot,
    /// Memory at end (peak approximation)
    pub memory_end: MemorySnapshot,
    /// Total pipeline time
    pub total_time: Duration,
}

impl PipelineReport {
    /// Create a new report for a directory.
    pub fn new(directory: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
            stages: Vec::new(),
            memory_start: MemorySnapshot::current(),
            memory_end: MemorySnapshot::default(),
            total_time: Duration::ZERO,
        }
    }

    /// Add a stage's metrics.
    pub fn add_stage(&mut self, metrics: StageMetrics) {
        self.stages.push(metrics);
    }

    /// Finalize the report.
    pub fn finalize(&mut self, total_time: Duration) {
        self.total_time = total_time;
        self.memory_end = MemorySnapshot::current();
    }

    /// Identify the bottleneck stage (highest percentage of time).
    pub fn bottleneck(&self) -> Option<&StageMetrics> {
        self.stages
            .iter()
            .max_by(|a, b| a.wall_time.cmp(&b.wall_time))
    }

    /// Log the report using tracing.
    pub fn log(&self) {
        tracing::info!(target: "pipeline", "");
        tracing::info!(target: "pipeline", "========================================");
        tracing::info!(target: "pipeline", "PIPELINE TRACE: {}", self.directory);
        tracing::info!(target: "pipeline", "========================================");
        tracing::info!(target: "pipeline",
            "{:<10} {:>7} {:>10} {:>14} {:>12}",
            "Stage", "Threads", "Time", "Throughput", "Wait"
        );
        tracing::info!(target: "pipeline", "{}", "-".repeat(60));

        for stage in &self.stages {
            let throughput = if stage.items_processed > 0 {
                format!("{:.0}/s", stage.throughput())
            } else {
                "-".to_string()
            };

            let wait = stage.input_wait + stage.output_wait;
            let wait_str = if wait.as_millis() > 0 {
                format!("{:.1}s", wait.as_secs_f64())
            } else {
                "-".to_string()
            };

            tracing::info!(target: "pipeline",
                "{:<10} {:>7} {:>10} {:>14} {:>12}",
                stage.name,
                stage.threads,
                format!("{:.2}s", stage.wall_time.as_secs_f64()),
                throughput,
                wait_str
            );

            // Log secondary metric if present
            if !stage.secondary_label.is_empty() && stage.secondary_count > 0 {
                tracing::info!(target: "pipeline",
                    "           {:>7} {:>10}",
                    "",
                    format!("{} {}", stage.secondary_count, stage.secondary_label)
                );
            }
        }

        tracing::info!(target: "pipeline", "{}", "-".repeat(60));

        // Summary line
        let mem_delta = self.memory_end.rss.saturating_sub(self.memory_start.rss);
        let bottleneck = self
            .bottleneck()
            .map(|s| format!("{} ({:.0}%)", s.name, s.percentage_of(self.total_time)))
            .unwrap_or_else(|| "-".to_string());

        tracing::info!(target: "pipeline",
            "Total: {:.2}s | Memory: {} -> {} (+{}) | Bottleneck: {}",
            self.total_time.as_secs_f64(),
            self.memory_start.rss_human(),
            self.memory_end.rss_human(),
            format_bytes(mem_delta),
            bottleneck
        );
        tracing::info!(target: "pipeline", "");
    }
}

/// Shared metrics collector for pipeline-wide tracking.
#[derive(Debug)]
pub struct PipelineMetrics {
    enabled: bool,
    report: std::sync::Mutex<PipelineReport>,
}

impl PipelineMetrics {
    /// Create new pipeline metrics collector.
    pub fn new(directory: impl Into<String>, enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled,
            report: std::sync::Mutex::new(PipelineReport::new(directory)),
        })
    }

    /// Check if metrics collection is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Add stage metrics to the report.
    pub fn add_stage(&self, metrics: StageMetrics) {
        if self.enabled {
            if let Ok(mut report) = self.report.lock() {
                report.add_stage(metrics);
            }
        }
    }

    /// Finalize the report without logging.
    /// Use this when logging needs to be deferred (e.g., until StatusLine is dropped).
    pub fn finalize(&self, total_time: Duration) {
        if self.enabled {
            if let Ok(mut report) = self.report.lock() {
                report.finalize(total_time);
            }
        }
    }

    /// Log the finalized report.
    /// Call after StatusLine is dropped to avoid stderr race conditions.
    pub fn log(&self) {
        if self.enabled {
            if let Ok(report) = self.report.lock() {
                report.log();
            }
        }
    }

    /// Finalize and log the report.
    pub fn finalize_and_log(&self, total_time: Duration) {
        if self.enabled {
            if let Ok(mut report) = self.report.lock() {
                report.finalize(total_time);
                report.log();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_tracker() {
        let tracker = StageTracker::new("TEST", 4).with_secondary("symbols");

        tracker.record_items(100);
        tracker.record_secondary(500);
        tracker.record_input_wait(Duration::from_millis(50));

        std::thread::sleep(Duration::from_millis(10));

        let metrics = tracker.finalize();
        assert_eq!(metrics.name, "TEST");
        assert_eq!(metrics.threads, 4);
        assert_eq!(metrics.items_processed, 100);
        assert_eq!(metrics.secondary_count, 500);
        assert!(metrics.wall_time >= Duration::from_millis(10));
    }

    #[test]
    fn test_memory_snapshot() {
        let snapshot = MemorySnapshot::current();
        // Should have some memory usage
        assert!(snapshot.rss > 0);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(1500), "1.5KB");
        assert_eq!(format_bytes(1_500_000), "1.4MB");
        assert_eq!(format_bytes(1_500_000_000), "1.4GB");
    }
}
