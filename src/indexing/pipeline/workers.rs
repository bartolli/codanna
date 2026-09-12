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
/// Returns (files, errors, symbols, input_wait, output_wait, wall_time,
/// first fatal parse-stage error).
type ParseJoinHandle = thread::JoinHandle<(
    usize,
    usize,
    usize,
    Duration,
    Duration,
    Duration,
    Option<PipelineError>,
)>;

impl Pipeline {
    /// Join READ worker threads and aggregate results.
    ///
    /// Returns (files_read, errors, total_input_wait, total_output_wait,
    /// max_wall_time, first fatal READ-stage infrastructure error).
    ///
    /// Ordinary per-file read failures remain recoverable and are counted in
    /// the error total. A worker-level error or panic is fatal because the
    /// channel may have been abandoned with unread paths, which would make a
    /// successful Phase 1 result silently incomplete.
    pub(super) fn join_read_workers(
        &self,
        handles: Vec<ReadJoinHandle>,
    ) -> (
        usize,
        usize,
        Duration,
        Duration,
        Duration,
        Option<PipelineError>,
    ) {
        let mut files = 0;
        let mut errors = 0;
        let mut input_wait = Duration::ZERO;
        let mut output_wait = Duration::ZERO;
        let mut max_wall_time = Duration::ZERO;
        let mut fatal_error = None;

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
                    if fatal_error.is_none() {
                        fatal_error = Some(e);
                    }
                }
                Err(_) => {
                    tracing::error!(target: "pipeline", "READ worker panicked");
                    errors += 1;
                    if fatal_error.is_none() {
                        fatal_error = Some(PipelineError::ChannelRecv(
                            "READ worker panicked".to_string(),
                        ));
                    }
                }
            }
        }

        (
            files,
            errors,
            input_wait,
            output_wait,
            max_wall_time,
            fatal_error,
        )
    }

    /// Join PARSE worker threads and aggregate results.
    ///
    /// Returns (files_parsed, errors, symbols, total_input_wait, total_output_wait, max_wall_time,
    /// first fatal parse-stage error across workers).
    ///
    /// Parser-construction failures and worker panics are surfaced through the
    /// final error slot. `run_phase1` checks that slot after INDEX/COLLECT have
    /// been joined and counters persisted, so a panicked parser can no longer
    /// leave a silently truncated index while the command reports success.
    pub(super) fn join_parse_workers(
        &self,
        handles: Vec<ParseJoinHandle>,
    ) -> (
        usize,
        usize,
        usize,
        Duration,
        Duration,
        Duration,
        Option<PipelineError>,
    ) {
        let mut files = 0;
        let mut errors = 0;
        let mut symbols = 0;
        let mut input_wait = Duration::ZERO;
        let mut output_wait = Duration::ZERO;
        let mut max_wall_time = Duration::ZERO;
        let mut fatal_error = None;

        for handle in handles {
            match handle.join() {
                Ok((f, e, s, i, o, w, c)) => {
                    files += f;
                    errors += e;
                    symbols += s;
                    input_wait += i;
                    output_wait += o;
                    // Use max wall_time (when last thread finished)
                    if w > max_wall_time {
                        max_wall_time = w;
                    }
                    if fatal_error.is_none() {
                        fatal_error = c;
                    }
                }
                Err(_) => {
                    tracing::error!(target: "pipeline", "PARSE worker panicked");
                    errors += 1;
                    if fatal_error.is_none() {
                        fatal_error = Some(PipelineError::ChannelRecv(
                            "PARSE worker panicked".to_string(),
                        ));
                    }
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
            fatal_error,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;
    use std::sync::Arc;

    #[test]
    fn hardening_read_worker_panic_is_reported_as_fatal_error() {
        let pipeline = Pipeline::with_settings(Arc::new(Settings::default()));
        let handle: ReadJoinHandle = thread::spawn(|| {
            panic!("synthetic read worker panic");
        });

        let (_, errors, _, _, _, fatal) = pipeline.join_read_workers(vec![handle]);

        assert_eq!(errors, 1);
        assert!(matches!(
            fatal,
            Some(PipelineError::ChannelRecv(message)) if message == "READ worker panicked"
        ));
    }

    #[test]
    fn hardening_parse_worker_panic_is_reported_as_fatal_error() {
        let pipeline = Pipeline::with_settings(Arc::new(Settings::default()));
        let handle: ParseJoinHandle = thread::spawn(|| {
            panic!("synthetic parser worker panic");
        });

        let (_, errors, _, _, _, _, fatal) = pipeline.join_parse_workers(vec![handle]);

        assert_eq!(errors, 1);
        assert!(matches!(
            fatal,
            Some(PipelineError::ChannelRecv(message)) if message == "PARSE worker panicked"
        ));
    }
}
