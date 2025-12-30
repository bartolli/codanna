//! Serve command - MCP server modes (stdio, HTTP, HTTPS).

use std::path::PathBuf;
use std::sync::Arc;

use crate::SimpleIndexer;
use crate::config::Settings;

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
    indexer: SimpleIndexer,
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
                indexer,
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
    indexer: SimpleIndexer,
    index_path: PathBuf,
    watch: bool,
    actual_watch_interval: u64,
) {
    // stdio mode - current implementation
    eprintln!("Starting MCP server on stdio transport");
    if watch {
        eprintln!("Index watching enabled (interval: {actual_watch_interval}s)");
    }
    eprintln!("To test: npx @modelcontextprotocol/inspector cargo run -- serve");

    // Create MCP server using the already-loaded indexer
    tracing::debug!(
        target: "mcp",
        "creating server with indexer - symbols: {}, semantic: {}",
        indexer.symbol_count(),
        indexer.has_semantic_search()
    );
    let server = crate::mcp::CodeIntelligenceServer::new(indexer);

    // If watch mode is enabled, start the hot-reload watcher
    if watch {
        use crate::watcher::HotReloadWatcher;
        use std::time::Duration;

        let indexer_arc = server.get_indexer_arc();
        let watcher = HotReloadWatcher::new(
            indexer_arc,
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
        use crate::documents::DocumentStore;
        use crate::mcp::notifications::NotificationBroadcaster;
        use crate::vector::{EmbeddingGenerator, FastEmbedGenerator};
        use crate::watcher::UnifiedWatcher;
        use crate::watcher::handlers::{CodeFileHandler, ConfigFileHandler, DocumentFileHandler};
        use tokio::sync::RwLock;

        let broadcaster = Arc::new(NotificationBroadcaster::new(100));

        let workspace_root = config
            .workspace_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let settings_path = workspace_root.join(".codanna/settings.toml");
        let debounce_ms = config.file_watch.debounce_ms;
        let indexer_arc = server.get_indexer_arc();

        // Build unified watcher with handlers
        let mut builder = UnifiedWatcher::builder()
            .broadcaster(broadcaster.clone())
            .indexer(indexer_arc.clone())
            .index_path(index_path.clone())
            .workspace_root(workspace_root.clone())
            .debounce_ms(debounce_ms);

        // Add code file handler
        builder = builder.handler(CodeFileHandler::new(
            indexer_arc.clone(),
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

        // Add document handler if documents are enabled
        if config.documents.enabled {
            let doc_path = config.index_path.join("documents");
            if doc_path.exists() {
                if let Ok(generator) =
                    FastEmbedGenerator::from_settings(&config.semantic_search.model, false)
                {
                    let dimension = generator.dimension();
                    if let Ok(store) = DocumentStore::new(&doc_path, dimension) {
                        if let Ok(store_with_emb) = store.with_embeddings(Box::new(generator)) {
                            let store_arc = Arc::new(RwLock::new(store_with_emb));
                            builder = builder
                                .document_store(store_arc.clone())
                                .chunking_config(config.documents.defaults.clone())
                                .handler(DocumentFileHandler::new(
                                    store_arc,
                                    workspace_root.clone(),
                                ));
                        }
                    }
                }
            }
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
