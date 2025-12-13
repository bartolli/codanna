//! HTTPS server implementation for MCP using streamable HTTP transport with TLS
//!
//! Provides a secure HTTPS server with TLS support for MCP communication.
//! Uses streamable HTTP transport which is compatible with Claude Code.

#[cfg(feature = "https-server")]
pub async fn serve_https(config: crate::Settings, watch: bool, bind: String) -> anyhow::Result<()> {
    use crate::mcp::{
        CodeIntelligenceServer, notifications::NotificationBroadcaster, watcher::IndexWatcher,
    };
    use crate::{IndexPersistence, SimpleIndexer};
    use anyhow::Context;
    use axum::Router;
    use axum_server::tls_rustls::RustlsConfig;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio_util::sync::CancellationToken;

    if config.mcp.debug {
        eprintln!("Starting HTTPS MCP server on {bind}");
    }

    // Create notification broadcaster for file change events
    let broadcaster = Arc::new(NotificationBroadcaster::new(100).with_debug(config.mcp.debug));

    // Create shared indexer
    let indexer = Arc::new(RwLock::new(SimpleIndexer::with_settings(Arc::new(
        config.clone(),
    ))));

    // Load existing index if available
    let persistence = IndexPersistence::new(config.index_path.clone());
    if persistence.exists() {
        match persistence.load_with_settings(Arc::new(config.clone()), false) {
            Ok(loaded_indexer) => {
                let mut indexer_guard = indexer.write().await;
                *indexer_guard = loaded_indexer;
                let symbol_count = indexer_guard.symbol_count();
                drop(indexer_guard);
                if config.mcp.debug {
                    eprintln!("Loaded index with {symbol_count} symbols");
                }
            }
            Err(e) => {
                if config.mcp.debug {
                    eprintln!("Failed to load existing index: {e}");
                    eprintln!("Starting with empty index");
                }
            }
        }
    } else if config.mcp.debug {
        eprintln!("No existing index found, starting fresh");
    }

    // Create cancellation token for graceful shutdown
    let ct = CancellationToken::new();

    // Start file watcher if enabled
    if watch || config.file_watch.enabled {
        use crate::indexing::FileSystemWatcher;

        let watcher_indexer = indexer.clone();
        let watcher_broadcaster = broadcaster.clone();
        let debounce_ms = config.file_watch.debounce_ms;
        let watcher_ct = ct.clone();

        match FileSystemWatcher::new(
            watcher_indexer,
            debounce_ms,
            config.mcp.debug,
            &config.index_path,
        ) {
            Ok(watcher) => {
                let watcher = watcher.with_broadcaster(watcher_broadcaster);
                tokio::spawn(async move {
                    tokio::select! {
                        result = watcher.watch() => {
                            if let Err(e) = result {
                                eprintln!("File watcher error: {e}");
                            }
                        }
                        _ = watcher_ct.cancelled() => {
                            eprintln!("File watcher stopped by cancellation token");
                        }
                    }
                });
                if config.mcp.debug {
                    eprintln!(
                        "File system watcher started (event-driven with {debounce_ms}ms debounce)"
                    );
                }
            }
            Err(e) => {
                eprintln!("Failed to start file watcher: {e}");
                eprintln!("Continuing without file watching");
            }
        }

        // Start config file watcher (watches settings.toml for indexed_paths changes)
        use crate::indexing::ConfigFileWatcher;

        let config_watcher_indexer = indexer.clone();
        let config_watcher_broadcaster = broadcaster.clone();
        let settings_path = config
            .workspace_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .join(".codanna/settings.toml");

        match ConfigFileWatcher::new(
            settings_path.clone(),
            config_watcher_indexer,
            config.mcp.debug,
        ) {
            Ok(config_watcher) => {
                let config_watcher = config_watcher.with_broadcaster(config_watcher_broadcaster);
                let config_watcher_ct = ct.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        result = config_watcher.watch() => {
                            if let Err(e) = result {
                                eprintln!("Config watcher error: {e}");
                            }
                        }
                        _ = config_watcher_ct.cancelled() => {
                            eprintln!("Config watcher stopped by cancellation token");
                        }
                    }
                });
                eprintln!(
                    "Config watcher started - monitoring {}",
                    settings_path.display()
                );
            }
            Err(e) => {
                eprintln!("Failed to start config watcher: {e}");
            }
        }
    }

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
                    eprintln!("Index watcher ended");
                }
                _ = index_watcher_ct.cancelled() => {
                    eprintln!("Index watcher stopped by cancellation token");
                }
            }
        });

        if config.mcp.debug {
            eprintln!(
                "Index watcher started (checks every {watch_interval} seconds for index changes)"
            );
        }
    }

    // Create streamable HTTP service for MCP connections
    // Important: We share the SAME indexer instance across all connections
    // to ensure hot reload works properly. The indexer is already Arc<RwLock<_>>
    // so it's safe to share across connections.
    let indexer_for_service = indexer.clone();
    let config_for_service = Arc::new(config.clone());

    // Create a shared service instance that all connections will use
    let shared_service =
        CodeIntelligenceServer::new_with_indexer(indexer_for_service, config_for_service);

    let mcp_service = StreamableHttpService::new(
        move || {
            // Return a clone of the shared service
            // Since CodeIntelligenceServer derives Clone and the indexer is Arc<RwLock<_>>,
            // all clones will share the same underlying indexer
            Ok(shared_service.clone())
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            sse_keep_alive: Some(Duration::from_secs(15)),
            stateful_mode: true,
        },
    );

    // Create OAuth metadata handler with the bind address
    let bind_for_metadata = bind.clone();
    let oauth_metadata = move || async move {
        eprintln!("OAuth metadata endpoint called");
        axum::Json(serde_json::json!({
            "issuer": format!("https://{}", bind_for_metadata.clone()),
            "authorization_endpoint": format!("https://{}/oauth/authorize", bind_for_metadata.clone()),
            "token_endpoint": format!("https://{}/oauth/token", bind_for_metadata.clone()),
            "registration_endpoint": format!("https://{}/oauth/register", bind_for_metadata),
            "scopes_supported": ["mcp"],
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256", "plain"],
            "token_endpoint_auth_methods_supported": ["none"]
        }))
    };

    // Request logging middleware (OAuth authentication is optional for HTTPS)
    async fn log_requests(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> Result<axum::response::Response, axum::http::StatusCode> {
        let path = req.uri().path();
        eprintln!("Request to: {path}");

        // Debug: Print all headers
        eprintln!("Headers received:");
        for (name, value) in req.headers() {
            if let Ok(v) = value.to_str() {
                eprintln!("  {name}: {v}");
            }
        }

        // Pass through - TLS provides transport security
        Ok(next.run(req).await)
    }

    // Create MCP router with logging middleware
    let mcp_router_with_logging = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::middleware::from_fn(log_requests));

    // Create main router - OAuth endpoints available but optional for HTTPS
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
        // MCP endpoint - No authentication required (TLS provides transport security)
        .merge(mcp_router_with_logging);

    // Get or create TLS certificates
    let (cert_pem, key_pem) = get_or_create_certificate(&bind)
        .await
        .context("Failed to get or create TLS certificate")?;

    // Configure TLS
    let tls_config = RustlsConfig::from_pem(cert_pem, key_pem)
        .await
        .context("Failed to configure TLS")?;

    // Parse bind address
    let addr: SocketAddr = bind.parse().context("Failed to parse bind address")?;

    eprintln!("HTTPS MCP server listening on https://{bind}");
    eprintln!("MCP endpoint: https://{bind}/mcp");
    eprintln!("Health check: https://{bind}/health");
    eprintln!();
    eprintln!("Using self-signed certificate. Clients will show security warnings.");
    eprintln!("To trust the certificate, visit https://{bind} in your browser first");
    eprintln!();
    eprintln!("Press Ctrl+C to stop the server");

    // Serve with TLS
    let server = axum_server::bind_rustls(addr, tls_config).serve(router.into_make_service());

    // Handle graceful shutdown
    tokio::select! {
        result = server => {
            result?;
        }
        _ = shutdown_signal() => {
            eprintln!("Shutting down HTTPS server...");
            ct.cancel();
        }
    }

    eprintln!("HTTPS server shut down gracefully");
    Ok(())
}

/// Helper function for health check endpoint
#[cfg(feature = "https-server")]
async fn health_check() -> &'static str {
    eprintln!("Health check endpoint called");
    "OK"
}

/// OAuth register endpoint - accepts any registration
#[cfg(feature = "https-server")]
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

/// OAuth token endpoint - exchanges authorization code for access token
#[cfg(feature = "https-server")]
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

/// OAuth authorize endpoint - redirects back with auth code
#[cfg(feature = "https-server")]
async fn oauth_authorize(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
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
        .spinner {{
            margin: 20px auto;
            width: 50px;
            height: 50px;
            border: 3px solid #f3f3f3;
            border-top: 3px solid #667eea;
            border-radius: 50%;
            animation: spin 1s linear infinite;
        }}
        @keyframes spin {{
            0% {{ transform: rotate(0deg); }}
            100% {{ transform: rotate(360deg); }}
        }}
    </style>
    <script>
        // Auto-redirect after a short delay
        setTimeout(function() {{
            window.location.href = "{callback_url}";
        }}, 1500);
    </script>
</head>
<body>
    <div class="container">
        <h1>🔐 Authorize Codanna</h1>
        <div class="spinner"></div>
        <p>Authorizing access to Codanna MCP Server...</p>
        <p>You will be redirected automatically.</p>
        <button onclick="window.location.href='{callback_url}'">
            Continue Manually
        </button>
    </div>
</body>
</html>
"#
    );

    axum::response::Html(html)
}

/// Helper function for shutdown signal
#[cfg(feature = "https-server")]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    eprintln!("Received shutdown signal");
}

/// Get or create self-signed certificate for HTTPS
#[cfg(feature = "https-server")]
async fn get_or_create_certificate(bind: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    use anyhow::Context;
    use rcgen::generate_simple_self_signed;

    // Determine certificate storage directory
    let cert_dir = dirs::config_dir()
        .context("Failed to get config directory")?
        .join("codanna")
        .join("certs");

    let cert_path = cert_dir.join("server.pem");
    let key_path = cert_dir.join("server.key");

    // Create directory if it doesn't exist
    tokio::fs::create_dir_all(&cert_dir)
        .await
        .context("Failed to create certificate directory")?;

    // Check if server certificate already exists
    if cert_path.exists() && key_path.exists() {
        eprintln!("Loading existing certificates from {cert_dir:?}");
        let cert = tokio::fs::read(&cert_path)
            .await
            .context("Failed to read certificate file")?;
        let key = tokio::fs::read(&key_path)
            .await
            .context("Failed to read key file")?;
        return Ok((cert, key));
    }

    eprintln!("Generating new enhanced self-signed certificate...");

    // Build list of Subject Alternative Names
    let mut subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];

    // If binding to 0.0.0.0, include local network IP
    if bind.starts_with("0.0.0.0") {
        if let Ok(local_ip) = local_ip_address::local_ip() {
            eprintln!("Including local network IP in certificate: {local_ip}");
            subject_alt_names.push(local_ip.to_string());
        }
    }

    // Generate certificate using the simpler API but with better parameters
    let cert = generate_simple_self_signed(subject_alt_names.clone())
        .context("Failed to generate self-signed certificate")?;

    let cert_pem = cert.cert.pem().into_bytes();
    let key_pem = cert.signing_key.serialize_pem().into_bytes();

    // Save certificate and key
    tokio::fs::write(&cert_path, &cert_pem)
        .await
        .context("Failed to write server certificate")?;
    tokio::fs::write(&key_path, &key_pem)
        .await
        .context("Failed to write server key")?;

    // Calculate fingerprint
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    cert.cert.der().hash(&mut hasher);
    let fingerprint = hasher.finish();
    let fingerprint_hex = format!("{fingerprint:016X}");

    eprintln!();
    eprintln!("🔐 Certificate Details:");
    eprintln!("   - Type: Self-Signed TLS Certificate");
    eprintln!("   - Location: {}", cert_path.display());
    eprintln!("   - Fingerprint: {fingerprint_hex}");
    eprintln!("   - Valid for: {}", subject_alt_names.join(", "));
    eprintln!();
    eprintln!("🔧 To trust this certificate on macOS:");
    eprintln!();
    eprintln!("   Option 1: Command line (requires sudo):");
    eprintln!(
        "   sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain {}",
        cert_path.display()
    );
    eprintln!();
    eprintln!("   Option 2: GUI (recommended):");
    eprintln!("   1. Open Finder and navigate to: {}", cert_dir.display());
    eprintln!("   2. Double-click 'server.pem'");
    eprintln!("   3. Add to 'System' keychain");
    eprintln!("   4. Set to 'Always Trust' for SSL");
    eprintln!();
    eprintln!("   Option 3: Open in browser first:");
    eprintln!("   1. Visit https://127.0.0.1:8443/health in Safari/Chrome");
    eprintln!("   2. Click 'Advanced' and proceed anyway");
    eprintln!("   3. This may help some clients accept the certificate");
    eprintln!();
    eprintln!("⚠️  After trusting the certificate, restart Claude Code to reconnect");
    eprintln!();

    Ok((cert_pem, key_pem))
}

/// Helper function to detect local IP address
#[cfg(feature = "https-server")]
mod local_ip_address {
    use std::net::{IpAddr, UdpSocket};

    pub fn local_ip() -> Result<IpAddr, Box<dyn std::error::Error>> {
        // Connect to a dummy address to determine local IP
        // This doesn't actually send any packets, just determines
        // which network interface would be used for external traffic
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect("8.8.8.8:80")?;
        let addr = socket.local_addr()?;
        Ok(addr.ip())
    }
}

#[cfg(not(feature = "https-server"))]
pub async fn serve_https(
    _config: crate::Settings,
    _watch: bool,
    _bind: String,
) -> anyhow::Result<()> {
    eprintln!("HTTPS server support is not compiled in.");
    eprintln!("Please rebuild with: cargo build --features https-server");
    std::process::exit(1);
}
