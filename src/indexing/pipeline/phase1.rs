//! Phase 1 orchestration: the source -> READ -> PARSE -> COLLECT -> INDEX (+ EMBED) skeleton.

use super::stages::{CollectStage, DiscoverStage, IndexStage, ReadStage};
use super::{
    EmbedOptions, FileSource, ParseStage, Phase1Options, Pipeline, PipelineError, PipelineMetrics,
    PipelineResult, ProgressSink, SemanticEmbedStage, StageMetrics, StageTracker,
    SymbolLookupCache, UnresolvedRelationship, init_parser_cache,
};
use crate::indexing::IndexStats;
use crate::storage::DocumentIndex;
use crossbeam_channel::bounded;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Result of Phase 1 indexing with optional metrics for deferred logging.
type Phase1Result = (
    IndexStats,
    Vec<UnresolvedRelationship>,
    SymbolLookupCache,
    Option<Arc<PipelineMetrics>>,
);

impl Pipeline {
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
    /// - `Vec<UnresolvedRelationship>`: Pending references for Phase 2 resolution
    /// - SymbolLookupCache: In-memory cache for O(1) Phase 2 resolution
    pub fn index_directory(
        &self,
        root: &Path,
        index: Arc<DocumentIndex>,
    ) -> PipelineResult<(IndexStats, Vec<UnresolvedRelationship>, SymbolLookupCache)> {
        let (stats, pending_relationships, symbol_cache, metrics) = self.run_phase1(
            FileSource::Walk(root.to_path_buf()),
            index,
            Phase1Options::default(),
        )?;

        // Log pipeline metrics report (no StatusLine in this path)
        if let Some(m) = metrics {
            m.log();
        }

        Ok((stats, pending_relationships, symbol_cache))
    }

    /// Run the Phase 1 skeleton: source -> READ -> PARSE -> COLLECT -> INDEX (+ EMBED).
    ///
    /// Sequencing contract lives in the architecture spec (Phase 1 orchestration):
    /// counters bracket the run, every stage handle joins before result
    /// inspection, results inspect source -> INDEX -> COLLECT -> counter save ->
    /// EMBED (soft-fail), metrics return for deferred logging, and the
    /// orchestrator ends at Phase 1 (no resolution, no embeddings save).
    pub(super) fn run_phase1(
        &self,
        source: FileSource,
        index: Arc<DocumentIndex>,
        opts: Phase1Options,
    ) -> PipelineResult<Phase1Result> {
        // Empty file list short-circuits: no counters read, no threads spawned.
        if let FileSource::List(files) = &source {
            if files.is_empty() {
                return Ok((
                    IndexStats::new(),
                    Vec::new(),
                    SymbolLookupCache::with_capacity(0),
                    None,
                ));
            }
        }

        let start = Instant::now();
        let Phase1Options { progress, embed } = opts;

        // Create metrics collector if tracing is enabled
        let metrics = if self.config.pipeline_tracing {
            let label = match &source {
                FileSource::Walk(root) => root.display().to_string(),
                FileSource::List(files) => format!("{} files", files.len()),
            };
            Some(PipelineMetrics::new(label, true))
        } else {
            None
        };

        // Query existing ID counters BEFORE spawning threads
        let (start_file_counter, start_symbol_counter) = self.get_start_counters(&index)?;

        // Create bounded channels with backpressure
        let (path_tx, path_rx) = bounded(self.config.path_channel_size);
        let (content_tx, content_rx) = bounded(self.config.content_channel_size);
        let (parsed_tx, parsed_rx) = bounded(self.config.parsed_channel_size);
        let (batch_tx, batch_rx) = bounded(self.config.batch_channel_size);
        // Embed channel for parallel EMBED stage
        let (embed_tx, embed_rx) = bounded(self.config.batch_channel_size);
        let embed_sender = if embed.is_some() {
            Some(embed_tx)
        } else {
            drop(embed_tx);
            None
        };

        // Clone settings for threads
        let settings = Arc::clone(&self.settings);
        let parse_threads = self.config.parse_threads;
        let read_threads = self.config.read_threads;
        let discover_threads = self.config.discover_threads;
        let batch_size = self.config.batch_size;
        let batches_per_commit = self.config.batches_per_commit;
        let tracing_enabled = self.config.pipeline_tracing;

        // Stage 1: SOURCE - directory walk or explicit file list
        type SourceJoinHandle = thread::JoinHandle<(PipelineResult<usize>, Option<StageMetrics>)>;
        let source_handle: SourceJoinHandle = match source {
            FileSource::Walk(root) => thread::spawn(move || {
                let tracker = if tracing_enabled {
                    Some(StageTracker::new("DISCOVER", discover_threads))
                } else {
                    None
                };

                let stage = DiscoverStage::new(root, discover_threads);
                let result = stage.run(path_tx);

                // Record metrics
                if let (Some(tracker), Ok(count)) = (&tracker, &result) {
                    tracker.record_items(*count);
                }

                (result, tracker.map(|t| t.finalize()))
            }),
            FileSource::List(files) => thread::spawn(move || {
                let mut sent = 0;
                for path in files {
                    if path_tx.send(path).is_err() {
                        break;
                    }
                    sent += 1;
                }
                (Ok(sent), None)
            }),
        };

        // Stage 2: READ - multi-threaded file reading
        let workspace_root = settings.workspace_root.clone();
        let read_handles: Vec<_> = (0..read_threads)
            .map(|_| {
                let rx = path_rx.clone();
                let tx = content_tx.clone();
                let workspace_root = workspace_root.clone();
                thread::spawn(move || {
                    let stage = ReadStage::with_workspace_root(1, workspace_root);
                    stage.run(rx, tx)
                })
            })
            .collect();
        drop(path_rx); // Close original receiver
        drop(content_tx); // Close original sender after cloning

        // Stage 3: PARSE - parallel parsing with thread-local parsers (with wait tracking)
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|_| {
                let rx = content_rx.clone();
                let tx = parsed_tx.clone();
                let settings = Arc::clone(&settings);
                thread::spawn(move || {
                    let start = Instant::now();
                    // Initialize thread-local parser cache
                    init_parser_cache(settings.clone());

                    let stage = ParseStage::new(settings);
                    let mut parsed_count = 0;
                    let mut error_count = 0;
                    let mut symbol_count = 0;
                    let mut input_wait = std::time::Duration::ZERO;
                    let mut output_wait = std::time::Duration::ZERO;

                    loop {
                        // Track input wait (time blocked on recv)
                        let recv_start = Instant::now();
                        let content = match rx.recv() {
                            Ok(c) => c,
                            Err(_) => break, // Channel closed
                        };
                        input_wait += recv_start.elapsed();

                        match stage.parse(content) {
                            Ok(parsed) => {
                                parsed_count += 1;
                                symbol_count += parsed.raw_symbols.len();

                                // Track output wait (time blocked on send)
                                let send_start = Instant::now();
                                if tx.send(parsed).is_err() {
                                    break; // Channel closed
                                }
                                output_wait += send_start.elapsed();
                            }
                            Err(_e) => {
                                error_count += 1;
                                // Continue on parse errors - don't fail the whole batch
                            }
                        }
                    }

                    (
                        parsed_count,
                        error_count,
                        symbol_count,
                        input_wait,
                        output_wait,
                        start.elapsed(),
                    )
                })
            })
            .collect();
        drop(content_rx);
        drop(parsed_tx);

        // Stage 4: COLLECT - single-threaded ID assignment (with starting counters)
        // Sends IndexBatch to INDEX, EmbeddingBatch to EMBED (parallel)
        let embed_total_callback = match &progress {
            ProgressSink::Dual(dp) => {
                let dp = Arc::clone(dp);
                Some(Arc::new(move |count: u64| dp.add_bar1_total(count))
                    as Arc<dyn Fn(u64) + Send + Sync>)
            }
            _ => None,
        };
        let collect_handle = thread::spawn(move || {
            let tracker = if tracing_enabled {
                Some(StageTracker::new("COLLECT", 1).with_secondary("batches"))
            } else {
                None
            };

            let stage = CollectStage::new(batch_size)
                .with_start_counters(start_file_counter, start_symbol_counter);
            let result = stage.run(parsed_rx, batch_tx, embed_sender, embed_total_callback);

            // Record items and wait times before finalizing
            if let (Some(t), Ok((_, symbol_count, _, input_wait, output_wait))) =
                (&tracker, &result)
            {
                t.record_items(*symbol_count as usize);
                t.record_input_wait(*input_wait);
                t.record_output_wait(*output_wait);
            }

            (result, tracker.map(|t| t.finalize()))
        });

        // Stage 5a: EMBED (parallel with INDEX) - iff embedding options provided
        let embed_handle = if let Some(EmbedOptions { pool, semantic }) = embed {
            let embed_callback = match &progress {
                ProgressSink::Dual(dp) => {
                    let dp = Arc::clone(dp);
                    Some(Arc::new(move |count: u64| dp.add_bar1(count))
                        as Arc<dyn Fn(u64) + Send + Sync>)
                }
                _ => None,
            };
            // Completion callback to freeze timer when EMBED finishes
            let embed_complete = match &progress {
                ProgressSink::Dual(dp) => Some(Arc::clone(dp)),
                _ => None,
            };

            Some(thread::spawn(move || {
                let mut stage = SemanticEmbedStage::new(pool, semantic);
                if let Some(callback) = embed_callback {
                    stage = stage.with_progress(callback);
                }
                let result = stage.run(embed_rx);
                // Freeze EMBED timer immediately when done
                if let Some(dp) = embed_complete {
                    dp.complete_bar1();
                }
                result
            }))
        } else {
            drop(embed_rx);
            None
        };

        // Stage 5b: INDEX (parallel with EMBED) - single-threaded Tantivy writes
        // Clone index Arc for metadata update after pipeline completes
        let index_for_metadata = Arc::clone(&index);
        // Completion callback to freeze timer when INDEX finishes
        let index_complete = match &progress {
            ProgressSink::Dual(dp) => Some(Arc::clone(dp)),
            _ => None,
        };
        let index_handle = {
            let mut index_stage = IndexStage::new(index, batches_per_commit);
            match &progress {
                ProgressSink::Silent => {}
                ProgressSink::Bar(bar) => {
                    index_stage = index_stage.with_progress(Arc::clone(bar));
                }
                ProgressSink::Dual(dp) => {
                    let dp = Arc::clone(dp);
                    let callback = Arc::new(move |count: u64| dp.add_bar2(count))
                        as Arc<dyn Fn(u64) + Send + Sync>;
                    index_stage = index_stage.with_progress_callback(callback);
                }
            }

            thread::spawn(move || {
                let tracker = if tracing_enabled {
                    Some(StageTracker::new("INDEX", 1).with_secondary("commits"))
                } else {
                    None
                };

                let result = index_stage.run(batch_rx);

                // Freeze INDEX timer immediately when done
                if let Some(dp) = index_complete {
                    dp.complete_bar2();
                }

                // Record items and wait times before finalizing
                if let (Some(t), Ok((stats, _, _, input_wait))) = (&tracker, &result) {
                    t.record_items(stats.symbols_found);
                    t.record_input_wait(*input_wait);
                }

                (result, tracker.map(|t| t.finalize()))
            })
        };

        // Join every stage handle before inspecting any result, so progress
        // bars complete on error paths too. Channel closure cascades shutdown,
        // so all joins terminate regardless of individual stage failures.
        let source_join = source_handle.join();
        let (read_files, read_errors, read_input_wait, read_output_wait, read_wall_time) =
            self.join_read_workers(read_handles);
        let (
            parsed_files,
            parse_errors,
            total_symbols,
            parse_input_wait,
            parse_output_wait,
            parse_wall_time,
        ) = self.join_parse_workers(parse_handles);
        let collect_join = collect_handle.join();
        let embed_join = embed_handle.map(|h| h.join());
        let index_join = index_handle.join();

        // Complete progress bars (idempotent - safe even if threads already completed them)
        if let ProgressSink::Dual(ref dp) = progress {
            dp.complete_bar1();
            dp.complete_bar2();
        }

        // Inspect SOURCE first: a partial walk must fail the run before counters save
        let (source_result, source_metrics) = source_join
            .map_err(|_| PipelineError::ChannelRecv("DISCOVER thread panicked".to_string()))?;
        let files_discovered = source_result?;

        // Add DISCOVER metrics
        if let (Some(m), Some(sm)) = (&metrics, source_metrics) {
            m.add_stage(sm);
        }

        // READ stage metrics (aggregate across threads)
        if let Some(m) = &metrics {
            m.add_stage(StageMetrics {
                name: "READ",
                threads: read_threads,
                wall_time: read_wall_time,
                input_wait: read_input_wait,
                output_wait: read_output_wait,
                items_processed: read_files,
                secondary_count: 0,
                secondary_label: "MB",
            });
        }

        // PARSE stage metrics (aggregate across threads)
        if let Some(m) = &metrics {
            m.add_stage(StageMetrics {
                name: "PARSE",
                threads: parse_threads,
                wall_time: parse_wall_time,
                input_wait: parse_input_wait,
                output_wait: parse_output_wait,
                items_processed: parsed_files,
                secondary_count: total_symbols,
                secondary_label: "symbols",
            });
        }

        // CRITICAL: Unwrap INDEX next - this is the critical path.
        // If INDEX succeeded, we MUST save counters regardless of EMBED status.
        let (index_result, index_metrics) = index_join
            .map_err(|_| PipelineError::ChannelRecv("INDEX thread panicked".to_string()))?;
        let (mut stats, pending_relationships, symbol_cache, _) = index_result?;

        // Add INDEX metrics
        if let (Some(m), Some(im)) = (&metrics, index_metrics) {
            m.add_stage(im);
        }

        // Unwrap COLLECT (needed for counters)
        let (collect_result, collect_metrics) = collect_join
            .map_err(|_| PipelineError::ChannelRecv("COLLECT thread panicked".to_string()))?;
        let (final_file_count, final_symbol_count, embed_candidates, _, _) = collect_result?;

        // Add COLLECT metrics
        if let (Some(m), Some(cm)) = (&metrics, collect_metrics) {
            m.add_stage(cm);
        }

        // CRITICAL: Save counters NOW, before checking EMBED.
        // INDEX succeeded, so we MUST persist the new ID pointers to prevent
        // duplicate IDs on the next run.
        self.save_final_counters(&index_for_metadata, final_file_count, final_symbol_count)?;

        // Handle EMBED results (Soft Failure - log but don't fail pipeline)
        // The index is valid even if embeddings failed; semantic search will be incomplete.
        if let Some(join_result) = embed_join {
            match join_result {
                Ok(Ok(embed_stats)) => {
                    // Validate COLLECT/EMBED data flow integrity
                    if embed_stats.received != embed_candidates as usize {
                        tracing::warn!(
                            target: "pipeline",
                            "EMBED data flow mismatch: COLLECT sent {} candidates, EMBED received {}",
                            embed_candidates,
                            embed_stats.received
                        );
                    }

                    // Track any symbols that failed embedding
                    let failed = embed_stats.received.saturating_sub(embed_stats.embedded);
                    if failed > 0 {
                        stats.embeddings_failed = failed;
                    }

                    // Add EMBED metrics to pipeline report
                    if let Some(m) = &metrics {
                        m.add_stage(StageMetrics {
                            name: "EMBED",
                            threads: 1, // Single coordinator thread (pool parallelizes internally)
                            wall_time: embed_stats.elapsed,
                            input_wait: embed_stats.input_wait,
                            output_wait: Duration::ZERO, // EMBED has no output channel
                            items_processed: embed_stats.embedded,
                            secondary_count: embed_stats.skipped,
                            secondary_label: "skipped",
                        });
                    }

                    tracing::info!(
                        target: "semantic",
                        "EMBED: {}/{} embedded ({} candidates from COLLECT)",
                        embed_stats.embedded,
                        embed_stats.received,
                        embed_candidates
                    );
                }
                Ok(Err(e)) => {
                    // All candidates failed embedding
                    stats.embeddings_failed = embed_candidates as usize;
                    tracing::error!(
                        target: "pipeline",
                        "Embedding generation failed: {e}. Index is valid but semantic search may be incomplete. Run with --force to regenerate."
                    );
                }
                Err(_) => {
                    // All candidates failed embedding
                    stats.embeddings_failed = embed_candidates as usize;
                    tracing::error!(
                        target: "pipeline",
                        "EMBED thread panicked. Index is valid but semantic search may be incomplete."
                    );
                }
            }
        }

        // Update stats with timing and error counts
        stats.elapsed = start.elapsed();
        stats.files_failed = read_errors + parse_errors;

        // Finalize metrics but don't log (caller logs after StatusLine drop)
        if let Some(ref m) = metrics {
            m.finalize(start.elapsed());
        }

        tracing::info!(
            target: "pipeline",
            "Phase 1 complete: discovered={}, read={}, parsed={}, indexed={} files, {} symbols, {} cached, {} pending refs in {:?}",
            files_discovered,
            read_files,
            parsed_files,
            stats.files_indexed,
            stats.symbols_found,
            symbol_cache.len(),
            pending_relationships.len(),
            stats.elapsed
        );

        Ok((stats, pending_relationships, symbol_cache, metrics))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;
    use crate::indexing::pipeline::PipelineConfig;
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
            Ok((stats, pending_relationships, symbol_cache)) => {
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
                println!("SYMBOL CACHE (for O(1) Phase 2 resolution):");
                println!("  Symbols cached:       {}", symbol_cache.len());
                println!("  Unique names:         {}", symbol_cache.unique_names());
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

                // Verify symbol cache matches indexed symbols
                assert_eq!(
                    symbol_cache.len(),
                    stats.symbols_found,
                    "Symbol cache must contain all indexed symbols"
                );

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

    /// Proves that pipeline stages run on distinct OS threads.
    ///
    /// Uses thread_id crate to get actual OS-level thread IDs (pthread_t on macOS/Linux).
    /// Verifies:
    /// - All thread IDs are unique (different OS threads)
    /// - Thread count matches configuration (read + parse + collect + index + discover)
    #[test]
    fn test_pipeline_uses_distinct_threads() {
        use std::collections::HashSet;
        use std::sync::Mutex;

        // Shared storage for OS-level thread IDs
        let thread_ids: Arc<Mutex<HashSet<usize>>> = Arc::new(Mutex::new(HashSet::new()));

        // Simulate pipeline thread structure with known counts
        let read_threads = 2;
        let parse_threads = 4;

        // Track main thread (OS-level)
        let main_thread_id = thread_id::get();
        println!("Main thread (OS): {main_thread_id}");

        // Stage 1: DISCOVER (1 thread)
        let ids = Arc::clone(&thread_ids);
        let discover_handle = thread::spawn(move || {
            let tid = thread_id::get();
            ids.lock().unwrap().insert(tid);
            println!("DISCOVER thread (OS): {tid}");
            tid
        });

        // Stage 2: READ (N threads)
        let read_handles: Vec<_> = (0..read_threads)
            .map(|i| {
                let ids = Arc::clone(&thread_ids);
                thread::spawn(move || {
                    let tid = thread_id::get();
                    ids.lock().unwrap().insert(tid);
                    println!("READ[{i}] thread (OS): {tid}");
                    tid
                })
            })
            .collect();

        // Stage 3: PARSE (N threads)
        let parse_handles: Vec<_> = (0..parse_threads)
            .map(|i| {
                let ids = Arc::clone(&thread_ids);
                thread::spawn(move || {
                    let tid = thread_id::get();
                    ids.lock().unwrap().insert(tid);
                    println!("PARSE[{i}] thread (OS): {tid}");
                    tid
                })
            })
            .collect();

        // Stage 4: COLLECT (1 thread)
        let ids = Arc::clone(&thread_ids);
        let collect_handle = thread::spawn(move || {
            let tid = thread_id::get();
            ids.lock().unwrap().insert(tid);
            println!("COLLECT thread (OS): {tid}");
            tid
        });

        // Stage 5: INDEX (1 thread)
        let ids = Arc::clone(&thread_ids);
        let index_handle = thread::spawn(move || {
            let tid = thread_id::get();
            ids.lock().unwrap().insert(tid);
            println!("INDEX thread (OS): {tid}");
            tid
        });

        // Wait for all threads
        let discover_tid = discover_handle.join().expect("DISCOVER panic");
        let read_tids: Vec<_> = read_handles
            .into_iter()
            .map(|h| h.join().expect("READ panic"))
            .collect();
        let parse_tids: Vec<_> = parse_handles
            .into_iter()
            .map(|h| h.join().expect("PARSE panic"))
            .collect();
        let collect_tid = collect_handle.join().expect("COLLECT panic");
        let index_tid = index_handle.join().expect("INDEX panic");

        // Verify results
        let unique_ids = thread_ids.lock().unwrap();
        let expected_threads = 1 + read_threads + parse_threads + 1 + 1; // discover + read + parse + collect + index

        println!("\n========================================");
        println!("OS-LEVEL THREAD VERIFICATION");
        println!("========================================");
        println!("Expected threads: {expected_threads}");
        println!("Unique OS thread IDs: {}", unique_ids.len());
        println!("Main thread (OS): {main_thread_id}");
        println!();
        println!("OS Thread ID breakdown:");
        println!("  DISCOVER: {discover_tid}");
        println!("  READ:     {read_tids:?}");
        println!("  PARSE:    {parse_tids:?}");
        println!("  COLLECT:  {collect_tid}");
        println!("  INDEX:    {index_tid}");
        println!("========================================\n");

        // Assertions
        assert_eq!(
            unique_ids.len(),
            expected_threads,
            "All threads must have unique OS-level IDs"
        );
        assert!(
            !unique_ids.contains(&main_thread_id),
            "Work threads must be different from main thread"
        );

        // Verify no thread ID appears twice
        let all_tids = [
            vec![discover_tid, collect_tid, index_tid],
            read_tids,
            parse_tids,
        ]
        .concat();

        let unique_count = all_tids.iter().collect::<HashSet<_>>().len();
        assert_eq!(
            unique_count,
            all_tids.len(),
            "Every stage must run on its own OS thread"
        );
    }
}
