//! CodeIntelligenceServer: construction, server plumbing, custom requests.

use rmcp::model::ErrorData as McpError;
use rmcp::model::*;
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    service::{Peer, RequestContext, RoleServer, ServiceError},
    tool_handler,
};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::Settings;
use crate::documents::DocumentStore;
use crate::indexing::facade::IndexFacade;

/// Generate guidance for MCP tool responses
pub(crate) fn generate_mcp_guidance(
    settings: &Settings,
    tool: &str,
    result_count: usize,
) -> Option<String> {
    use crate::io::guidance_engine::generate_guidance_from_config;
    generate_guidance_from_config(&settings.guidance, tool, None, result_count)
}

/// Format a Unix timestamp as relative time (e.g., "2 hours ago")
pub fn format_relative_time(timestamp: u64) -> String {
    use chrono::{DateTime, Utc};

    let now = Utc::now();
    let then = DateTime::from_timestamp(timestamp as i64, 0).unwrap_or_else(Utc::now);

    let diff = (now.timestamp() - then.timestamp()) as u64;

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        let mins = diff / 60;
        format!("{} minute{} ago", mins, if mins == 1 { "" } else { "s" })
    } else if diff < 86400 {
        let hours = diff / 3600;
        format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
    } else if diff < 604800 {
        let days = diff / 86400;
        format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
    } else {
        // For older dates, show the actual formatted date
        then.format("%Y-%m-%d").to_string()
    }
}

#[derive(Clone)]
pub struct CodeIntelligenceServer {
    pub facade: Arc<RwLock<IndexFacade>>,
    pub document_store: Option<Arc<RwLock<DocumentStore>>>,
    tool_router: ToolRouter<Self>,
    pub(super) peer: Arc<Mutex<Option<Peer<RoleServer>>>>,
    broadcaster: Option<Arc<crate::mcp::notifications::NotificationBroadcaster>>,
}

impl CodeIntelligenceServer {
    pub fn new(facade: IndexFacade) -> Self {
        Self {
            facade: Arc::new(RwLock::new(facade)),
            document_store: None,
            tool_router: Self::symbols_router() + Self::search_router(),
            peer: Arc::new(Mutex::new(None)),
            broadcaster: None,
        }
    }

    /// Create server from an already-loaded facade (most efficient)
    pub fn from_facade(facade: Arc<RwLock<IndexFacade>>) -> Self {
        Self {
            facade,
            document_store: None,
            tool_router: Self::symbols_router() + Self::search_router(),
            peer: Arc::new(Mutex::new(None)),
            broadcaster: None,
        }
    }

    /// Create server with existing facade and settings (for HTTP server)
    pub fn new_with_facade(facade: Arc<RwLock<IndexFacade>>, _settings: Arc<Settings>) -> Self {
        Self {
            facade,
            document_store: None,
            tool_router: Self::symbols_router() + Self::search_router(),
            peer: Arc::new(Mutex::new(None)),
            broadcaster: None,
        }
    }

    /// Wire the watch-lane broadcaster; enables `subscriptions/listen`.
    pub fn with_broadcaster(
        mut self,
        broadcaster: Arc<crate::mcp::notifications::NotificationBroadcaster>,
    ) -> Self {
        self.broadcaster = Some(broadcaster);
        self
    }

    /// Add document store for document search capability
    pub fn with_document_store(mut self, store: DocumentStore) -> Self {
        self.document_store = Some(Arc::new(RwLock::new(store)));
        self
    }

    /// Add document store from existing Arc (for sharing with watcher)
    pub fn with_document_store_arc(mut self, store: Arc<RwLock<DocumentStore>>) -> Self {
        self.document_store = Some(store);
        self
    }

    /// Get a reference to the facade Arc for external management (e.g., hot-reload)
    pub fn get_facade_arc(&self) -> Arc<RwLock<IndexFacade>> {
        self.facade.clone()
    }

    /// Send a notification when a file is re-indexed
    pub async fn notify_file_reindexed(&self, file_path: &str) {
        let peer_guard = self.peer.lock().await;
        if let Some(peer) = peer_guard.as_ref() {
            // Send a resource updated notification
            let _ = peer
                .notify_resource_updated(ResourceUpdatedNotificationParam::new(format!(
                    "file://{file_path}"
                )))
                .await;

            // Also send a logging message for visibility. Logging is deprecated by
            // SEP-2577; keep emitting it for client compatibility until rmcp removes it.
            #[allow(deprecated)]
            let _ = peer
                .notify_logging_message(
                    LoggingMessageNotificationParam::new(
                        LoggingLevel::Info,
                        serde_json::json!({
                            "action": "re-indexed",
                            "file": file_path
                        }),
                    )
                    .with_logger("codanna"),
                )
                .await;
        }
    }
}

/// Cache lifetime for list results. The tool list is static per
/// binary; `toolsListChanged` covers upgrades.
pub(crate) const LIST_CACHE_TTL_MS: u64 = 3_600_000;

#[tool_handler(router = self.tool_router)]
impl ServerHandler for CodeIntelligenceServer {
    // Suppresses the tool_handler-generated list_tools, which leaves
    // ttl_ms/cache_scope unset; 2026-07-28 requires both on list results.
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListToolsResult {
            result_type: Some(rmcp::model::ResultType::COMPLETE),
            tools: self.tool_router.list_all(),
            meta: None,
            next_cursor: None,
            ttl_ms: Some(LIST_CACHE_TTL_MS),
            cache_scope: Some(rmcp::model::CacheScope::Private),
        })
    }

    // The watch lane emits resource-level changes only; tool and prompt
    // categories are never accepted.
    fn accepted_subscription_filter(
        &self,
        requested: &rmcp::model::SubscriptionFilter,
    ) -> Option<rmcp::model::SubscriptionFilter> {
        let mut accepted = requested.clone();
        accepted.tools_list_changed = None;
        accepted.prompts_list_changed = None;
        Some(accepted)
    }

    async fn listen(
        &self,
        context: rmcp::service::SubscriptionContext,
    ) -> Result<(), rmcp::ErrorData> {
        use rmcp::service::SubscriptionSendError;
        use tokio::sync::broadcast::error::RecvError;

        let Some(broadcaster) = self.broadcaster.as_ref() else {
            // No watch lane wired (serve without file watching): hold the
            // stream open until the client cancels; nothing will flow.
            context.cancelled().await;
            return Ok(());
        };
        let mut events = broadcaster.subscribe();
        let sink = context.sink();

        loop {
            let send_result = tokio::select! {
                _ = context.cancelled() => break,
                event = events.recv() => match event {
                    Ok(crate::mcp::notifications::FileChangeEvent::FileReindexed { path }) => {
                        sink.notify_resource_updated(format!("file://{}", path.display())).await
                    }
                    Ok(_) => sink.notify_resource_list_changed().await,
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                },
            };
            match send_result {
                Ok(()) => {}
                // The client did not opt in to this category or URI;
                // the filter is doing its job, keep the stream open.
                Err(SubscriptionSendError::NotificationNotAccepted(_)) => {}
                Err(SubscriptionSendError::SubscriptionClosed) => break,
                Err(e) => {
                    tracing::debug!(target: "mcp", "listen send failed: {e}");
                    break;
                }
            }
        }
        Ok(())
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_resources_list_changed()
                .enable_resources_subscribe()
                .build(),
        )
        .with_server_info(
            Implementation::new("codanna", env!("CARGO_PKG_VERSION"))
                .with_title("Codanna Code Intelligence")
                .with_website_url("https://github.com/bartolli/codanna"),
        )
        .with_instructions(
            "This server provides code intelligence tools for analyzing this codebase. \
            WORKFLOW: Start with 'semantic_search_with_context' or 'semantic_search_docs' to anchor on the right files and APIs - they provide the highest-quality context. \
            Then use 'find_symbol' and 'search_symbols' to lock onto exact files and kinds. \
            Treat 'get_calls', 'find_callers', and 'analyze_impact' as hints; confirm with code reading or tighter queries (unique names, kind filters). \
            Use 'search_documents' to find relevant project documentation (markdown files). \
            Use 'get_index_info' to understand what's indexed.",
        )
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        // Register client capabilities (required for MCP handshake)
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }

        // Store the peer reference for sending notifications
        let mut peer_guard = self.peer.lock().await;
        *peer_guard = Some(context.peer.clone());

        // Return the server info
        Ok(self.get_info())
    }

    async fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, McpError> {
        match request.method.as_str() {
            "requests/codanna/force-reindex" => self.handle_force_reindex(request).await,
            "requests/codanna/index-stats" => self.handle_index_stats().await,
            _ => Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("Unknown method: {}", request.method),
                None,
            )),
        }
    }
}

// Custom request handlers
impl CodeIntelligenceServer {
    /// Handle force-reindex request
    async fn handle_force_reindex(&self, request: CustomRequest) -> Result<CustomResult, McpError> {
        use std::time::Instant;

        let start = Instant::now();

        // Parse optional paths parameter
        let paths: Option<Vec<String>> = request
            .params
            .as_ref()
            .and_then(|p| p.get("paths"))
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let mut indexer = self.facade.write().await;

        let (reindexed, symbols) = if let Some(paths) = paths {
            // Reindex specific paths
            let mut total_reindexed = 0;
            for path in &paths {
                let path = std::path::Path::new(path);
                if path.is_file() {
                    match indexer.index_file(path) {
                        Ok(crate::IndexingResult::Indexed(_)) => total_reindexed += 1,
                        Ok(crate::IndexingResult::Cached(_)) => {}
                        Err(e) => {
                            tracing::warn!("Failed to reindex {}: {e}", path.display());
                        }
                    }
                } else if path.is_dir() {
                    match indexer.index_directory(path, false) {
                        Ok(stats) => total_reindexed += stats.files_indexed,
                        Err(e) => {
                            tracing::warn!("Failed to reindex {}: {e}", path.display());
                        }
                    }
                }
            }
            (total_reindexed, indexer.symbol_count())
        } else {
            // Full reindex using indexed_paths from settings
            let indexed_paths = indexer.settings().indexing.indexed_paths.clone();
            let mut total_reindexed = 0;

            for path in &indexed_paths {
                if path.is_dir() {
                    match indexer.index_directory(path, false) {
                        Ok(stats) => total_reindexed += stats.files_indexed,
                        Err(e) => {
                            tracing::warn!("Failed to reindex {}: {e}", path.display());
                        }
                    }
                }
            }
            (total_reindexed, indexer.symbol_count())
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(CustomResult(serde_json::json!({
            "reindexed": reindexed,
            "symbols": symbols,
            "duration_ms": duration_ms
        })))
    }

    /// Handle index-stats request
    async fn handle_index_stats(&self) -> Result<CustomResult, McpError> {
        let indexer = self.facade.read().await;

        let semantic = if let Some(metadata) = indexer.get_semantic_metadata() {
            let live_count = indexer.semantic_search_embedding_count();
            serde_json::json!({
                "enabled": true,
                "model": metadata.model_name,
                "embeddings": live_count,
                "dimensions": metadata.dimension
            })
        } else {
            serde_json::json!({
                "enabled": false
            })
        };

        Ok(CustomResult(serde_json::json!({
            "symbols": indexer.symbol_count(),
            "files": indexer.file_count(),
            "relationships": indexer.relationship_count(),
            "semantic": semantic
        })))
    }

    /// Send a custom notification to the connected client
    pub async fn notify_custom(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), ServiceError> {
        let peer_guard = self.peer.lock().await;
        if let Some(peer) = peer_guard.as_ref() {
            peer.send_notification(ServerNotification::CustomNotification(
                CustomNotification::new(method, Some(params)),
            ))
            .await?;
        }
        Ok(())
    }
}
