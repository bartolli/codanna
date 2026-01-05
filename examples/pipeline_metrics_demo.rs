//! Pipeline metrics demo - simulates pipeline stages with metrics collection
//!
//! Run with: cargo run --example pipeline_metrics_demo
//!
//! This demonstrates:
//! - Stage timing and throughput tracking
//! - Channel wait time measurement
//! - Memory usage tracking via sysinfo
//! - Bottleneck identification

use codanna::indexing::pipeline::metrics::{MemorySnapshot, PipelineMetrics, StageTracker};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    // Initialize tracing to see the output
    tracing_subscriber::fmt()
        .with_target(true)
        .with_level(true)
        .with_env_filter("pipeline=info")
        .init();

    println!("Pipeline Metrics Demo");
    println!("=====================\n");

    // Demo 1: Basic stage tracking
    demo_stage_tracker();

    // Demo 2: Memory snapshot
    demo_memory_snapshot();

    // Demo 3: Full pipeline simulation
    demo_pipeline_simulation();
}

fn demo_stage_tracker() {
    println!("--- Demo 1: Stage Tracker ---");

    let tracker = StageTracker::new("PARSE", 4).with_secondary("symbols");

    // Simulate work
    for _ in 0..1000 {
        tracker.record_item();
        tracker.record_secondary(5); // 5 symbols per file
        thread::sleep(Duration::from_micros(100));
    }

    // Simulate some wait time
    tracker.record_input_wait(Duration::from_millis(50));
    tracker.record_output_wait(Duration::from_millis(30));

    let metrics = tracker.finalize();

    println!("Stage: {}", metrics.name);
    println!("Threads: {}", metrics.threads);
    println!("Items: {}", metrics.items_processed);
    println!(
        "Symbols: {} {}",
        metrics.secondary_count, metrics.secondary_label
    );
    println!("Wall time: {:.2}s", metrics.wall_time.as_secs_f64());
    println!("Active time: {:.2}s", metrics.active_time().as_secs_f64());
    println!("Throughput: {:.0} items/s", metrics.throughput());
    println!("Input wait: {:.3}s", metrics.input_wait.as_secs_f64());
    println!("Output wait: {:.3}s", metrics.output_wait.as_secs_f64());
    println!();
}

fn demo_memory_snapshot() {
    println!("--- Demo 2: Memory Snapshot ---");

    let before = MemorySnapshot::current();
    println!("Before allocation: RSS = {}", before.rss_human());

    // Allocate some memory
    let _data: Vec<u8> = vec![0u8; 50 * 1024 * 1024]; // 50MB

    let after = MemorySnapshot::current();
    println!("After 50MB allocation: RSS = {}", after.rss_human());

    let delta = after.rss.saturating_sub(before.rss);
    println!("Delta: +{:.1}MB", delta as f64 / (1024.0 * 1024.0));
    println!();
}

fn demo_pipeline_simulation() {
    println!("--- Demo 3: Pipeline Simulation ---");
    println!("Simulating: DISCOVER -> READ -> PARSE -> COLLECT -> INDEX");
    println!();

    let start = Instant::now();
    let metrics = PipelineMetrics::new("examples/typescript", true);

    // Stage 1: DISCOVER (fast, I/O bound)
    {
        let tracker = StageTracker::new("DISCOVER", 4);
        thread::sleep(Duration::from_millis(200));
        for _ in 0..1565 {
            tracker.record_item();
        }
        metrics.add_stage(tracker.finalize());
    }

    // Stage 2: READ (I/O bound, some backpressure)
    {
        let tracker = StageTracker::new("READ", 2).with_secondary("MB");
        thread::sleep(Duration::from_millis(500));
        tracker.record_items(1565);
        tracker.record_secondary(45); // 45MB read
        tracker.record_output_wait(Duration::from_millis(100)); // Channel was full
        metrics.add_stage(tracker.finalize());
    }

    // Stage 3: PARSE (CPU bound, the bottleneck)
    {
        let tracker = StageTracker::new("PARSE", 12).with_secondary("symbols");
        thread::sleep(Duration::from_millis(800)); // Slowest stage
        tracker.record_items(1565);
        tracker.record_secondary(45000); // 45K symbols
        tracker.record_input_wait(Duration::from_millis(50)); // Waiting for READ
        metrics.add_stage(tracker.finalize());
    }

    // Stage 4: COLLECT (single thread, fast)
    {
        let tracker = StageTracker::new("COLLECT", 1).with_secondary("batches");
        thread::sleep(Duration::from_millis(150));
        tracker.record_items(45000);
        tracker.record_secondary(9); // 9 batches
        metrics.add_stage(tracker.finalize());
    }

    // Stage 5: INDEX (I/O + CPU, Tantivy writes)
    {
        let tracker = StageTracker::new("INDEX", 1).with_secondary("commits");
        thread::sleep(Duration::from_millis(400));
        tracker.record_items(45000);
        tracker.record_secondary(3); // 3 commits
        tracker.record_input_wait(Duration::from_millis(80)); // Waiting for batches
        metrics.add_stage(tracker.finalize());
    }

    // Finalize and log the report
    metrics.finalize_and_log(start.elapsed());

    println!("\nDemo complete!");
}
