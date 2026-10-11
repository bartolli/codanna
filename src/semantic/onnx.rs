//! One process-wide ONNX Runtime thread pool for local fastembed sessions.
//!
//! fastembed 5.6 sizes every `TextEmbedding` session to all logical CPUs
//! with spin-waiting on and exposes no per-session thread setting. With
//! `embedding_threads` pool instances that is `embedding_threads x ncpu`
//! busy threads during embedding, whatever `indexing.parallelism` says.
//!
//! ort disables per-session pools for every session created after an
//! environment with a global pool is committed, and the environment is
//! one-shot per process: a commit that arrives after the first session
//! changes nothing. `new_text_embedding` is therefore the only session
//! constructor in this crate. It commits the pool first, so a new
//! session-creation site cannot skip the cap.

use std::sync::OnceLock;

use fastembed::{TextEmbedding, TextInitOptions};
use ort::environment::GlobalThreadPoolOptions;

/// The intra-op cap the environment was committed with. Set once by the
/// first session; later callers with a different cap keep this one.
static COMMITTED_CAP: OnceLock<usize> = OnceLock::new();

fn thread_ceiling() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

fn clamp_threads(requested: usize, ceiling: usize) -> usize {
    requested.clamp(1, ceiling.max(1))
}

/// Clamp a configured `embedding_threads` to `1..=available_parallelism`,
/// warning when the value changes. ORT takes the count as a C `int`, so an
/// unbounded value could wrap to 0, which ORT reads as "all CPUs".
pub(crate) fn clamp_embedding_threads(requested: usize) -> usize {
    let ceiling = thread_ceiling();
    let clamped = clamp_threads(requested, ceiling);
    if clamped != requested {
        tracing::warn!(
            target: "semantic",
            "semantic_search.embedding_threads = {requested} is outside 1..={ceiling}; using {clamped}"
        );
    }
    clamped
}

/// Create a fastembed text session inside the shared, capped ONNX pool.
///
/// The first call in a process commits the environment: `embedding_threads`
/// (clamped) intra-op threads, one inter-op thread, spin-waiting off. The
/// pool is shared by every session, so about `2N-1` threads are busy at
/// peak: `N-1` pool threads plus the `N` workers calling into it.
///
/// Degrades, never fails: a commit that finds the environment already
/// created, or that ORT rejects, is logged at warn and the session is still
/// built with the per-session pool ORT would have given it anyway.
pub(crate) fn new_text_embedding(
    options: TextInitOptions,
    embedding_threads: usize,
) -> Result<TextEmbedding, fastembed::Error> {
    let cap = clamp_embedding_threads(embedding_threads);
    let committed = *COMMITTED_CAP.get_or_init(|| {
        commit_global_pool(cap);
        cap
    });
    if committed != cap {
        tracing::debug!(
            target: "semantic",
            "ONNX Runtime pool already committed with {committed} threads; {cap} requested, keeping {committed}"
        );
    }
    TextEmbedding::try_new(session_options(options, cap))
}

/// Per-session settings for the case the global pool did not apply (an
/// environment committed elsewhere, or a commit ORT rejected): `cap`
/// intra-op threads with spin-waiting off. With the global pool in place
/// ORT disables per-session threads and these are inert.
fn session_options(options: TextInitOptions, cap: usize) -> TextInitOptions {
    options
        .with_intra_threads(cap)
        .with_session_config("session.intra_op.allow_spinning", "0")
}

fn commit_global_pool(intra_threads: usize) {
    let committed = GlobalThreadPoolOptions::default()
        .with_intra_threads(intra_threads)
        .and_then(|o| o.with_inter_threads(1))
        .and_then(|o| o.with_spin_control(false))
        .map(|o| ort::init().with_global_thread_pool(o).commit());
    match committed {
        Ok(true) => tracing::debug!(
            target: "semantic",
            "ONNX Runtime global thread pool: {intra_threads} intra-op threads, spinning off"
        ),
        Ok(false) => tracing::warn!(
            target: "semantic",
            "ONNX Runtime environment already initialized; global pool of {intra_threads} not applied, sessions run capped per-session pools"
        ),
        Err(e) => tracing::warn!(
            target: "semantic",
            "ONNX Runtime global thread pool not committed ({e}); sessions run capped per-session pools"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_options_carry_the_cap_and_disable_spinning() {
        let options = session_options(
            TextInitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2),
            3,
        );
        assert_eq!(options.intra_threads, Some(3));
        assert!(
            options
                .session_config
                .iter()
                .any(|(k, v)| k == "session.intra_op.allow_spinning" && v == "0")
        );
    }

    #[test]
    fn clamp_raises_zero_to_one() {
        assert_eq!(clamp_threads(0, 8), 1);
    }

    #[test]
    fn clamp_caps_at_the_ceiling() {
        assert_eq!(clamp_threads(9, 8), 8);
        assert_eq!(clamp_threads(usize::MAX, 8), 8);
    }

    #[test]
    fn clamp_keeps_an_in_range_value() {
        assert_eq!(clamp_threads(3, 8), 3);
    }

    #[test]
    fn clamp_tolerates_a_zero_ceiling() {
        assert_eq!(clamp_threads(5, 0), 1);
    }

    // Lock for the single-constructor invariant: a session built anywhere
    // else bypasses the cap with only a tracing warning to show for it.
    #[test]
    fn try_new_is_called_only_by_the_constructor() {
        // No trailing paren: `try_new_from_user_defined` builds a session too.
        let needle = concat!("TextEmbedding::", "try_new");
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits = Vec::new();
        for entry in walkdir::WalkDir::new(&src)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            for (i, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    let rel = entry.path().strip_prefix(&src).unwrap().to_path_buf();
                    // Forward slashes on every host: the assertion below names the path.
                    let rel = rel.display().to_string().replace('\\', "/");
                    hits.push(format!("{rel}:{}", i + 1));
                }
            }
        }
        assert_eq!(
            hits.len(),
            1,
            "sessions must be built through new_text_embedding: {hits:?}"
        );
        assert!(
            hits[0].starts_with("semantic/onnx.rs:"),
            "the one call site must be this module's constructor: {hits:?}"
        );
    }
}
