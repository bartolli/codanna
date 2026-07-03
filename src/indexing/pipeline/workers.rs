//! READ and PARSE worker join helpers.

use super::Pipeline;
use super::types::PipelineError;
use std::thread;
use std::time::Duration;

/// Thread join handle type for READ workers.
/// Returns (files, errors, input_wait, output_wait, wall_time).
type ReadJoinHandle =
    thread::JoinHandle<Result<(usize, usize, Duration, Duration, Duration), PipelineError>>;

/// Thread join handle type for PARSE workers (with timing).
/// Returns (files, errors, symbols, input_wait, output_wait, wall_time).
type ParseJoinHandle = thread::JoinHandle<(usize, usize, usize, Duration, Duration, Duration)>;

impl Pipeline {
    /// Join READ worker threads and aggregate results.
    ///
    /// Returns (files_read, errors, total_input_wait, total_output_wait).
    /// Panicked threads are logged and counted as errors.
    pub(super) fn join_read_workers(
        &self,
        handles: Vec<ReadJoinHandle>,
    ) -> (usize, usize, Duration, Duration, Duration) {
        let mut files = 0;
        let mut errors = 0;
        let mut input_wait = Duration::ZERO;
        let mut output_wait = Duration::ZERO;
        let mut max_wall_time = Duration::ZERO;

        for handle in handles {
            match handle.join() {
                Ok(Ok((f, e, i, o, w))) => {
                    files += f;
                    errors += e;
                    input_wait += i;
                    output_wait += o;
                    // Use max wall_time (when last thread finished)
                    if w > max_wall_time {
                        max_wall_time = w;
                    }
                }
                Ok(Err(e)) => {
                    tracing::error!(target: "pipeline", "READ worker error: {e}");
                    errors += 1;
                }
                Err(_) => {
                    tracing::error!(target: "pipeline", "READ worker panicked");
                    errors += 1;
                }
            }
        }

        (files, errors, input_wait, output_wait, max_wall_time)
    }

    /// Join PARSE worker threads and aggregate results.
    ///
    /// Returns (files_parsed, errors, symbols, total_input_wait, total_output_wait, max_wall_time).
    /// Panicked threads are logged and counted as errors.
    pub(super) fn join_parse_workers(
        &self,
        handles: Vec<ParseJoinHandle>,
    ) -> (usize, usize, usize, Duration, Duration, Duration) {
        let mut files = 0;
        let mut errors = 0;
        let mut symbols = 0;
        let mut input_wait = Duration::ZERO;
        let mut output_wait = Duration::ZERO;
        let mut max_wall_time = Duration::ZERO;

        for handle in handles {
            match handle.join() {
                Ok((f, e, s, i, o, w)) => {
                    files += f;
                    errors += e;
                    symbols += s;
                    input_wait += i;
                    output_wait += o;
                    // Use max wall_time (when last thread finished)
                    if w > max_wall_time {
                        max_wall_time = w;
                    }
                }
                Err(_) => {
                    tracing::error!(target: "pipeline", "PARSE worker panicked");
                    errors += 1;
                }
            }
        }

        (
            files,
            errors,
            symbols,
            input_wait,
            output_wait,
            max_wall_time,
        )
    }
}
