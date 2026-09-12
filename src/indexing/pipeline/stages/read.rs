//! Read stage - file content reading
//!
//! Reads file contents and computes content hashes.
//! Runs with multiple threads to saturate I/O.

use crate::indexing::file_info::calculate_hash;
use crate::indexing::pipeline::types::{FileContent, PipelineError, PipelineResult};
use crossbeam_channel::{Receiver, Sender};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;

/// Hard upper bound for a single source file read into memory.
/// This protects the pipeline from generated/binary artifacts and accidental
/// giant files until a configurable byte-budget is introduced.
const MAX_SOURCE_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Read stage for file content loading.
pub struct ReadStage {
    threads: usize,
    /// Workspace root for path normalization (stores relative paths)
    workspace_root: Option<PathBuf>,
}

impl ReadStage {
    /// Create a new read stage.
    pub fn new(threads: usize) -> Self {
        Self {
            threads: threads.max(1),
            workspace_root: None,
        }
    }

    /// Create a new read stage with workspace root for path normalization.
    pub fn with_workspace_root(threads: usize, workspace_root: Option<PathBuf>) -> Self {
        Self {
            threads: threads.max(1),
            workspace_root,
        }
    }

    /// Read a single file directly (for incremental mode).
    pub fn read_single(&self, path: &PathBuf) -> PipelineResult<FileContent> {
        read_file(path)
    }

    /// Run the read stage, reading from path channel and sending to content channel.
    ///
    /// Returns (files_read, files_failed, input_wait, output_wait, wall_time).
    pub fn run(
        &self,
        receiver: Receiver<PathBuf>,
        sender: Sender<FileContent>,
    ) -> PipelineResult<(
        usize,
        usize,
        std::time::Duration,
        std::time::Duration,
        std::time::Duration,
    )> {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let read_count = Arc::new(AtomicUsize::new(0));
        let error_count = Arc::new(AtomicUsize::new(0));
        let input_wait_ns = Arc::new(AtomicU64::new(0));
        let output_wait_ns = Arc::new(AtomicU64::new(0));

        let workspace_root = self.workspace_root.clone();
        let workspace_root = Arc::new(workspace_root);

        let handles: Vec<_> = (0..self.threads)
            .map(|_| {
                let receiver = receiver.clone();
                let sender = sender.clone();
                let read_count = read_count.clone();
                let error_count = error_count.clone();
                let input_wait_ns = input_wait_ns.clone();
                let output_wait_ns = output_wait_ns.clone();
                let workspace_root = workspace_root.clone();

                thread::spawn(move || {
                    loop {
                        // Track input wait (time blocked on recv)
                        let recv_start = Instant::now();
                        let path = match receiver.recv() {
                            Ok(p) => p,
                            Err(_) => break, // Channel closed
                        };
                        input_wait_ns
                            .fetch_add(recv_start.elapsed().as_nanos() as u64, Ordering::Relaxed);

                        match read_file(&path) {
                            Ok(mut content) => {
                                // Normalize path to relative if workspace_root is set
                                if let Some(ref root) = *workspace_root {
                                    if let Ok(relative) = content.path.strip_prefix(root) {
                                        content.path = relative.to_path_buf();
                                    }
                                }

                                read_count.fetch_add(1, Ordering::Relaxed);

                                // Track output wait (time blocked on send)
                                let send_start = Instant::now();
                                if sender.send(content).is_err() {
                                    break;
                                }
                                output_wait_ns.fetch_add(
                                    send_start.elapsed().as_nanos() as u64,
                                    Ordering::Relaxed,
                                );
                            }
                            Err(_) => {
                                error_count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                })
            })
            .collect();

        // Wait for all threads. A panicked worker's unread files never
        // reached PARSE; returning Ok would report success on a silently
        // incomplete index (same convention as join_read_workers).
        let mut panicked_workers = 0usize;
        for handle in handles {
            if handle.join().is_err() {
                tracing::error!(target: "pipeline", "READ worker panicked");
                panicked_workers += 1;
            }
        }

        if panicked_workers > 0 {
            return Err(PipelineError::Parse {
                path: PathBuf::new(),
                reason: format!(
                    "{panicked_workers} READ worker(s) panicked; their files never reached PARSE"
                ),
            });
        }

        Ok((
            read_count.load(Ordering::Relaxed),
            error_count.load(Ordering::Relaxed),
            Duration::from_nanos(input_wait_ns.load(Ordering::Relaxed)),
            Duration::from_nanos(output_wait_ns.load(Ordering::Relaxed)),
            start.elapsed(),
        ))
    }
}

/// Read a single file and compute its SHA256 hash.
fn read_file(path: &PathBuf) -> PipelineResult<FileContent> {
    let metadata = fs::metadata(path).map_err(|e| PipelineError::FileRead {
        path: path.clone(),
        source: e,
    })?;

    if !metadata.file_type().is_file() {
        return Err(PipelineError::FileRead {
            path: path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "refusing to read non-regular filesystem entry",
            ),
        });
    }

    if metadata.len() > MAX_SOURCE_FILE_BYTES {
        return Err(PipelineError::FileRead {
            path: path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "source file is {} bytes; maximum supported size is {} bytes",
                    metadata.len(),
                    MAX_SOURCE_FILE_BYTES
                ),
            ),
        });
    }

    let content = fs::read_to_string(path).map_err(|e| PipelineError::FileRead {
        path: path.clone(),
        source: e,
    })?;

    // The file may have grown between metadata() and read_to_string(). Bound the
    // post-read size as well so the preflight check cannot be raced into an
    // unbounded retained allocation.
    if content.len() as u64 > MAX_SOURCE_FILE_BYTES {
        return Err(PipelineError::FileRead {
            path: path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "source file grew beyond the {} byte limit while being read",
                    MAX_SOURCE_FILE_BYTES
                ),
            ),
        });
    }

    let hash = calculate_hash(&content);

    Ok(FileContent::new(path.clone(), content, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;
    use tempfile::TempDir;

    #[test]
    fn test_read_single_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.rs");

        let content = "fn main() { println!(\"Hello\"); }";
        fs::write(&file_path, content).unwrap();

        let result = read_file(&file_path);
        assert!(result.is_ok(), "Read should succeed");

        let file_content = result.unwrap();
        assert_eq!(file_content.content, content);
        assert_eq!(file_content.path, file_path);

        // Hash should be consistent (SHA256)
        let expected_hash = calculate_hash(content);
        assert_eq!(file_content.hash, expected_hash);

        println!(
            "Read file: {} ({} bytes, hash: {})",
            file_path.display(),
            content.len(),
            file_content.hash
        );
    }

    #[test]
    fn test_read_stage_multiple_files() {
        let temp = TempDir::new().unwrap();

        // Create test files
        let files: Vec<_> = (0..5)
            .map(|i| {
                let path = temp.path().join(format!("file{i}.rs"));
                let content = format!("fn func{i}() {{}}");
                fs::write(&path, &content).unwrap();
                path
            })
            .collect();

        let (path_tx, path_rx) = bounded(100);
        let (content_tx, content_rx) = bounded(100);

        // Send paths
        for path in &files {
            path_tx.send(path.clone()).unwrap();
        }
        drop(path_tx); // Close channel

        let stage = ReadStage::new(2);
        let result = stage.run(path_rx, content_tx);

        assert!(result.is_ok());
        let (read, failed, input_wait, output_wait, wall_time) = result.unwrap();

        // Collect results
        let contents: Vec<_> = content_rx.iter().collect();

        println!("Read {read} files, {failed} failed:");
        println!(
            "  Input wait: {input_wait:?}, Output wait: {output_wait:?}, Wall time: {wall_time:?}"
        );
        for fc in &contents {
            println!(
                "  - {} ({} bytes, hash: {})",
                fc.path.display(),
                fc.content.len(),
                fc.hash
            );
        }

        assert_eq!(read, 5, "Should read all 5 files");
        assert_eq!(failed, 0, "No files should fail");
        assert_eq!(contents.len(), 5, "Should have 5 FileContent items");
    }

    #[test]
    fn test_read_stage_handles_errors() {
        let (path_tx, path_rx) = bounded(100);
        let (content_tx, content_rx) = bounded(100);

        // Send non-existent paths
        path_tx
            .send(PathBuf::from("/nonexistent/file1.rs"))
            .unwrap();
        path_tx
            .send(PathBuf::from("/nonexistent/file2.rs"))
            .unwrap();
        drop(path_tx);

        let stage = ReadStage::new(1);
        let result = stage.run(path_rx, content_tx);

        assert!(result.is_ok());
        let (read, failed, _, _, _) = result.unwrap();

        let contents: Vec<_> = content_rx.iter().collect();

        println!("Read {read} files, {failed} failed");

        assert_eq!(read, 0, "No files should be read");
        assert_eq!(failed, 2, "Both files should fail");
        assert!(contents.is_empty(), "No content should be produced");
    }

    #[test]
    fn hardening_read_rejects_oversized_source_before_loading() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("huge.rs");
        let file = fs::File::create(&file_path).unwrap();
        file.set_len(MAX_SOURCE_FILE_BYTES + 1).unwrap();

        let err = read_file(&file_path).expect_err("oversized source must be rejected");
        assert!(err.to_string().contains("maximum supported size"));
    }

    #[cfg(unix)]
    #[test]
    fn hardening_read_rejects_non_regular_entries_without_blocking() {
        use std::os::unix::net::UnixListener;

        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("looks_like_source.rs");
        let _listener = UnixListener::bind(&socket_path).unwrap();

        let err = read_file(&socket_path).expect_err("unix socket must not be read as source");
        assert!(err.to_string().contains("non-regular"));
    }

    #[test]
    fn test_hash_consistency() {
        let content1 = "fn hello() {}";
        let content2 = "fn hello() {}";
        let content3 = "fn world() {}";

        let hash1 = calculate_hash(content1);
        let hash2 = calculate_hash(content2);
        let hash3 = calculate_hash(content3);

        println!("hash1: {hash1}");
        println!("hash2: {hash2}");
        println!("hash3: {hash3}");

        assert_eq!(hash1, hash2, "Same content should have same hash");
        assert_ne!(hash1, hash3, "Different content should have different hash");
    }
}
