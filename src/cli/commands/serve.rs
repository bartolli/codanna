//! Serve command - MCP server modes (stdio, HTTP, HTTPS).

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Settings;
use crate::indexing::facade::IndexFacade;
use crate::serve_discovery::{PidLockError, PidLockGuard};

/// Arguments for the serve command.
pub struct ServeArgs {
    pub watch: bool,
    pub watch_interval: u64,
    pub http: bool,
    pub https: bool,
    pub proxy: bool,
    pub bind: String,
}

/// Resolve which server transport `codanna serve` should start.
///
/// Precedence mirrors `main.rs`'s `is_proxy_serve` so the pre-dispatch
/// resource predicates and the actual server startup never disagree about
/// which mode is in effect:
/// 1. CLI `--https` (highest precedence)
/// 2. CLI `--http`
/// 3. CLI `--proxy`, OR `config.server.mode == "proxy"` when no CLI transport
///    flag was given (bare `codanna serve` can default to proxy via
///    `settings.toml`)
/// 4. `config.server.mode == "http"`
/// 5. stdio (default)
fn resolve_server_mode(https: bool, http: bool, proxy: bool, config_mode: &str) -> &'static str {
    if https {
        "https"
    } else if http {
        "http"
    } else if proxy || config_mode == "proxy" {
        "proxy"
    } else if config_mode == "http" {
        "http"
    } else {
        "stdio"
    }
}

/// Run the serve command.
///
/// `facade` is `None` exactly when proxy mode is selected: `main.rs` computes
/// the effective mode ahead of index loading (§4.5) and skips constructing an
/// `IndexFacade` entirely for proxy, since the proxy process holds no index
/// state of its own -- it only relays to a backing HTTP server's facade.
pub async fn run(
    args: ServeArgs,
    config: Settings,
    settings: Arc<Settings>,
    facade: Option<IndexFacade>,
    index_path: PathBuf,
) {
    let ServeArgs {
        watch,
        watch_interval,
        http,
        https,
        proxy,
        bind,
    } = args;

    let server_mode = resolve_server_mode(https, http, proxy, &config.server.mode);

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
        "proxy" => {
            run_proxy_server(config).await;
        }
        _ => {
            run_stdio_server(
                config,
                settings,
                facade.expect("stdio serve requires an already-loaded IndexFacade"),
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

async fn run_proxy_server(config: Settings) {
    // Proxy mode - stdio-facing delegate that discovers/spawns a backing
    // `codanna serve --http` and relays MCP traffic to it. No IndexFacade is
    // constructed in this process (§4.5).
    eprintln!("Starting MCP server in proxy mode (stdio <-> HTTP delegate)");

    if let Err(e) = crate::mcp::proxy::serve_proxy(config).await {
        eprintln!("Proxy server error: {e}");
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
    // The process::exit arms below must drop it explicitly: exit skips
    // destructors and would leave the lockfile behind.
    let serve_lock = match PidLockGuard::acquire(&index_path.join("serve.lock")) {
        Ok(guard) => guard,
        Err(PidLockError::Held { pid, lock_path }) => {
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
        Err(PidLockError::Io(e)) => {
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
    let service = match server.serve(stdio()).await {
        Ok(service) => service,
        Err(e) => {
            eprintln!("Failed to start MCP server: {e}");
            drop(serve_lock);
            std::process::exit(1);
        }
    };

    // Wait for server to complete
    if let Err(e) = service.waiting().await {
        eprintln!("MCP server error: {e}");
        drop(serve_lock);
        std::process::exit(1);
    }
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
mod server_mode_selection_tests {
    use super::resolve_server_mode;

    // These tests exercise `resolve_server_mode` in isolation: it is a pure
    // function of (https, http, proxy, config_mode) -> &'static str with no
    // I/O, so precedence can be asserted hermetically without spawning any
    // process or binding any port.
    //
    // The stdio<->HTTP delegating pump in `mcp::proxy` is intentionally NOT
    // unit tested here: exercising it for real requires a live backing HTTP
    // MCP server (a spawned child process) and a connected stdio client on
    // the other end, which is an integration/manual validation concern, not
    // a hermetic unit test.

    #[test]
    fn cli_proxy_flag_selects_proxy() {
        assert_eq!(resolve_server_mode(false, false, true, "stdio"), "proxy");
    }

    #[test]
    fn config_mode_proxy_selects_proxy_with_bare_serve() {
        // Bare `codanna serve` (no CLI transport flags) must be able to
        // default to proxy via settings.toml `server.mode = "proxy"`.
        assert_eq!(resolve_server_mode(false, false, false, "proxy"), "proxy");
    }

    #[test]
    fn cli_http_flag_wins_over_config_mode_proxy() {
        assert_eq!(resolve_server_mode(false, true, false, "proxy"), "http");
    }

    #[test]
    fn cli_https_flag_wins_over_everything() {
        assert_eq!(resolve_server_mode(true, false, false, "proxy"), "https");
        assert_eq!(resolve_server_mode(true, false, true, "http"), "https");
    }

    #[test]
    fn config_mode_http_selects_http_without_cli_flags() {
        assert_eq!(resolve_server_mode(false, false, false, "http"), "http");
    }

    #[test]
    fn default_is_stdio() {
        assert_eq!(resolve_server_mode(false, false, false, "stdio"), "stdio");
    }
}
