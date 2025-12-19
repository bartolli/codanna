//! HTTP server implementation for MCP
//!
//! Provides a persistent HTTP server with streamable HTTP transport
//! for multiple concurrent clients and real-time updates.

#[cfg(feature = "http-server")]
pub async fn serve_http(config: crate::Settings, watch: bool, bind: String) -> anyhow::Result<()> {
    use crate::mcp::{
        CodeIntelligenceServer, notifications::NotificationBroadcaster, watcher::IndexWatcher,
    };
    use crate::{IndexPersistence, SimpleIndexer};
    use axum::Router;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio_util::sync::CancellationToken;

    // Initialize logging with config
    crate::logging::init_with_config(&config.logging);

    crate::log_event!("http", "starting", "MCP server on {bind}");

    // Create notification broadcaster for file change events
    let broadcaster = Arc::new(NotificationBroadcaster::new(100));

    // Create shared indexer
    let indexer = Arc::new(RwLock::new(SimpleIndexer::with_settings(Arc::new(
        config.clone(),
    ))));

    // Load existing index if available
    let persistence = IndexPersistence::new(config.index_path.clone());
    if persistence.exists() {
        match persistence.load_with_settings(Arc::new(config.clone())) {
            Ok(loaded) => {
                let mut indexer_guard = indexer.write().await;
                *indexer_guard = loaded;
                let symbol_count = indexer_guard.symbol_count();
                drop(indexer_guard);
                crate::log_event!("http", "loaded", "{symbol_count} symbols");
            }
            Err(e) => {
                tracing::warn!("[http] failed to load index: {e}");
                crate::log_event!("http", "starting", "empty index");
            }
        }
    } else {
        crate::log_event!("http", "starting", "no existing index");
    }

    // Create cancellation token for coordinated shutdown
    let ct = CancellationToken::new();

    // Start index watcher if watch mode is enabled
    if watch {
        let index_watcher_indexer = indexer.clone();
        let index_watcher_settings = Arc::new(config.clone());
        let index_watcher_broadcaster = broadcaster.clone();
        let index_watcher_ct = ct.clone();

        // Default to 5 second interval
        let watch_interval = 5u64;

        let index_watcher = IndexWatcher::new(
            index_watcher_indexer,
            index_watcher_settings,
            Duration::from_secs(watch_interval),
        )
        .with_broadcaster(index_watcher_broadcaster);

        tokio::spawn(async move {
            tokio::select! {
                _ = index_watcher.watch() => {
                    crate::log_event!("index-watcher", "ended");
                }
                _ = index_watcher_ct.cancelled() => {
                    crate::log_event!("index-watcher", "stopped");
                }
            }
        });

        crate::log_event!(
            "index-watcher",
            "started",
            "polling every {watch_interval}s"
        );
    }

    // Start unified file watcher if enabled
    if watch || config.file_watch.enabled {
        use crate::documents::DocumentStore;
        use crate::vector::{EmbeddingGenerator, FastEmbedGenerator};
        use crate::watcher::UnifiedWatcher;
        use crate::watcher::handlers::{CodeFileHandler, ConfigFileHandler, DocumentFileHandler};

        let workspace_root = config
            .workspace_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let settings_path = workspace_root.join(".codanna/settings.toml");
        let debounce_ms = config.file_watch.debounce_ms;

        // Build unified watcher with handlers
        let mut builder = UnifiedWatcher::builder()
            .broadcaster(broadcaster.clone())
            .indexer(indexer.clone())
            .index_path(config.index_path.clone())
            .workspace_root(workspace_root.clone())
            .debounce_ms(debounce_ms);

        // Add code file handler
        builder = builder.handler(CodeFileHandler::new(
            indexer.clone(),
            workspace_root.clone(),
        ));

        // Add config file handler
        match ConfigFileHandler::new(settings_path.clone()) {
            Ok(config_handler) => {
                builder = builder.handler(config_handler);
            }
            Err(e) => {
                tracing::warn!("[config] failed to create handler: {e}");
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

        // Build and start the unified watcher
        match builder.build() {
            Ok(unified_watcher) => {
                let watcher_ct = ct.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        result = unified_watcher.watch() => {
                            if let Err(e) = result {
                                tracing::error!("[watcher] error: {e}");
                            }
                        }
                        _ = watcher_ct.cancelled() => {
                            crate::log_event!("watcher", "stopped");
                        }
                    }
                });
                crate::log_event!(
                    "watcher",
                    "started",
                    "debounce: {debounce_ms}ms, config: {}",
                    settings_path.display()
                );
            }
            Err(e) => {
                tracing::warn!("[watcher] failed to start: {e}");
                tracing::warn!("[watcher] continuing without file watching");
            }
        }
    }

    // Create streamable HTTP service for MCP connections
    let indexer_for_service = indexer.clone();
    let config_for_service = Arc::new(config.clone());
    let broadcaster_for_service = broadcaster.clone();
    let ct_for_service = ct.clone();

    let mcp_service = StreamableHttpService::new(
        move || {
            crate::debug_event!("mcp", "creating server instance");
            let server = CodeIntelligenceServer::new_with_indexer(
                indexer_for_service.clone(),
                config_for_service.clone(),
            );

            // Start notification listener for this connection
            // Note: We need to wait for initialize() to be called first
            let server_clone = server.clone();
            let receiver = broadcaster_for_service.subscribe();
            let listener_ct = ct_for_service.clone();
            crate::debug_event!("mcp", "subscribing to broadcaster");
            tokio::spawn(async move {
                // Wait a bit for the MCP handshake to complete
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                crate::debug_event!("mcp", "notification listener started");

                // Run listener until cancelled
                tokio::select! {
                    _ = server_clone.start_notification_listener(receiver) => {
                        crate::debug_event!("mcp", "notification listener ended");
                    }
                    _ = listener_ct.cancelled() => {
                        crate::debug_event!("mcp", "notification listener stopped");
                    }
                }
            });

            Ok(server)
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            sse_keep_alive: Some(Duration::from_secs(15)),
            stateful_mode: true,
        },
    );

    // Helper function for health check endpoint
    async fn health_check() -> &'static str {
        "OK"
    }

    // Create OAuth metadata handler with the bind address
    let bind_for_metadata = bind.clone();
    let oauth_metadata = move || async move {
        eprintln!("OAuth metadata endpoint called");
        // Return OAuth metadata that supports authorization code flow
        axum::Json(serde_json::json!({
            "issuer": format!("http://{}", bind_for_metadata.clone()),
            "authorization_endpoint": format!("http://{}/oauth/authorize", bind_for_metadata.clone()),
            "token_endpoint": format!("http://{}/oauth/token", bind_for_metadata.clone()),
            "registration_endpoint": format!("http://{}/oauth/register", bind_for_metadata),
            "scopes_supported": ["mcp"],
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256", "plain"],
            "token_endpoint_auth_methods_supported": ["none"]
        }))
    };

    // Dummy OAuth register endpoint - accepts any registration
    async fn oauth_register(
        axum::Json(payload): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        eprintln!("OAuth register endpoint called with: {payload:?}");
        // Return a dummy client registration response that matches the request
        // Use empty string for public clients (Claude Code expects a string, not null)
        axum::Json(serde_json::json!({
            "client_id": "dummy-client-id",
            "client_secret": "",  // Empty string for public client
            "client_id_issued_at": 1234567890,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "redirect_uris": payload.get("redirect_uris").unwrap_or(&serde_json::json!([])).clone(),
            "client_name": payload.get("client_name").unwrap_or(&serde_json::json!("MCP Client")).clone(),
            "token_endpoint_auth_method": "none"
        }))
    }

    // OAuth token endpoint - exchanges authorization code for access token
    async fn oauth_token(body: String) -> axum::Json<serde_json::Value> {
        eprintln!("OAuth token endpoint called with body: {body}");

        // Parse form-encoded data (OAuth uses application/x-www-form-urlencoded)
        let params: std::collections::HashMap<String, String> =
            serde_urlencoded::from_str(&body).unwrap_or_default();

        eprintln!("Token request params: {params:?}");

        // Check grant type
        let grant_type = params.get("grant_type").cloned().unwrap_or_default();
        let code = params.get("code").cloned().unwrap_or_default();

        // IMPORTANT: Reject refresh_token grant type (like the SDK example)
        if grant_type == "refresh_token" {
            eprintln!("Rejecting refresh_token grant type");
            return axum::Json(serde_json::json!({
                "error": "unsupported_grant_type",
                "error_description": "only authorization_code is supported"
            }));
        }

        // For authorization_code grant, verify the code
        if grant_type == "authorization_code" && code == "dummy-auth-code" {
            // Return access token WITHOUT refresh token
            axum::Json(serde_json::json!({
                "access_token": "mcp-access-token-dummy",
                "token_type": "Bearer",
                "expires_in": 3600,
                "scope": "mcp"
            }))
        } else {
            // Invalid request
            eprintln!("Invalid token request: grant_type={grant_type}, code={code}");
            axum::Json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Invalid authorization code or grant type"
            }))
        }
    }

    // Dummy OAuth authorize endpoint - redirects back with auth code
    async fn oauth_authorize(
        axum::extract::Query(params): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
    ) -> impl axum::response::IntoResponse {
        eprintln!("OAuth authorize endpoint called with params: {params:?}");

        // Extract redirect_uri and state from query params
        let redirect_uri = params
            .get("redirect_uri")
            .cloned()
            .unwrap_or_else(|| "http://localhost:3118/callback".to_string());
        let state = params.get("state").cloned().unwrap_or_default();

        // Build the callback URL with authorization code
        let callback_url = format!("{redirect_uri}?code=dummy-auth-code&state={state}");

        // Return HTML with auto-redirect and manual button
        let html = format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <title>Authorize Codanna</title>
    <meta charset="utf-8">
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        }}
        .container {{
            background: white;
            padding: 2rem;
            border-radius: 10px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.2);
            text-align: center;
            max-width: 400px;
        }}
        h1 {{
            color: #333;
            margin-bottom: 1rem;
        }}
        p {{
            color: #666;
            margin-bottom: 2rem;
        }}
        button {{
            background: #667eea;
            color: white;
            border: none;
            padding: 12px 30px;
            border-radius: 5px;
            font-size: 16px;
            cursor: pointer;
            transition: background 0.3s;
        }}
        button:hover {{
            background: #764ba2;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🔐 Authorize Codanna</h1>
        <p>Grant access to Claude Code?</p>
        <p>Click Continue to complete the authorization.</p>
        <button onclick="window.location.href='{callback_url}'">Continue</button>
    </div>
</body>
</html>
"#
        );

        axum::response::Html(html)
    }

    // Helper function for shutdown signal with cancellation token
    async fn shutdown_signal() {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c");
        eprintln!("Received shutdown signal");
    }

    // Bearer token validation middleware - only for MCP endpoints
    async fn validate_bearer_token(
        req: axum::http::Request<axum::body::Body>,
        next: axum::middleware::Next,
    ) -> Result<axum::response::Response, axum::http::StatusCode> {
        // Check for Bearer token in Authorization header
        if let Some(auth_header) = req.headers().get("Authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                // Accept our dummy token
                if auth_str == "Bearer mcp-access-token-dummy" {
                    eprintln!("MCP request authorized with Bearer token");
                    return Ok(next.run(req).await);
                }
            }
        }

        // For OPTIONS requests (CORS preflight), allow without auth
        if req.method() == axum::http::Method::OPTIONS {
            return Ok(next.run(req).await);
        }

        eprintln!("MCP request rejected - invalid or missing Bearer token");
        Err(axum::http::StatusCode::UNAUTHORIZED)
    }

    // Create protected MCP router with Bearer token validation
    let protected_mcp_router = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::middleware::from_fn(validate_bearer_token));

    // Create main router - OAuth endpoints FIRST (no auth), then MCP endpoints (with auth)
    let router = Router::new()
        // OAuth endpoints - NO authentication required
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(oauth_metadata),
        )
        .route("/oauth/register", axum::routing::post(oauth_register))
        .route("/oauth/token", axum::routing::post(oauth_token))
        .route("/oauth/authorize", axum::routing::get(oauth_authorize))
        // Health check - NO authentication required
        .route("/health", axum::routing::get(health_check))
        // MCP endpoint - Bearer token authentication required
        .merge(protected_mcp_router);

    // Bind and serve
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("HTTP MCP server listening on http://{bind}");
    eprintln!("MCP endpoint: http://{bind}/mcp");
    eprintln!("Health check: http://{bind}/health");
    eprintln!("Press Ctrl+C to stop the server");

    // Create server future
    let server = axum::serve(listener, router);

    // Handle graceful shutdown with tokio::select!
    tokio::select! {
        result = server => {
            result?;
        }
        _ = shutdown_signal() => {
            eprintln!("Shutting down HTTP server...");
            ct.cancel();
        }
    }

    eprintln!("HTTP server shut down gracefully");
    Ok(())
}

#[cfg(not(feature = "http-server"))]
pub async fn serve_http(
    _config: crate::Settings,
    _watch: bool,
    _bind: String,
) -> anyhow::Result<()> {
    eprintln!("HTTP server support is not compiled in.");
    eprintln!("Please rebuild with: cargo build --features http-server");
    std::process::exit(1);
}
