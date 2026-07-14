//! stdio<->HTTP delegating MCP proxy.
//!
//! `codanna serve --proxy` (or `server.mode = "proxy"` in `settings.toml`)
//! speaks stdio to the connecting MCP client while delegating every request
//! to a backing `codanna serve --http` process, discovered or spawned via
//! [`crate::serve_discovery::discover_or_spawn`]. This lets several stdio
//! clients (e.g. multiple AI-tool subagents rooted at the same workspace)
//! share one HTTP-mode index/tantivy writer without each holding its own
//! `IndexFacade` -- the proxy process itself never constructs one.
//!
//! ## Scope for this PR
//!
//! - Request/response delegation across the full `ServerHandler` surface
//!   (tools, resources, prompts, completion, custom requests).
//! - Best-effort forwarding of server-initiated notifications (logging,
//!   resource/tool/prompt list-changed, resource-updated, progress) from the
//!   upstream HTTP server down to the stdio client.
//!
//! ## Explicitly out of scope
//!
//! A byte-level transparent transport relay -- splicing the stdio and HTTP
//! transports directly instead of round-tripping through typed rmcp
//! requests/responses -- is an optional later optimization. It would remove
//! one layer of (de)serialization but adds real complexity (framing,
//! backpressure, session lifecycle) that isn't justified until the
//! request/response delegation implemented here is proven in practice.

// Logging notifications and `set_level` are deprecated by SEP-2577, but this
// module forwards the full `ServerHandler`/`ClientHandler` surface
// (including logging) for client compatibility, mirroring the same
// allowance already used in `mcp::server` and `mcp::notifications`.
#![allow(deprecated)]

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientRequest, CompleteRequestParams, CompleteResult,
    CustomRequest, CustomResult, ErrorData as McpError, GetPromptRequestParams, GetPromptResult,
    Implementation, InitializeRequestParams, InitializeResult, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    LoggingMessageNotificationParam, PaginatedRequestParams, ProgressNotificationParam,
    ReadResourceRequestParams, ReadResourceResult, ResourceUpdatedNotificationParam,
    ServerCapabilities, ServerInfo, ServerResult, SetLevelRequestParams, SubscribeRequestParams,
    UnsubscribeRequestParams,
};
use rmcp::service::{
    NotificationContext, Peer, RequestContext, RoleClient, RoleServer, RunningService, ServiceError,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ClientHandler, ServerHandler, ServiceExt};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::config::Settings;
use crate::mcp::DUMMY_BEARER_TOKEN;
use crate::serve_discovery::{self, DiscoveryError, ServeScheme};
use crate::serve_tls;

/// Errors from establishing or running the stdio<->HTTP proxy.
#[derive(Debug, Error)]
pub enum ProxyError {
    #[error(
        "could not resolve workspace root: no '.codanna' directory found in the current directory or its ancestors"
    )]
    NoWorkspaceRoot,

    #[error("failed to discover/spawn backing HTTP server: {0}")]
    Discovery(#[from] DiscoveryError),

    #[error("failed to connect to backing HTTP server: {0}")]
    UpstreamConnect(String),

    #[error("stdio transport error: {0}")]
    Stdio(String),

    #[error("failed to build TLS-pinned client for backing HTTPS server: {0}")]
    Tls(#[from] crate::serve_tls::TlsClientError),
}

pub type ProxyResult<T> = Result<T, ProxyError>;

/// Converts an upstream `ServiceError` into the `McpError` shape expected by
/// `ServerHandler` methods. A `ServiceError::McpError` already carries a
/// well-formed protocol error and is passed through unchanged; every other
/// variant (transport closed, timeout, cancellation, ...) becomes an
/// internal error describing the underlying delegation failure.
fn map_service_err(err: ServiceError) -> McpError {
    match err {
        ServiceError::McpError(e) => e,
        other => McpError::internal_error(format!("proxy delegation failed: {other}"), None),
    }
}

/// `ClientHandler` for the connection to the backing HTTP MCP server.
///
/// Its only job is forwarding server-initiated notifications down to the
/// stdio client once the downstream `initialize` handshake has populated
/// `downstream`. Before that point (a narrow window right at startup)
/// notifications are dropped rather than buffered, since there is no
/// downstream peer yet to forward them to.
#[derive(Clone, Default)]
struct NotificationRelay {
    downstream: Arc<Mutex<Option<Peer<RoleServer>>>>,
}

impl ClientHandler for NotificationRelay {
    async fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        if let Some(peer) = self.downstream.lock().await.as_ref() {
            // Logging notifications are deprecated by SEP-2577; forward them
            // anyway for client compatibility, mirroring `CodeIntelligenceServer`.
            #[allow(deprecated)]
            let _ = peer.notify_logging_message(params).await;
        }
    }

    async fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        if let Some(peer) = self.downstream.lock().await.as_ref() {
            let _ = peer.notify_resource_updated(params).await;
        }
    }

    async fn on_resource_list_changed(&self, _context: NotificationContext<RoleClient>) {
        if let Some(peer) = self.downstream.lock().await.as_ref() {
            let _ = peer.notify_resource_list_changed().await;
        }
    }

    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        if let Some(peer) = self.downstream.lock().await.as_ref() {
            let _ = peer.notify_tool_list_changed().await;
        }
    }

    async fn on_prompt_list_changed(&self, _context: NotificationContext<RoleClient>) {
        if let Some(peer) = self.downstream.lock().await.as_ref() {
            let _ = peer.notify_prompt_list_changed().await;
        }
    }

    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        if let Some(peer) = self.downstream.lock().await.as_ref() {
            let _ = peer.notify_progress(params).await;
        }
    }
}

/// `ServerHandler` facing the stdio client. Every request is delegated to the
/// upstream HTTP MCP server; this process holds no `IndexFacade` and no
/// index state of its own.
#[derive(Clone)]
struct DelegatingProxyHandler {
    upstream: Arc<RunningService<RoleClient, NotificationRelay>>,
    downstream: Arc<Mutex<Option<Peer<RoleServer>>>>,
}

impl ServerHandler for DelegatingProxyHandler {
    fn get_info(&self) -> ServerInfo {
        // Reflect the upstream server's negotiated capabilities/info when
        // available (set during the upstream `initialize` handshake that
        // already completed by the time this proxy starts serving stdio);
        // fall back to a minimal description if it is somehow unset.
        self.upstream
            .peer_info()
            .map(|info| (*info).clone())
            .unwrap_or_else(|| {
                ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
                    .with_server_info(Implementation::new(
                        "codanna-proxy",
                        env!("CARGO_PKG_VERSION"),
                    ))
            })
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }

        *self.downstream.lock().await = Some(context.peer.clone());

        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.upstream
            .list_tools(request)
            .await
            .map_err(map_service_err)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.upstream
            .call_tool(request)
            .await
            .map_err(map_service_err)
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        self.upstream
            .list_resources(request)
            .await
            .map_err(map_service_err)
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        self.upstream
            .list_resource_templates(request)
            .await
            .map_err(map_service_err)
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        self.upstream
            .read_resource(request)
            .await
            .map_err(map_service_err)
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        self.upstream
            .list_prompts(request)
            .await
            .map_err(map_service_err)
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        self.upstream
            .get_prompt(request)
            .await
            .map_err(map_service_err)
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, McpError> {
        self.upstream
            .complete(request)
            .await
            .map_err(map_service_err)
    }

    async fn set_level(
        &self,
        request: SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.upstream
            .set_level(request)
            .await
            .map_err(map_service_err)
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.upstream
            .subscribe(request)
            .await
            .map_err(map_service_err)
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.upstream
            .unsubscribe(request)
            .await
            .map_err(map_service_err)
    }

    async fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, McpError> {
        match self
            .upstream
            .peer()
            .send_request(ClientRequest::CustomRequest(request))
            .await
        {
            Ok(ServerResult::CustomResult(custom)) => Ok(custom),
            Ok(other) => Err(McpError::internal_error(
                format!("unexpected upstream response to custom request: {other:?}"),
                None,
            )),
            Err(e) => Err(map_service_err(e)),
        }
    }
}

/// Run the stdio<->HTTP delegating proxy until the stdio transport closes.
///
/// No `IndexFacade` is constructed in this process: discovery/spawn of the
/// backing HTTP server (and all index state) lives entirely in the process
/// `serve_discovery::discover_or_spawn` finds or launches.
pub async fn serve_proxy(config: Settings) -> ProxyResult<()> {
    let workspace_root =
        serve_discovery::resolve_workspace_root(&config).ok_or(ProxyError::NoWorkspaceRoot)?;

    eprintln!(
        "Proxy: discovering backing HTTP MCP server for {}",
        workspace_root.display()
    );
    let record = serve_discovery::discover_or_spawn(&workspace_root, &config).await?;
    eprintln!(
        "Proxy: delegating to backing MCP server at {}://127.0.0.1:{} (pid {})",
        record.scheme.as_str(),
        record.port,
        record.pid
    );

    let transport_config = StreamableHttpClientTransportConfig::with_uri(format!(
        "{}://127.0.0.1:{}/mcp",
        record.scheme.as_str(),
        record.port
    ))
    .auth_header(DUMMY_BEARER_TOKEN);

    let downstream: Arc<Mutex<Option<Peer<RoleServer>>>> = Arc::new(Mutex::new(None));
    let relay = NotificationRelay {
        downstream: downstream.clone(),
    };

    let upstream = match record.scheme {
        // `from_config` uses rmcp's own bundled reqwest client (gated behind
        // the `transport-streamable-http-client-reqwest` feature) rather than
        // a hand-rolled HTTP client, per the preference for rmcp's default
        // client transport.
        ServeScheme::Http => {
            let transport = StreamableHttpClientTransport::from_config(transport_config);
            relay
                .serve(transport)
                .await
                .map_err(|e| ProxyError::UpstreamConnect(e.to_string()))?
        }
        // The backing server is `--https`: dial it ONLY through the
        // cert-pinning client (`serve_tls::pinned_client`), never through
        // `from_config`'s bundled client. A pinning failure (missing/mismatched
        // persisted cert) must fail the proxy outright rather than silently
        // falling back to an unauthenticated/plaintext-trusting transport.
        ServeScheme::Https => {
            let client = serve_tls::pinned_client()?;
            let transport = StreamableHttpClientTransport::with_client(client, transport_config);
            relay
                .serve(transport)
                .await
                .map_err(|e| ProxyError::UpstreamConnect(e.to_string()))?
        }
    };

    let handler = DelegatingProxyHandler {
        upstream: Arc::new(upstream),
        downstream,
    };

    use rmcp::transport::stdio;
    let service = handler
        .serve(stdio())
        .await
        .map_err(|e| ProxyError::Stdio(e.to_string()))?;

    service
        .waiting()
        .await
        .map_err(|e| ProxyError::Stdio(e.to_string()))?;

    Ok(())
}
