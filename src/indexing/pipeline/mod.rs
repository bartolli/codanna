//! Parallel indexing pipeline
//!
//! [PIPELINE API] A high-performance, multi-stage pipeline for indexing source code.
//!
//! ## Architecture
//!
//! ```text
//! DISCOVER → READ → PARSE → COLLECT → INDEX
//!    │         │       │        │        │
//!    ▼         ▼       ▼        ▼        ▼
//! [paths]  [content] [parsed] [batch]  Tantivy
//! ```
//!
//! ### Stage Overview
//!
//! - **DISCOVER**: Parallel file system walk, produces paths
//! - **READ**: Reads file contents, computes hashes
//! - **PARSE**: Parallel parsing with thread-local parsers
//! - **COLLECT**: Single-threaded ID assignment and batching
//! - **INDEX**: Writes batches to Tantivy
//!
//! ## Usage
//!
//! ```ignore
//! use codanna::indexing::pipeline::{Pipeline, PipelineConfig};
//!
//! let config = PipelineConfig::default();
//! let pipeline = Pipeline::new(settings, config);
//! let stats = pipeline.index_directory(path, &index)?;
//! ```

pub mod config;
pub mod stages;
pub mod types;

pub use config::PipelineConfig;
pub use stages::parse::{ParseStage, init_parser_cache, parse_file};
pub use types::{
    FileContent, FileRegistration, IndexBatch, ParsedFile, PipelineError, PipelineResult,
    RawImport, RawRelationship, RawSymbol, UnresolvedRelationship,
};

use crate::Settings;
use crate::indexing::IndexStats;
use crate::storage::DocumentIndex;
use crossbeam_channel::bounded;
use stages::{CollectStage, DiscoverStage, IndexStage, ReadStage};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

/// The parallel indexing pipeline.
///
/// [PIPELINE API] Orchestrates multiple stages to efficiently index source code
/// using all available CPU cores.
pub struct Pipeline {
    settings: Arc<Settings>,
    config: PipelineConfig,
}

impl Pipeline {
    /// Create a new pipeline with the given settings and configuration.
    pub fn new(settings: Arc<Settings>, config: PipelineConfig) -> Self {
        Self { settings, config }
    }

    /// Create a pipeline with configuration derived from settings.
    pub fn with_settings(settings: Arc<Settings>) -> Self {
        let config = PipelineConfig::from_settings(&settings);
        Self::new(settings, config)
    }

    /// Get the pipeline configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Get the settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Index a directory using the parallel pipeline (Phase 1).
    ///
    /// [PIPELINE API] This is the main entry point for indexing. It:
    /// 1. Discovers all source files (parallel walk)
    /// 2. Reads file contents (N threads)
    /// 3. Parses them in parallel (N threads)
    /// 4. Collects and assigns IDs (single thread)
    /// 5. Writes to Tantivy (single thread)
    ///
    /// Returns:
    /// - IndexStats: Statistics about the indexing operation
    /// - Vec<UnresolvedRelationship>: Pending references for Phase 2 resolution
    pub fn index_directory(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>)> {
        let start = Instant::now();

        // Create bounded channels with backpressure
        let (path_tx, path_rx) = bounded(self.config.path_channel_size);
        let (content_tx, content_rx) = bounded(self.config.content_channel_size);
        let (parsed_tx, parsed_rx) = bounded(self.config.parsed_channel_size);
        let (batch_tx, batch_rx) = bounded(self.config.batch_channel_size);

        // Clone settings for threads
        let settings = Arc::clone(&self.settings);
        let parse_threads = self.config.parse_threads;
        let read_threads = self.config.read_threads;
        let batch_size = self.config.batch_size;
        let batches_per_commit = self.config.batches_per_commit;

        // Stage 1: DISCOVER - parallel file walk
        let discover_root = root.to_path_buf();
        let discover_handle = thread::spawn(move || {
            let stage = DiscoverStage::new(discover_root, 4); // 4 walker threads
            stage.run(path_tx)
        });

        // Stage 2: READ - multi-threaded file reading
        let read_handles: Vec<_> = (0..read_threads)
            .map(|_| {
                let rx = path_rx.clone();
                let tx = content_tx.clone();
                thread::spawn(move || {
                    let stage = ReadStage::new(1);
                    stage.run(rx, tx)
                })
            })
            .collect();
        drop(path_rx); // Close original receiver
        drop(content_tx); // Close original sender after cloning

        // Stage 3: PARSE - parallel parsing with thread-local parsers
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|_| {
                let rx = content_rx.clone();
                let tx = parsed_tx.clone();
                let settings = Arc::clone(&settings);
                thread::spawn(move || {
                    // Initialize thread-local parser cache
                    init_parser_cache(settings.clone());

                    let stage = ParseStage::new(settings);
                    let mut parsed_count = 0;
                    let mut error_count = 0;

                    for content in rx {
                        match stage.parse(content) {
                            Ok(parsed) => {
                                parsed_count += 1;
                                if tx.send(parsed).is_err() {
                                    break; // Channel closed
                                }
                            }
                            Err(_e) => {
                                error_count += 1;
                                // Continue on parse errors - don't fail the whole batch
                            }
                        }
                    }

                    (parsed_count, error_count)
                })
            })
            .collect();
        drop(content_rx);
        drop(parsed_tx);

        // Stage 4: COLLECT - single-threaded ID assignment
        let collect_handle = thread::spawn(move || {
            let stage = CollectStage::new(batch_size);
            stage.run(parsed_rx, batch_tx)
        });

        // Stage 5: INDEX - single-threaded Tantivy writes
        let index_handle = thread::spawn(move || {
            let stage = IndexStage::new(index, batches_per_commit);
            stage.run(batch_rx)
        });

        // Wait for all stages to complete and collect results
        let discover_result = discover_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("DISCOVER thread panicked".to_string()))?;
        let files_discovered = discover_result?;

        let mut read_files = 0;
        let mut read_errors = 0;
        for handle in read_handles {
            if let Ok(Ok((files, errors))) = handle.join() {
                read_files += files;
                read_errors += errors;
            }
        }

        let mut parsed_files = 0;
        let mut parse_errors = 0;
        for handle in parse_handles {
            if let Ok((files, errors)) = handle.join() {
                parsed_files += files;
                parse_errors += errors;
            }
        }

        let collect_result = collect_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("COLLECT thread panicked".to_string()))?;
        let (_collected_files, _collected_symbols) = collect_result?;

        let index_result = index_handle
            .join()
            .map_err(|_| PipelineError::ChannelRecv("INDEX thread panicked".to_string()))?;
        let (mut stats, pending_relationships) = index_result?;

        // Update stats with timing
        stats.elapsed = start.elapsed();
        stats.files_failed = read_errors + parse_errors;

        tracing::info!(
            "[pipeline] Phase 1 complete: discovered={}, read={}, parsed={}, indexed={} files, {} symbols, {} pending refs in {:?}",
            files_discovered,
            read_files,
            parsed_files,
            stats.files_indexed,
            stats.symbols_found,
            pending_relationships.len(),
            stats.elapsed
        );

        Ok((stats, pending_relationships))
    }

    /// Index a directory with progress reporting (Phase 1).
    pub fn index_directory_with_progress(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
        _progress: bool,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>)> {
        // TODO: Add progress bar integration
        self.index_directory(root, index)
    }
}

/// Statistics from pipeline execution.
#[derive(Debug, Default)]
pub struct PipelineStats {
    /// Time spent in each stage (milliseconds)
    pub stage_times: StageTimings,
    /// Number of files processed
    pub files_processed: usize,
    /// Number of files that failed to parse
    pub files_failed: usize,
    /// Total symbols extracted
    pub symbols_extracted: usize,
    /// Total relationships found
    pub relationships_found: usize,
    /// Total relationships resolved
    pub relationships_resolved: usize,
    /// Peak memory usage (bytes)
    pub peak_memory: usize,
}

/// Timing breakdown by stage.
#[derive(Debug, Default)]
pub struct StageTimings {
    pub discover_ms: u64,
    pub read_ms: u64,
    pub parse_ms: u64,
    pub collect_ms: u64,
    pub index_ms: u64,
    pub resolve_ms: u64,
}

impl StageTimings {
    pub fn total_ms(&self) -> u64 {
        self.discover_ms
            + self.read_ms
            + self.parse_ms
            + self.collect_ms
            + self.index_ms
            + self.resolve_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn truncate(s: &str, max: usize) -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            format!("{}...", &s[..max - 3])
        }
    }

    #[test]
    fn test_pipeline_creation() {
        let settings = Arc::new(Settings::default());
        let pipeline = Pipeline::with_settings(settings);

        assert!(pipeline.config().parse_threads >= 1);
    }

    #[test]
    fn test_pipeline_with_custom_config() {
        let settings = Arc::new(Settings::default());
        let config = PipelineConfig::default().with_parse_threads(4);
        let pipeline = Pipeline::new(settings, config);

        assert_eq!(pipeline.config().parse_threads, 4);
    }

    /// End-to-end test proving Phase 1 collects symbols, imports, and pending relationships.
    ///
    /// Scenario: Two TypeScript files where file1 imports and calls file2.
    /// This demonstrates what Phase 1 produces for Phase 2 resolution:
    /// - Symbols with IDs
    /// - Imports (cross-file dependencies)
    /// - Pending relationships with from_id known, to_id unknown
    #[test]
    fn test_pipeline_end_to_end_proof() {
        use crate::storage::DocumentIndex;

        // Create temp directory with source files
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src dir");

        // File 2: utils.ts - exports helper functions
        let utils_content = r#"
// utils.ts - Helper utilities

export function formatName(first: string, last: string): string {
    return `${first} ${last}`;
}

export function validateEmail(email: string): boolean {
    return email.includes("@");
}

export class StringUtils {
    static capitalize(s: string): string {
        return s.charAt(0).toUpperCase() + s.slice(1);
    }

    static lowercase(s: string): string {
        return s.toLowerCase();
    }
}
"#;
        fs::write(src_dir.join("utils.ts"), utils_content).expect("Failed to write utils.ts");

        // File 1: main.ts - imports and calls functions from utils.ts
        let main_content = r#"
// main.ts - Entry point, imports from utils

import { formatName, validateEmail } from "./utils";
import { StringUtils } from "./utils";

function processUser(first: string, last: string, email: string): string {
    // Cross-file call: formatName is defined in utils.ts
    const fullName = formatName(first, last);

    // Cross-file call: validateEmail is defined in utils.ts
    if (!validateEmail(email)) {
        throw new Error("Invalid email");
    }

    // Cross-file static method call
    return StringUtils.capitalize(fullName);
}

function main(): void {
    const result = processUser("john", "doe", "john@example.com");
    console.log(result);
}

export { processUser, main };
"#;
        fs::write(src_dir.join("main.ts"), main_content).expect("Failed to write main.ts");

        // Create index in temp location
        let index_dir = temp_dir.path().join("index");
        fs::create_dir_all(&index_dir).expect("Failed to create index dir");

        // Create pipeline with settings
        let settings = Settings::default();
        let index = DocumentIndex::new(&index_dir, &settings).expect("Failed to create index");
        let index = Arc::new(index);
        let settings = Arc::new(settings);
        let pipeline = Pipeline::with_settings(settings);

        // Run Phase 1
        let result = pipeline.index_directory(&src_dir, index);

        match result {
            Ok((stats, pending_relationships)) => {
                // Categorize relationships by kind
                let calls: Vec<_> = pending_relationships
                    .iter()
                    .filter(|r| matches!(r.kind, crate::RelationKind::Calls))
                    .collect();
                let uses: Vec<_> = pending_relationships
                    .iter()
                    .filter(|r| matches!(r.kind, crate::RelationKind::Uses))
                    .collect();

                // Print comprehensive Phase 1 output
                println!("\n================================================================");
                println!("PIPELINE PHASE 1 OUTPUT -> INPUT FOR PHASE 2");
                println!("================================================================");
                println!();
                println!("INDEXED DATA (in Tantivy):");
                println!("  Files indexed:        {}", stats.files_indexed);
                println!("  Symbols found:        {}", stats.symbols_found);
                println!("  Time elapsed:         {:?}", stats.elapsed);
                println!();
                println!("PENDING FOR PHASE 2 RESOLUTION:");
                println!("  Total relationships:  {}", pending_relationships.len());
                println!("    - Calls:            {}", calls.len());
                println!("    - Uses:             {}", uses.len());
                println!();

                // Show cross-file calls (the key scenario)
                println!("CROSS-FILE CALLS (Phase 2 must resolve to_id):");
                println!("----------------------------------------------------------------");
                println!(
                    "  {:20} {:20} {:8} {:8} {:12}",
                    "FROM", "TO", "from_id", "file_id", "call_site"
                );
                println!(
                    "  {:20} {:20} {:8} {:8} {:12}",
                    "----", "--", "-------", "-------", "---------"
                );
                for rel in calls.iter().take(15) {
                    let range_info = rel
                        .to_range
                        .as_ref()
                        .map(|r| format!("{}:{}", r.start_line, r.start_column))
                        .unwrap_or_else(|| "-".to_string());

                    println!(
                        "  {:20} {:20} {:8} {:8} {:12}",
                        truncate(&rel.from_name, 20),
                        truncate(&rel.to_name, 20),
                        rel.from_id.map(|id| id.value()).unwrap_or(0),
                        rel.file_id.value(),
                        range_info
                    );
                }
                if calls.len() > 15 {
                    println!("  ... and {} more calls", calls.len() - 15);
                }
                println!();

                // Show what Phase 2 needs to do
                println!("PHASE 2 TASK:");
                println!("  For each pending relationship:");
                println!("    1. from_id is KNOWN (assigned by COLLECT)");
                println!("    2. to_id is UNKNOWN (needs resolution)");
                println!("    3. Use imports + symbol cache + to_range for disambiguation");
                println!("================================================================\n");

                // Assertions
                assert_eq!(stats.files_indexed, 2, "Expected exactly 2 files indexed");
                assert!(
                    stats.symbols_found >= 6,
                    "Expected at least 6 symbols (functions + class + methods)"
                );
                assert!(!calls.is_empty(), "Expected cross-file call relationships");

                // Verify range data for disambiguation
                let with_range = pending_relationships
                    .iter()
                    .filter(|r| r.to_range.is_some())
                    .count();
                let with_from_id = pending_relationships
                    .iter()
                    .filter(|r| r.from_id.is_some())
                    .count();

                println!("Resolution readiness:");
                println!(
                    "  - with from_id:  {}/{}",
                    with_from_id,
                    pending_relationships.len()
                );
                println!(
                    "  - with to_range: {}/{}",
                    with_range,
                    pending_relationships.len()
                );

                assert!(
                    with_from_id > 0,
                    "Expected from_id to be populated by COLLECT stage"
                );
                assert!(
                    with_range > 0,
                    "Expected to_range for Phase 2 disambiguation"
                );
            }
            Err(e) => {
                panic!("Pipeline failed: {e:?}");
            }
        }
    }
}
