//! Per-tree MCP serve discovery record.
//!
//! When an HTTP MCP server starts for a project tree, it writes a small
//! `.codanna/serve.json` record `{pid, port}` so other tools (e.g. the CLI
//! proxy) can discover a running server without guessing ports. The record
//! is written atomically (temp file + rename) at mode 0600 and removed on
//! graceful shutdown.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Settings;

/// Scheme of the backing MCP server named by a [`ServeRecord`].
///
/// Defaults to `Http` so that legacy `serve.json` records written before this
/// field existed (no `scheme` key at all) deserialize as `Http` via
/// `#[serde(default)]` on `ServeRecord::scheme`, rather than failing to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServeScheme {
    #[default]
    Http,
    Https,
}

impl ServeScheme {
    /// The scheme as used when building a URI (`"http"` / `"https"`).
    pub fn as_str(self) -> &'static str {
        match self {
            ServeScheme::Http => "http",
            ServeScheme::Https => "https",
        }
    }
}

/// Discovery record written to `.codanna/serve.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeRecord {
    pub pid: u32,
    pub port: u16,
    #[serde(default)]
    pub scheme: ServeScheme,
}

/// Errors from reading/writing the serve discovery record.
#[derive(Error, Debug)]
pub enum DiscoveryError {
    #[error("failed to create directory '{path}': {source}")]
    CreateDir { path: PathBuf, source: io::Error },

    #[error("failed to write serve record '{path}': {source}")]
    Write { path: PathBuf, source: io::Error },

    #[error("failed to set permissions on '{path}': {source}")]
    Permissions { path: PathBuf, source: io::Error },

    #[error("failed to rename '{from}' to '{to}': {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        source: io::Error,
    },

    #[error("failed to serialize serve record: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error(
        "failed to acquire spawn lock '{path}': {source}; if this persists, inspect '{path}' for a stale lock left by a crashed process"
    )]
    LockIo { path: PathBuf, source: io::Error },

    #[error(
        "failed to determine current executable path to spawn the backing 'codanna serve' process: {source}"
    )]
    CurrentExe { source: io::Error },

    #[error(
        "failed to spawn backing 'codanna serve' process: {source}; verify the codanna binary is on PATH and '{workspace_root}' is writable"
    )]
    Spawn {
        workspace_root: PathBuf,
        source: io::Error,
    },

    #[error(
        "backing 'codanna serve --http' did not become healthy within {timeout_ms}ms; inspect the spawn lock at '{lock_path}' and the discovery record at '{record_path}' for a stuck or crashed process"
    )]
    SpawnTimeout {
        timeout_ms: u64,
        lock_path: PathBuf,
        record_path: PathBuf,
    },

    #[error(
        "no codanna configuration found for '{workspace_root}' (expected '{config_path}'); refusing to spawn a backing server for an unconfigured tree -- run 'codanna init' in the intended project root first"
    )]
    NoConfiguration {
        workspace_root: PathBuf,
        config_path: PathBuf,
    },

    #[error(
        "no backing 'codanna serve --http' is running for '{workspace_root}' and auto-spawn is disabled ([server] auto_spawn = false in '{config_path}'); start one manually with 'codanna serve --http --watch', or set auto_spawn = true to let the proxy spawn it"
    )]
    AutoSpawnDisabled {
        workspace_root: PathBuf,
        config_path: PathBuf,
    },
}

pub type DiscoveryResult<T> = Result<T, DiscoveryError>;

/// Resolve the workspace root to key discovery off of: prefer the already
/// -resolved `Settings::workspace_root`, falling back to walking up from the
/// current directory for a `.codanna` directory.
///
/// The `.or_else(Settings::workspace_root)` fallback is load-bearing, not
/// defensive: `Settings::load_from` (the `--config` code path) does not
/// populate `workspace_root`, unlike `Settings::load`. A helper that reads
/// only `config.workspace_root` would introduce a divergence in exactly the
/// `--config` case, so both fields must be considered here.
pub(crate) fn resolve_workspace_root(config: &Settings) -> Option<PathBuf> {
    config
        .workspace_root
        .clone()
        .or_else(Settings::workspace_root)
}

/// Derive the `.codanna` discovery-record directory for `workspace_root`.
///
/// This is deliberately NOT `index_path.parent()`: `index_path` may be
/// absolute or resolved relative to a `--config` file's parent
/// (`init::resolve_index_path`), so its parent only coincides with
/// `.codanna` in the default, unconfigured case. Deriving from
/// `workspace_root` instead keeps `serve --http` and `discover_or_spawn`
/// agreeing on the same directory regardless of how `index_path` was
/// customized.
pub(crate) fn discovery_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(crate::init::local_dir_name())
}

fn record_path(codanna_dir: &Path) -> PathBuf {
    codanna_dir.join("serve.json")
}

/// Write `record` to `<codanna_dir>/serve.json` atomically at mode 0600.
///
/// The record is first written to a sibling temp file, permissions are set,
/// then it is renamed into place. This mirrors the `PidLockGuard` acquire
/// discipline: a partial write (e.g. a crash mid-write) can never be
/// observed as a corrupt `serve.json` because the rename is the only
/// operation that makes the file visible under its final name.
pub fn write_record(codanna_dir: &Path, record: &ServeRecord) -> DiscoveryResult<()> {
    std::fs::create_dir_all(codanna_dir).map_err(|source| DiscoveryError::CreateDir {
        path: codanna_dir.to_path_buf(),
        source,
    })?;

    let final_path = record_path(codanna_dir);
    let tmp_path = codanna_dir.join(format!("serve.json.tmp.{}", std::process::id()));

    let json = serde_json::to_string(record)?;

    std::fs::write(&tmp_path, json.as_bytes()).map_err(|source| DiscoveryError::Write {
        path: tmp_path.clone(),
        source,
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DiscoveryError::Permissions {
                path: tmp_path.clone(),
                source,
            },
        )?;
    }

    std::fs::rename(&tmp_path, &final_path).map_err(|source| DiscoveryError::Rename {
        from: tmp_path.clone(),
        to: final_path.clone(),
        source,
    })?;

    Ok(())
}

/// Read the serve discovery record from `<codanna_dir>/serve.json`.
///
/// Returns `None` if the file is absent, unreadable, or fails to parse
/// (e.g. left over/corrupt from a previous crashed server) rather than
/// erroring, since discovery is best-effort.
pub fn read_record(codanna_dir: &Path) -> Option<ServeRecord> {
    let contents = std::fs::read_to_string(record_path(codanna_dir)).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Remove the serve discovery record, if present. Best-effort: errors are
/// swallowed since this is used on shutdown paths where there is nothing
/// meaningful to do about a failed removal.
pub fn remove_record(codanna_dir: &Path) {
    let _ = std::fs::remove_file(record_path(codanna_dir));
}

/// Check whether a process with the given PID is currently alive.
pub fn pid_is_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid).is_some()
}

/// Decision produced by inspecting the current discovery record: whether an
/// existing server can be reused as-is, or a new one must be spawned because
/// no record exists or the recorded PID is dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Discover,
    Spawn,
}

/// Decide whether `record` describes a live, reusable server.
///
/// A `None` record (no `serve.json` yet) or a record whose PID is no longer
/// alive (crashed/killed server, stale leftover) both resolve to `Spawn`.
fn decide(record: Option<&ServeRecord>) -> Decision {
    match record {
        Some(r) if pid_is_alive(r.pid) => Decision::Discover,
        _ => Decision::Spawn,
    }
}

/// PID lockfile guard shared by the stdio serve lock (`.codanna/index/serve.lock`,
/// used by `codanna serve`) and the spawn single-flight lock
/// (`.codanna/http.lock`, used by [`discover_or_spawn`]). These are two
/// distinct locks guarding two distinct races (one tantivy writer per index
/// vs. one spawn attempt per workspace); they must never share a filename or
/// a stdio serve and a proxy spawn could contend on the same lock.
///
/// `create_new`-first acquire: the lockfile is only ever removed after
/// `create_new` has failed with `AlreadyExists` AND the recorded PID is
/// verified dead. An unconditional pre-remove would delete a racing
/// process's live lock and let two callers share one guarded resource.
pub(crate) struct PidLockGuard {
    path: PathBuf,
}

#[derive(Debug)]
pub(crate) enum PidLockError {
    /// Another process currently holds the lock (recorded PID is alive).
    Held {
        pid: u32,
        lock_path: PathBuf,
    },
    Io(io::Error),
}

impl PidLockGuard {
    pub(crate) fn acquire(lock_path: &Path) -> Result<Self, PidLockError> {
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(PidLockError::Io)?;
        }

        for _ in 0..3 {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(lock_path)
            {
                Ok(mut f) => {
                    f.write_all(std::process::id().to_string().as_bytes())
                        .map_err(PidLockError::Io)?;
                    return Ok(Self {
                        path: lock_path.to_path_buf(),
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    match read_lock_pid(lock_path) {
                        Some(pid) if pid_is_alive(pid) => {
                            return Err(PidLockError::Held {
                                pid,
                                lock_path: lock_path.to_path_buf(),
                            });
                        }
                        Some(_) => {
                            // Recorded process is dead: reclaim and retry.
                            let _ = std::fs::remove_file(lock_path);
                        }
                        None => {
                            // No parseable PID. A racing process may have
                            // created the lock but not written its PID yet;
                            // re-read after a grace window before treating
                            // the file as a dead leftover (SIGKILL between
                            // create and write leaves an empty lock that
                            // must self-heal).
                            std::thread::sleep(Duration::from_millis(50));
                            match read_lock_pid(lock_path) {
                                Some(pid) if pid_is_alive(pid) => {
                                    return Err(PidLockError::Held {
                                        pid,
                                        lock_path: lock_path.to_path_buf(),
                                    });
                                }
                                _ => {
                                    let _ = std::fs::remove_file(lock_path);
                                }
                            }
                        }
                    }
                }
                Err(e) => return Err(PidLockError::Io(e)),
            }
        }

        // Retries exhausted: another process keeps winning the create race.
        let pid = read_lock_pid(lock_path).unwrap_or(0);
        Err(PidLockError::Held {
            pid,
            lock_path: lock_path.to_path_buf(),
        })
    }
}

fn read_lock_pid(lock_path: &Path) -> Option<u32> {
    std::fs::read_to_string(lock_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

impl Drop for PidLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Best-effort HTTP health probe against `127.0.0.1:{port}/health`.
///
/// Scheme-aware: an `Http` record is probed with a raw `TcpStream` (this
/// module only needs to know whether *some* response comes back promptly, not
/// to parse a body); an `Https` record is probed through
/// [`crate::serve_tls::pinned_client`], the SAME cert-pinning client used by
/// `mcp::proxy` -- this is deliberately not a second, hand-rolled TLS path.
/// Any connect/read/TLS failure or non-2xx status (including the client
/// being unavailable, e.g. `HttpsSupportNotCompiled`) is treated as "not yet
/// healthy" rather than an error, since this is called in a poll loop where
/// transient failures during server startup are expected.
async fn check_health(port: u16, scheme: ServeScheme) -> bool {
    match scheme {
        ServeScheme::Http => {
            let addr = format!("127.0.0.1:{port}");
            let Ok(socket_addr) = addr.parse() else {
                return false;
            };
            let Ok(mut stream) =
                TcpStream::connect_timeout(&socket_addr, Duration::from_millis(500))
            else {
                return false;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

            let request = format!(
                "GET /health HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(request.as_bytes()).is_err() {
                return false;
            }

            let mut buf = [0u8; 32];
            let Ok(n) = stream.read(&mut buf) else {
                return false;
            };
            let status_line = String::from_utf8_lossy(&buf[..n]);
            status_line.starts_with("HTTP/1.0 200") || status_line.starts_with("HTTP/1.1 200")
        }
        ServeScheme::Https => {
            let Ok(client) = crate::serve_tls::pinned_client() else {
                return false;
            };
            let url = format!("https://127.0.0.1:{port}/health");
            match client.get(url).send().await {
                Ok(response) => response.status().is_success(),
                Err(_) => false,
            }
        }
    }
}

/// Poll `codanna_dir` for a discovery record whose PID is alive and whose
/// `/health` endpoint responds, up to `timeout`. Used by both the lock
/// winner (after spawning) and the lock loser (waiting on the winner).
async fn wait_until_healthy(
    codanna_dir: &Path,
    lock_path: &Path,
    timeout: Duration,
    poll_interval: Duration,
) -> DiscoveryResult<ServeRecord> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(record) = read_record(codanna_dir) {
            if pid_is_alive(record.pid) && check_health(record.port, record.scheme).await {
                return Ok(record);
            }
        }

        if Instant::now() >= deadline {
            return Err(DiscoveryError::SpawnTimeout {
                timeout_ms: timeout.as_millis() as u64,
                lock_path: lock_path.to_path_buf(),
                record_path: record_path(codanna_dir),
            });
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(poll_interval.min(remaining).max(Duration::from_millis(1))).await;
    }
}

/// Spawn a detached `codanna serve --http --watch --bind 127.0.0.1:0` process
/// rooted at `workspace_root`. The child's stdio is severed and it is placed
/// in its own process group (Unix) / detached process group (Windows) so it
/// outlives the spawning process. The caller does not wait on the child;
/// readiness is observed exclusively through the discovery record and
/// `/health`, matching the record+lock discipline this module relies on
/// instead of a reaper or `ss`/`lsof`/`pgrep` process scan (deliberately
/// dropped: record+lock alone make duplicate servers unarisable).
fn spawn_detached(workspace_root: &Path) -> DiscoveryResult<()> {
    let exe = std::env::current_exe().map_err(|source| DiscoveryError::CurrentExe { source })?;

    let mut cmd = Command::new(exe);
    cmd.args(["serve", "--http", "--watch", "--bind", "127.0.0.1:0"])
        .current_dir(workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Detach into its own process group so it is not killed alongside
        // the spawning process's terminal/session.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    // Intentionally not waited on: a detached server is meant to outlive
    // this call. The handle is dropped without joining.
    let _child = cmd.spawn().map_err(|source| DiscoveryError::Spawn {
        workspace_root: workspace_root.to_path_buf(),
        source,
    })?;

    Ok(())
}

/// Discover a live HTTP MCP server for `workspace_root`, spawning one if none
/// is running.
///
/// 1. If `<workspace_root>/.codanna/serve.json` names a live PID, return it
///    immediately without spawning anything.
/// 2. Otherwise, race for a single-flight lock at
///    `<workspace_root>/.codanna/http.lock` (create_new-first, dead-PID
///    reclaim -- see `PidLockGuard`). The winner spawns a detached
///    `codanna serve --http --watch --bind 127.0.0.1:0` and polls the
///    discovery record + `/health` until it is healthy or
///    `settings.server.spawn_timeout_ms` elapses. The loser skips spawning
///    and polls the same way, waiting on the winner's record.
///
/// Spawning is guarded. A server is only ever *created* for a tree that holds a
/// real `.codanna/settings.toml` and that has `[server] auto_spawn = true`;
/// otherwise this returns [`DiscoveryError::NoConfiguration`] or
/// [`DiscoveryError::AutoSpawnDisabled`]. Discovering an *already-live* server is
/// not subject to either guard -- they gate creating a server, not using one.
///
/// This function never shells out to `ss`/`lsof`/`pgrep`, and there is no
/// reaper for duplicate processes: the record file plus the O_EXCL lock make
/// duplicate spawns structurally unarisable, so there is nothing to reap.
///
/// Note: exercising the real detached-spawn path end-to-end (spawning an
/// actual `codanna serve` child and observing it become healthy) is a
/// manual/integration validation step, not covered by the unit tests in this
/// module.
pub async fn discover_or_spawn(
    workspace_root: &Path,
    settings: &Settings,
) -> DiscoveryResult<ServeRecord> {
    let codanna_dir = discovery_dir(workspace_root);
    let lock_path = codanna_dir.join("http.lock");
    let timeout = Duration::from_millis(settings.server.spawn_timeout_ms);
    let poll_interval = Duration::from_millis(settings.server.health_poll_ms);

    // Fast path: an already-live server, no lock needed.
    //
    // Discovery is deliberately permissive: if a server is already serving this
    // tree we attach to it regardless of the guards below. Those guards gate the
    // decision to *create* a server, not the decision to *use* one.
    if decide(read_record(&codanna_dir).as_ref()) == Decision::Discover {
        // Safe to unwrap the option here only via re-match, not `.unwrap()`:
        // `decide` returning `Discover` implies `Some` with a live PID.
        if let Some(record) = read_record(&codanna_dir) {
            return Ok(record);
        }
    }

    // From here on the only way forward is to spawn (or to wait on a peer that
    // is spawning). Both guards below therefore run *before* we take the lock:
    // if this tree must never get a server, there is no point contending for the
    // right to create one.
    //
    // Guard 1: never spawn into an unconfigured tree. `Settings::workspace_root`
    // walks up for a `.codanna` *directory* and does not require the directory to
    // hold a `settings.toml`, so a bare or leftover `.codanna/` would otherwise
    // resolve as a workspace root and get a server (and an index) it never asked
    // for. Require a real config before creating anything.
    let config_path = codanna_dir.join("settings.toml");
    if !config_path.is_file() {
        return Err(DiscoveryError::NoConfiguration {
            workspace_root: workspace_root.to_path_buf(),
            config_path,
        });
    }

    // Guard 2: honour `[server] auto_spawn`. No live server was found above, so
    // with auto-spawn off there is nothing to attach to and nothing we may
    // create: fail with an actionable message rather than spawning anyway.
    if !settings.server.auto_spawn {
        return Err(DiscoveryError::AutoSpawnDisabled {
            workspace_root: workspace_root.to_path_buf(),
            config_path,
        });
    }

    match PidLockGuard::acquire(&lock_path) {
        Ok(_guard) => {
            // WINNER. Re-check: another process may have finished spawning
            // between our fast-path read and winning the lock.
            if decide(read_record(&codanna_dir).as_ref()) == Decision::Discover {
                if let Some(record) = read_record(&codanna_dir) {
                    return Ok(record);
                }
            }

            spawn_detached(workspace_root)?;
            wait_until_healthy(&codanna_dir, &lock_path, timeout, poll_interval).await
            // `_guard` drops here, releasing the lock once the winner's
            // spawn attempt has succeeded, timed out, or errored. Holding
            // the guard across the `.await` above is fine: it is a file
            // lock, not a mutex guard.
        }
        Err(PidLockError::Held { lock_path, .. }) => {
            // LOSER. Someone else is spawning (or just finished); wait on
            // their record instead of racing a second spawn.
            wait_until_healthy(&codanna_dir, &lock_path, timeout, poll_interval).await
        }
        Err(PidLockError::Io(source)) => Err(DiscoveryError::LockIo {
            path: lock_path,
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use tempfile::TempDir;

    #[test]
    fn write_then_read_round_trips() {
        let dir = TempDir::new().unwrap();
        let record = ServeRecord {
            pid: std::process::id(),
            port: 8080,
            scheme: ServeScheme::Http,
        };

        write_record(dir.path(), &record).expect("write_record should succeed");
        let read_back = read_record(dir.path()).expect("read_record should find the record");

        assert_eq!(read_back.pid, record.pid);
        assert_eq!(read_back.port, record.port);
    }

    #[test]
    #[cfg(unix)]
    fn written_record_has_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let record = ServeRecord {
            pid: std::process::id(),
            port: 9090,
            scheme: ServeScheme::Http,
        };

        write_record(dir.path(), &record).expect("write_record should succeed");

        let path = dir.path().join("serve.json");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "serve.json must be mode 0600");
    }

    #[test]
    fn read_record_returns_none_when_file_is_absent() {
        let dir = TempDir::new().unwrap();
        assert!(read_record(dir.path()).is_none());
    }

    #[test]
    fn read_record_returns_none_for_corrupt_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("serve.json"), b"not valid json{{{").unwrap();
        assert!(read_record(dir.path()).is_none());
    }

    #[test]
    fn pid_zero_is_never_alive() {
        assert!(!pid_is_alive(0));
    }

    #[tokio::test]
    async fn discover_or_spawn_returns_live_record_without_spawning() {
        // Given a workspace with a serve.json whose PID is *this* live test
        // process, discover_or_spawn must return it directly. If it instead
        // fell through to spawning, this test would hang (or fail) trying to
        // launch a real `codanna serve` child, since `current_exe()` in a
        // test binary is the test harness, not the CLI.
        let workspace = TempDir::new().unwrap();
        let codanna_dir = workspace.path().join(crate::init::local_dir_name());
        let record = ServeRecord {
            pid: std::process::id(),
            port: 12345,
            scheme: ServeScheme::Http,
        };
        write_record(&codanna_dir, &record).expect("write_record should succeed");

        let settings = Settings::default();
        let discovered = discover_or_spawn(workspace.path(), &settings)
            .await
            .expect("live record should short-circuit discovery");

        assert_eq!(discovered.pid, record.pid);
        assert_eq!(discovered.port, record.port);
        // No lock should have been created on the fast path.
        assert!(!codanna_dir.join("http.lock").exists());
    }

    /// A `.codanna/` directory that holds no `settings.toml` is NOT a configured
    /// tree. `Settings::workspace_root` walks up for the *directory* only, so
    /// without this guard a bare or leftover `.codanna/` anywhere up the tree
    /// would silently receive a spawned server and an index it never asked for.
    #[tokio::test]
    async fn refuses_to_spawn_when_tree_has_no_settings_toml() {
        let workspace = TempDir::new().unwrap();
        let codanna_dir = workspace.path().join(crate::init::local_dir_name());
        // A bare .codanna dir: exists, but carries no configuration.
        std::fs::create_dir_all(&codanna_dir).unwrap();

        let settings = Settings::default();
        let err = discover_or_spawn(workspace.path(), &settings)
            .await
            .expect_err("an unconfigured tree must not get a spawned server");

        match err {
            DiscoveryError::NoConfiguration { config_path, .. } => {
                assert_eq!(config_path, codanna_dir.join("settings.toml"));
            }
            other => panic!("expected NoConfiguration, got: {other:?}"),
        }
        // Nothing was created: no lock, no record.
        assert!(!codanna_dir.join("http.lock").exists());
        assert!(!codanna_dir.join("serve.json").exists());
    }

    /// `[server] auto_spawn = false` must actually be honoured. With no live
    /// server to attach to there is nothing to use and nothing we may create.
    #[tokio::test]
    async fn refuses_to_spawn_when_auto_spawn_is_disabled() {
        let workspace = TempDir::new().unwrap();
        let codanna_dir = workspace.path().join(crate::init::local_dir_name());
        std::fs::create_dir_all(&codanna_dir).unwrap();
        // A genuinely configured tree -- so this test isolates the auto_spawn
        // guard rather than tripping the NoConfiguration one.
        std::fs::write(codanna_dir.join("settings.toml"), "").unwrap();

        let mut settings = Settings::default();
        settings.server.auto_spawn = false;

        let err = discover_or_spawn(workspace.path(), &settings)
            .await
            .expect_err("auto_spawn = false must not spawn a server");

        match err {
            DiscoveryError::AutoSpawnDisabled { .. } => {}
            other => panic!("expected AutoSpawnDisabled, got: {other:?}"),
        }
        assert!(!codanna_dir.join("http.lock").exists());
    }

    /// The guards gate *creating* a server, never *using* one. A live server is
    /// attached to even when this process would not have been allowed to spawn
    /// it (auto_spawn off, and no settings.toml on disk).
    #[tokio::test]
    async fn discovers_live_server_even_when_spawning_would_be_refused() {
        let workspace = TempDir::new().unwrap();
        let codanna_dir = workspace.path().join(crate::init::local_dir_name());
        let record = ServeRecord {
            pid: std::process::id(),
            port: 12345,
            scheme: ServeScheme::Http,
        };
        write_record(&codanna_dir, &record).expect("write_record should succeed");

        let mut settings = Settings::default();
        settings.server.auto_spawn = false;

        let discovered = discover_or_spawn(workspace.path(), &settings)
            .await
            .expect("a live server must be discovered regardless of the spawn guards");

        assert_eq!(discovered.pid, record.pid);
        assert_eq!(discovered.port, record.port);
    }

    #[test]
    fn spawn_lock_single_flight_yields_one_winner() {
        // Two threads race to create the same lock file via create_new
        // (O_EXCL). Exactly one must win; the other must observe Held.
        // This is exercised directly against PidLockGuard rather than
        // discover_or_spawn end-to-end, so the test stays hermetic and never
        // spawns a real server process.
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        // A successful guard is intentionally leaked (not dropped) inside
        // each racing thread: if the winner's guard dropped immediately
        // after `acquire` returns, it would remove the lockfile before the
        // losing thread's `create_new` attempt lands, letting both threads
        // observe an empty slot and both "win" -- a timing-dependent false
        // pass. Leaking keeps the lock held for the duration of the race;
        // the TempDir is removed wholesale when the test ends regardless.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let lock_path_a = lock_path.clone();
        let barrier_a = barrier.clone();
        let handle_a = std::thread::spawn(move || {
            barrier_a.wait();
            match PidLockGuard::acquire(&lock_path_a) {
                Ok(guard) => {
                    std::mem::forget(guard);
                    true
                }
                Err(_) => false,
            }
        });

        let lock_path_b = lock_path.clone();
        let barrier_b = barrier.clone();
        let handle_b = std::thread::spawn(move || {
            barrier_b.wait();
            match PidLockGuard::acquire(&lock_path_b) {
                Ok(guard) => {
                    std::mem::forget(guard);
                    true
                }
                Err(_) => false,
            }
        });

        let won_a = handle_a.join().expect("thread a should not panic");
        let won_b = handle_b.join().expect("thread b should not panic");

        assert_ne!(
            won_a, won_b,
            "exactly one racing thread must win the single-flight lock"
        );
    }

    #[test]
    fn acquire_writes_pid_and_drop_removes_lock() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        {
            let _guard = PidLockGuard::acquire(&lock_path).expect("first acquire");
            let contents = std::fs::read_to_string(&lock_path).unwrap();
            assert_eq!(contents.trim(), std::process::id().to_string());
        }

        assert!(
            !lock_path.exists(),
            "lockfile should be removed when guard drops"
        );
    }

    /// B3: contention is signalled as `Held`, and the payload carries the
    /// *live* PID holding the lock (not a placeholder). Before the two guards
    /// were unified this payload was only exercised via the old
    /// `ServeLockError::AlreadyRunning`; `PidLockError::Held` is now the single
    /// carrier for both the stdio-serve and proxy-spawn paths.
    #[test]
    fn second_acquire_blocks_when_first_is_alive() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");
        let _first = PidLockGuard::acquire(&lock_path).expect("first acquire");

        match PidLockGuard::acquire(&lock_path) {
            Err(PidLockError::Held { pid, .. }) => {
                assert_eq!(pid, std::process::id());
            }
            Ok(_) => panic!("second acquire should have failed"),
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn unparseable_lock_is_reclaimed_after_grace_window() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        // SIGKILL between create and PID write leaves an empty lock; it must
        // self-heal instead of blocking serve forever.
        std::fs::write(&lock_path, "").unwrap();

        let guard = PidLockGuard::acquire(&lock_path).expect("empty lock should be reclaimed");
        let contents = std::fs::read_to_string(&lock_path).unwrap();
        assert_eq!(contents.trim(), std::process::id().to_string());
        drop(guard);
        assert!(!lock_path.exists());
    }

    #[test]
    fn stale_lock_with_dead_pid_is_overwritten() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        // PID 0 never refers to a normal process on Unix; sysinfo also
        // reports it as absent. Use it as a synthetic stale entry.
        std::fs::write(&lock_path, "0").unwrap();
        assert!(!pid_is_alive(0), "PID 0 must read as dead for this test");

        let guard = PidLockGuard::acquire(&lock_path).expect("stale lock should be reclaimed");
        let contents = std::fs::read_to_string(&lock_path).unwrap();
        assert_eq!(contents.trim(), std::process::id().to_string());
        drop(guard);
        assert!(!lock_path.exists());
    }

    #[test]
    fn decide_returns_spawn_when_recorded_pid_is_dead() {
        // PID 0 never refers to a live process on Unix and sysinfo reports
        // it as absent too; use it as a synthetic dead/stale record.
        let stale = ServeRecord {
            pid: 0,
            port: 8080,
            scheme: ServeScheme::Http,
        };
        assert!(!pid_is_alive(0), "PID 0 must read as dead for this test");

        assert_eq!(decide(Some(&stale)), Decision::Spawn);
        assert_eq!(decide(None), Decision::Spawn);
    }

    #[test]
    fn decide_returns_discover_when_recorded_pid_is_alive() {
        let live = ServeRecord {
            pid: std::process::id(),
            port: 8080,
            scheme: ServeScheme::Http,
        };
        assert_eq!(decide(Some(&live)), Decision::Discover);
    }

    /// THE LOAD-BEARING REGRESSION TEST.
    ///
    /// `discovery_dir` must be derived from `workspace_root` alone, never from
    /// `index_path`. Before this fix, `serve --http` derived the discovery
    /// record dir from `index_path.parent()` while `discover_or_spawn` used
    /// `workspace_root/.codanna`; under a custom `index_path` the two
    /// diverged, burning the proxy's spawn timeout and leaking an orphan
    /// process. This test fails if anyone ever reintroduces an
    /// index_path-derived discovery dir.
    #[test]
    fn discovery_dir_ignores_custom_index_path() {
        let workspace = TempDir::new().unwrap();
        // A second, unrelated tempdir stands in for a custom index_path that
        // is NOT under `workspace/.codanna`.
        let custom_index = TempDir::new().unwrap();

        let settings = Settings {
            workspace_root: Some(workspace.path().to_path_buf()),
            index_path: custom_index.path().join("some-other-index"),
            ..Settings::default()
        };

        let resolved_root = resolve_workspace_root(&settings)
            .expect("workspace_root was explicitly set and must resolve");
        let dir = discovery_dir(&resolved_root);

        assert_eq!(
            dir,
            workspace.path().join(crate::init::local_dir_name()),
            "discovery_dir must key off workspace_root, not index_path -- \
             if this assertion fails, an index_path-derived discovery dir \
             has been reintroduced and serve --http / discover_or_spawn will \
             diverge again under a custom index_path"
        );
    }

    #[test]
    fn resolve_workspace_root_prefers_explicit_setting() {
        // Deliberately does not rely on the CWD walk-up fallback: tests share
        // a process CWD, so relying on that fallback here would make this
        // test order-dependent and flaky against other tests in the suite.
        let workspace = TempDir::new().unwrap();
        let settings = Settings {
            workspace_root: Some(workspace.path().to_path_buf()),
            ..Settings::default()
        };

        assert_eq!(
            resolve_workspace_root(&settings),
            Some(workspace.path().to_path_buf())
        );
    }

    /// Proves the writer (`write_record` via `discovery_dir`) and the reader
    /// (`discover_or_spawn`'s fast path) agree on exactly one directory.
    #[tokio::test]
    async fn discovery_dir_matches_discover_or_spawn_fast_path() {
        let workspace = TempDir::new().unwrap();
        let settings = Settings {
            workspace_root: Some(workspace.path().to_path_buf()),
            ..Settings::default()
        };

        let resolved_root = resolve_workspace_root(&settings).unwrap();
        let codanna_dir = discovery_dir(&resolved_root);

        let record = ServeRecord {
            pid: std::process::id(),
            port: 12345,
            scheme: ServeScheme::Http,
        };
        write_record(&codanna_dir, &record).expect("write_record should succeed");

        let discovered = discover_or_spawn(workspace.path(), &settings)
            .await
            .expect("live record at discovery_dir(&workspace_root) should short-circuit discovery");

        assert_eq!(discovered.pid, record.pid);
        assert_eq!(discovered.port, record.port);
        // No lock should have been created on the fast path.
        assert!(!codanna_dir.join("http.lock").exists());
    }

    /// THE LOAD-BEARING COMPAT TEST.
    ///
    /// A `serve.json` written before `scheme` existed carries no `scheme`
    /// key at all. `#[serde(default)]` must let this still parse (as
    /// `ServeScheme::Http`); if it instead failed to parse, `read_record`
    /// would silently return `None`, `decide` would resolve to `Spawn`, and
    /// a live server left over from before an upgrade would get a duplicate
    /// spawned alongside it.
    #[test]
    fn legacy_record_without_scheme_reads_as_http() {
        let dir = TempDir::new().unwrap();
        let pid = std::process::id();
        std::fs::write(
            dir.path().join("serve.json"),
            format!(r#"{{"pid":{pid},"port":8080}}"#),
        )
        .unwrap();

        let record = read_record(dir.path());
        assert!(
            record.is_some(),
            "a legacy serve.json without a scheme key must still parse; \
             otherwise read_record returns None, decide() resolves to Spawn, \
             and a duplicate server gets spawned alongside the live legacy one"
        );
        assert_eq!(record.unwrap().scheme, ServeScheme::Http);
    }

    #[test]
    fn https_record_round_trips() {
        let dir = TempDir::new().unwrap();
        let record = ServeRecord {
            pid: std::process::id(),
            port: 8443,
            scheme: ServeScheme::Https,
        };

        write_record(dir.path(), &record).expect("write_record should succeed");

        let raw = std::fs::read_to_string(dir.path().join("serve.json")).unwrap();
        assert!(
            raw.contains(r#""scheme":"https""#),
            "on-disk JSON must literally contain the lowercase scheme value, got: {raw}"
        );

        let read_back = read_record(dir.path()).expect("read_record should find the record");
        assert_eq!(read_back.scheme, ServeScheme::Https);
    }

    /// THE ANTI-RELOCATION CHECK.
    ///
    /// Probing a plaintext listener through the `Https` arm must never report
    /// healthy -- whether because there is no persisted cert to pin
    /// (`pinned_client` fails closed) or, when one is present, because the
    /// TLS handshake itself fails against a plaintext peer. This guards
    /// against a future change accidentally treating a bare TCP response (or
    /// a connection error swallowed the wrong way) as a valid HTTPS health
    /// check.
    #[tokio::test]
    async fn https_health_probe_rejects_plaintext_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // The listener must serve a genuine plaintext 200 -- the same shape
        // `http_health_probe_still_succeeds` relies on. A listener that merely
        // accepted and dropped the connection would make this test vacuous:
        // the plaintext probe would report unhealthy against it too, so the
        // assertion below would hold even if the `Https` arm were wired back
        // to the plaintext probe. Answering 200 is what gives it teeth -- the
        // plaintext probe reports *healthy* here, so only a real TLS handshake
        // (which a plaintext peer cannot complete) yields the expected `false`.
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });

        assert!(!check_health(port, ServeScheme::Https).await);
    }

    /// Guards the untouched `Http` arm: a trivial plaintext 200 response
    /// must still be recognized as healthy after the scheme-aware,
    /// async conversion.
    #[tokio::test]
    async fn http_health_probe_still_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });

        assert!(check_health(port, ServeScheme::Http).await);
    }
}
