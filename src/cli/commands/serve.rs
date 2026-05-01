//! Serve command - MCP server modes (stdio, HTTP, HTTPS).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Settings;
use crate::indexing::facade::IndexFacade;

/// PID lockfile guard for stdio MCP servers. Prevents two concurrent
/// `codanna serve` (stdio) processes from racing the tantivy writer on the
/// same `.codanna/index/`. Removed automatically on drop. HTTP/HTTPS modes
/// get exclusion via port binding and do not use this lock.
struct ServeLockGuard {
    path: PathBuf,
}

#[derive(Debug)]
enum ServeLockError {
    AlreadyRunning { pid: u32, lock_path: PathBuf },
    Io(std::io::Error),
}

impl ServeLockGuard {
    fn acquire(index_path: &Path) -> Result<Self, ServeLockError> {
        let lock_path = index_path.join("serve.lock");

        if let Ok(contents) = std::fs::read_to_string(&lock_path)
            && let Ok(pid) = contents.trim().parse::<u32>()
            && pid_is_alive(pid)
        {
            return Err(ServeLockError::AlreadyRunning { pid, lock_path });
        }

        // Stale or missing — clear and create exclusively to close the race
        // window between the liveness check above and the create below.
        let _ = std::fs::remove_file(&lock_path);

        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServeLockError::Io)?;
        }

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut f) => {
                f.write_all(std::process::id().to_string().as_bytes())
                    .map_err(ServeLockError::Io)?;
                Ok(Self { path: lock_path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let pid = std::fs::read_to_string(&lock_path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                Err(ServeLockError::AlreadyRunning { pid, lock_path })
            }
            Err(e) => Err(ServeLockError::Io(e)),
        }
    }
}

impl Drop for ServeLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn pid_is_alive(pid: u32) -> bool {
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

/// Arguments for the serve command.
pub struct ServeArgs {
    pub watch: bool,
    pub watch_interval: u64,
    pub http: bool,
    pub https: bool,
    pub bind: String,
}

/// Run the serve command.
pub async fn run(
    args: ServeArgs,
    config: Settings,
    settings: Arc<Settings>,
    facade: IndexFacade,
    index_path: PathBuf,
) {
    let ServeArgs {
        watch,
        watch_interval,
        http,
        https,
        bind,
    } = args;

    // Determine server mode:
    // 1. CLI --https flag takes highest precedence
    // 2. CLI --http flag takes second precedence
    // 3. Otherwise, check config.server.mode
    let server_mode = if https {
        "https"
    } else if http || config.server.mode == "http" {
        "http"
    } else {
        "stdio"
    };

    // Use bind address from CLI if provided, otherwise from config
    // For HTTPS, default to port 8443 if using default bind
    let bind_address = if bind != "127.0.0.1:8080" {
        // CLI flag was explicitly set (not default)
        bind
    } else if https {
        // For HTTPS, use port 8443 by default
        "127.0.0.1:8443".to_string()
    } else {
        // Use config value
        config.server.bind.clone()
    };

    // Use watch interval from CLI if provided, otherwise from config
    let actual_watch_interval = if watch_interval != 5 {
        // CLI flag was explicitly set (not default)
        watch_interval
    } else {
        config.server.watch_interval
    };

    match server_mode {
        "https" => {
            run_https_server(&config, watch, bind_address).await;
        }
        "http" => {
            run_http_server(config, watch, bind_address).await;
        }
        _ => {
            run_stdio_server(
                config,
                settings,
                facade,
                index_path,
                watch,
                actual_watch_interval,
            )
            .await;
        }
    }
}

async fn run_https_server(config: &Settings, watch: bool, bind_address: String) {
    // HTTPS mode - secure server with TLS
    tracing::info!(target: "mcp", "starting HTTPS server on {bind_address}");
    if watch || config.file_watch.enabled {
        tracing::debug!(
            target: "mcp",
            "file watching enabled with {}ms debounce",
            config.file_watch.debounce_ms
        );
    }

    // Use the HTTPS server implementation
    #[cfg(feature = "https-server")]
    {
        use crate::mcp::https_server::serve_https;
        if let Err(e) = serve_https(config.clone(), watch, bind_address).await {
            eprintln!("HTTPS server error: {e}");
            std::process::exit(1);
        }
    }

    #[cfg(not(feature = "https-server"))]
    {
        eprintln!("HTTPS server support is not compiled in.");
        eprintln!("Please rebuild with: cargo build --features https-server");
        std::process::exit(1);
    }
}

async fn run_http_server(config: Settings, watch: bool, bind_address: String) {
    // HTTP mode - persistent server with event-driven file watching
    eprintln!("Starting MCP server in HTTP mode");
    eprintln!("Bind address: {bind_address}");
    if watch || config.file_watch.enabled {
        eprintln!(
            "File watching: ENABLED (event-driven with {}ms debounce)",
            config.file_watch.debounce_ms
        );
    }

    // Use the HTTP server implementation
    use crate::mcp::http_server::serve_http;
    if let Err(e) = serve_http(config, watch, bind_address).await {
        eprintln!("HTTP server error: {e}");
        std::process::exit(1);
    }
}

async fn run_stdio_server(
    config: Settings,
    settings: Arc<Settings>,
    facade: IndexFacade,
    index_path: PathBuf,
    watch: bool,
    actual_watch_interval: u64,
) {
    // Acquire the stdio serve lock before doing anything else. Bound at
    // function scope so the guard removes the lockfile on return / unwind.
    let _serve_lock = match ServeLockGuard::acquire(&index_path) {
        Ok(guard) => guard,
        Err(ServeLockError::AlreadyRunning { pid, lock_path }) => {
            eprintln!(
                "Another codanna serve is already running for this index (PID {pid}, lock at {}).",
                lock_path.display()
            );
            eprintln!();
            eprintln!("Subagents and other AI tools may have spawned a duplicate. To run multiple");
            eprintln!("clients against one index, use HTTP mode:");
            eprintln!("  codanna serve --http --watch");
            eprintln!("HTTP mode supports concurrent clients without lock conflicts.");
            eprintln!();
            eprintln!(
                "If you are sure no other codanna serve is running, remove {} and retry.",
                lock_path.display()
            );
            std::process::exit(1);
        }
        Err(ServeLockError::Io(e)) => {
            eprintln!(
                "Failed to acquire serve lock under {}: {e}",
                index_path.display()
            );
            std::process::exit(1);
        }
    };

    // stdio mode - current implementation
    eprintln!("Starting MCP server on stdio transport");
    if watch {
        eprintln!("Index watching enabled (interval: {actual_watch_interval}s)");
    }
    eprintln!("To test: npx @modelcontextprotocol/inspector cargo run -- serve");

    // Create MCP server using the already-loaded facade
    tracing::debug!(
        target: "mcp",
        "creating server with facade - symbols: {}, semantic: {}",
        facade.symbol_count(),
        facade.has_semantic_search()
    );
    let server = crate::mcp::CodeIntelligenceServer::new(facade);

    // Load document store and attach to server (shared with watcher later)
    let document_store_arc = crate::documents::load_from_settings(&config);
    let server = if let Some(ref store_arc) = document_store_arc {
        tracing::debug!(target: "mcp", "attaching document store to server");
        server.with_document_store_arc(store_arc.clone())
    } else {
        server
    };

    // If watch mode is enabled, start the hot-reload watcher
    if watch {
        use crate::watcher::HotReloadWatcher;
        use std::time::Duration;

        let facade_arc = server.get_facade_arc();
        let watcher = HotReloadWatcher::new(
            facade_arc,
            settings.clone(),
            Duration::from_secs(actual_watch_interval),
        );

        // Spawn watcher in background
        tokio::spawn(async move {
            watcher.watch().await;
        });

        eprintln!("Hot-reload watcher started");
    }

    // Start unified file watcher if enabled
    if watch || config.file_watch.enabled {
        use crate::mcp::notifications::NotificationBroadcaster;
        use crate::watcher::UnifiedWatcher;
        use crate::watcher::handlers::{CodeFileHandler, ConfigFileHandler, DocumentFileHandler};

        let broadcaster = Arc::new(NotificationBroadcaster::new(100));

        let workspace_root = config
            .workspace_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let settings_path = workspace_root.join(".codanna/settings.toml");
        let debounce_ms = config.file_watch.debounce_ms;
        let facade_arc = server.get_facade_arc();

        // Build unified watcher with handlers
        let mut builder = UnifiedWatcher::builder()
            .broadcaster(broadcaster.clone())
            .indexer(facade_arc.clone())
            .index_path(index_path.clone())
            .workspace_root(workspace_root.clone())
            .debounce_ms(debounce_ms);

        // Add code file handler
        builder = builder.handler(CodeFileHandler::new(
            facade_arc.clone(),
            workspace_root.clone(),
        ));

        // Add config file handler
        match ConfigFileHandler::new(settings_path.clone()) {
            Ok(config_handler) => {
                builder = builder.handler(config_handler);
            }
            Err(e) => {
                eprintln!("Failed to create config handler: {e}");
            }
        }

        // Add document handler using shared document store
        if let Some(store_arc) = document_store_arc {
            tracing::debug!(target: "mcp", "adding document handler to watcher");
            builder = builder
                .document_store(store_arc.clone())
                .chunking_config(config.documents.defaults.clone())
                .handler(DocumentFileHandler::new(store_arc, workspace_root.clone()));
        }

        // Subscribe to broadcaster for MCP notifications
        let notification_receiver = broadcaster.subscribe();
        let notification_server = server.clone();

        // Build and start the unified watcher
        match builder.build() {
            Ok(unified_watcher) => {
                tokio::spawn(async move {
                    if let Err(e) = unified_watcher.watch().await {
                        eprintln!("Unified watcher error: {e}");
                    }
                });
                eprintln!(
                    "Unified watcher started (debounce: {debounce_ms}ms, config: {})",
                    settings_path.display()
                );

                // Start notification listener to forward events to MCP client
                tokio::spawn(async move {
                    notification_server
                        .start_notification_listener(notification_receiver)
                        .await;
                });
            }
            Err(e) => {
                eprintln!("Failed to start unified watcher: {e}");
            }
        }
    }

    // Start server with stdio transport
    use rmcp::{ServiceExt, transport::stdio};
    let service = server
        .serve(stdio())
        .await
        .map_err(|e| {
            eprintln!("Failed to start MCP server: {e}");
            std::process::exit(1);
        })
        .unwrap();

    // Wait for server to complete
    service
        .waiting()
        .await
        .map_err(|e| {
            eprintln!("MCP server error: {e}");
            std::process::exit(1);
        })
        .unwrap();
}

/// Run the MCP test command.
pub async fn run_mcp_test(
    server_binary: Option<PathBuf>,
    cli_config: Option<PathBuf>,
    tool: Option<String>,
    args: Option<String>,
    delay: Option<u64>,
) {
    use crate::mcp::client::CodeIntelligenceClient;

    // Get server binary path (default to current executable)
    let server_path = server_binary
        .unwrap_or_else(|| std::env::current_exe().expect("Failed to get current executable path"));

    // Run the test
    if let Err(e) =
        CodeIntelligenceClient::test_server(server_path, cli_config, tool, args, delay).await
    {
        eprintln!("MCP test failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod serve_lock_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn acquire_writes_pid_and_drop_removes_lock() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("serve.lock");

        {
            let _guard = ServeLockGuard::acquire(dir.path()).expect("first acquire");
            let contents = std::fs::read_to_string(&lock_path).unwrap();
            assert_eq!(contents.trim(), std::process::id().to_string());
        }

        assert!(
            !lock_path.exists(),
            "lockfile should be removed when guard drops"
        );
    }

    #[test]
    fn second_acquire_blocks_when_first_is_alive() {
        let dir = TempDir::new().unwrap();
        let _first = ServeLockGuard::acquire(dir.path()).expect("first acquire");

        match ServeLockGuard::acquire(dir.path()) {
            Err(ServeLockError::AlreadyRunning { pid, .. }) => {
                assert_eq!(pid, std::process::id());
            }
            Ok(_) => panic!("second acquire should have failed"),
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn stale_lock_with_dead_pid_is_overwritten() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("serve.lock");

        // PID 0 never refers to a normal process on Unix; sysinfo also reports
        // it as absent. Use it as a synthetic stale entry.
        std::fs::write(&lock_path, "0").unwrap();
        assert!(!pid_is_alive(0), "PID 0 must read as dead for this test");

        let guard = ServeLockGuard::acquire(dir.path()).expect("stale lock should be reclaimed");
        let contents = std::fs::read_to_string(&lock_path).unwrap();
        assert_eq!(contents.trim(), std::process::id().to_string());
        drop(guard);
        assert!(!lock_path.exists());
    }
}
