//! MCP (Model Context Protocol) サーバー実装モジュール
//!
//! このモジュールは、AIアシスタントがコードインテリジェンスインデックスを
//! クエリできるようにするMCPツールを提供します。
//!
//! # アーキテクチャ
//!
//! MCPサーバーは2つのモードで実行できます：
//!
//! 1. **スタンドアロンサーバーモード**: `cargo run -- serve` で実行
//!    - インデックスを一度メモリにロード
//!    - stdioを介してクライアント接続をリッスン
//!    - AIアシスタントとの本番使用に効率的
//!
//! 2. **埋め込みモード**: CLIから直接使用
//!    - 別プロセス不要
//!    - 既にロードされたインデックスへの直接アクセス
//!    - CLI操作に最もメモリ効率的
//!
//! # 使用例
//!
//! ```no_run
//! // サーバーモードでの起動
//! // cargo run -- serve --stdio
//! ```

pub mod client;
pub mod http_server;
pub mod https_server;
pub mod notifications;
pub mod watcher;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ErrorData as McpError, *},
    schemars,
    service::{Peer, RequestContext, RoleServer},
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::{Settings, SimpleIndexer, Symbol};

/// Generate guidance for MCP tool responses
fn generate_mcp_guidance(settings: &Settings, tool: &str, result_count: usize) -> Option<String> {
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

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FindSymbolRequest {
    /// Name of the symbol to find
    pub name: String,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct GetCallsRequest {
    /// Name of the function to analyze (use symbol_id for unambiguous lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    /// Symbol ID for direct lookup (recommended to avoid ambiguity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FindCallersRequest {
    /// Name of the function to find callers for (use symbol_id for unambiguous lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    /// Symbol ID for direct lookup (recommended to avoid ambiguity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AnalyzeImpactRequest {
    /// Name of the symbol to analyze impact for (use symbol_id for unambiguous lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    /// Symbol ID for direct lookup (recommended to avoid ambiguity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<u32>,
    /// Maximum depth to search (default: 3)
    #[serde(default = "default_depth")]
    pub max_depth: u32,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchSymbolsRequest {
    /// Search query (supports fuzzy matching)
    pub query: String,
    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by symbol kind (e.g., "Function", "Struct", "Trait")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Filter by module path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SemanticSearchRequest {
    /// Natural language search query
    pub query: String,
    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Minimum similarity score (0-1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SemanticSearchWithContextRequest {
    /// Natural language search query
    pub query: String,
    /// Maximum number of results (default: 5, as each includes full context)
    #[serde(default = "default_context_limit")]
    pub limit: u32,
    /// Minimum similarity score (0-1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct GetIndexInfoRequest {}

fn default_depth() -> u32 {
    3
}

fn default_limit() -> u32 {
    10
}

fn default_context_limit() -> u32 {
    5
}

#[derive(Clone)]
pub struct CodeIntelligenceServer {
    pub indexer: Arc<RwLock<SimpleIndexer>>,
    tool_router: ToolRouter<Self>,
    peer: Arc<Mutex<Option<Peer<RoleServer>>>>,
}

#[tool_router]
impl CodeIntelligenceServer {
    pub fn new(indexer: SimpleIndexer) -> Self {
        Self {
            indexer: Arc::new(RwLock::new(indexer)),
            tool_router: Self::tool_router(),
            peer: Arc::new(Mutex::new(None)),
        }
    }

    /// Create server from an already-loaded indexer (most efficient)
    pub fn from_indexer(indexer: Arc<RwLock<SimpleIndexer>>) -> Self {
        Self {
            indexer,
            tool_router: Self::tool_router(),
            peer: Arc::new(Mutex::new(None)),
        }
    }

    /// Create server with existing indexer and settings (for HTTP server)
    pub fn new_with_indexer(indexer: Arc<RwLock<SimpleIndexer>>, _settings: Arc<Settings>) -> Self {
        // For now, settings is unused but might be needed for future enhancements
        Self {
            indexer,
            tool_router: Self::tool_router(),
            peer: Arc::new(Mutex::new(None)),
        }
    }

    /// Get a reference to the indexer Arc for external management (e.g., hot-reload)
    pub fn get_indexer_arc(&self) -> Arc<RwLock<SimpleIndexer>> {
        self.indexer.clone()
    }

    /// Send a notification when a file is re-indexed
    pub async fn notify_file_reindexed(&self, file_path: &str) {
        let peer_guard = self.peer.lock().await;
        if let Some(peer) = peer_guard.as_ref() {
            // Send a resource updated notification
            let _ = peer
                .notify_resource_updated(ResourceUpdatedNotificationParam {
                    uri: format!("file://{file_path}"),
                })
                .await;

            // Also send a logging message for visibility
            let _ = peer
                .notify_logging_message(LoggingMessageNotificationParam {
                    level: LoggingLevel::Info,
                    logger: Some("codanna".to_string()),
                    data: serde_json::json!({
                        "action": "re-indexed",
                        "file": file_path
                    }),
                })
                .await;
        }
    }

    #[tool(description = "Find a symbol by name in the indexed codebase")]
    pub async fn find_symbol(
        &self,
        Parameters(FindSymbolRequest { name, lang }): Parameters<FindSymbolRequest>,
    ) -> Result<CallToolResult, McpError> {
        use crate::symbol::context::ContextIncludes;

        let indexer = self.indexer.read().await;
        let symbols = indexer.find_symbols_by_name(&name, lang.as_deref());

        if symbols.is_empty() {
            let mut output = format!("No symbols found with name: {name}");
            // Add guidance for no results
            if let Some(guidance) = generate_mcp_guidance(indexer.settings(), "find_symbol", 0) {
                output.push_str("\n\n---\n💡 ");
                output.push_str(&guidance);
                output.push('\n');
            }
            return Ok(CallToolResult::success(vec![Content::text(output)]));
        }

        let mut result = format!("Found {} symbol(s) named '{}':\n\n", symbols.len(), name);

        for (idx, symbol) in symbols.iter().enumerate() {
            if idx > 0 {
                result.push_str("\n---\n\n");
            }

            // Try to get full context
            if let Some(ctx) = indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::IMPLEMENTATIONS
                    | ContextIncludes::DEFINITIONS
                    | ContextIncludes::CALLERS,
            ) {
                // Use formatted output from context
                result.push_str(&ctx.format_location_with_type());
                result.push('\n');

                // Add module path if available
                if let Some(module) = symbol.as_module_path() {
                    result.push_str(&format!("Module: {module}\n"));
                }

                // Add signature if available
                if let Some(sig) = symbol.as_signature() {
                    result.push_str(&format!("Signature: {sig}\n"));
                }

                // Add documentation preview
                if let Some(doc) = symbol.as_doc_comment() {
                    let doc_preview: Vec<&str> = doc.lines().take(3).collect();
                    let preview = if doc.lines().count() > 3 {
                        format!("{}...", doc_preview.join(" "))
                    } else {
                        doc_preview.join(" ")
                    };
                    result.push_str(&format!("Documentation: {preview}\n"));
                }

                // Add relationship summary
                let mut has_relationships = false;

                if let Some(impls) = &ctx.relationships.implemented_by {
                    if !impls.is_empty() {
                        result.push_str(&format!("Implemented by: {} type(s)\n", impls.len()));
                        has_relationships = true;
                    }
                }

                if let Some(defines) = &ctx.relationships.defines {
                    if !defines.is_empty() {
                        let methods = defines
                            .iter()
                            .filter(|s| s.kind == crate::SymbolKind::Method)
                            .count();
                        if methods > 0 {
                            result.push_str(&format!("Defines: {methods} method(s)\n"));
                            has_relationships = true;
                        }
                    }
                }

                if let Some(callers) = &ctx.relationships.called_by {
                    if !callers.is_empty() {
                        result.push_str(&format!("Called by: {} function(s)\n", callers.len()));
                        has_relationships = true;
                    }
                }

                if !has_relationships && symbol.kind == crate::SymbolKind::Function {
                    result.push_str("No direct callers found\n");
                }
            } else {
                // Fallback to basic info
                result.push_str(&format!(
                    "{:?} at {}:{}\n",
                    symbol.kind,
                    symbol.file_path,
                    symbol.range.start_line + 1
                ));

                if let Some(ref doc) = symbol.doc_comment {
                    let doc_preview: Vec<&str> = doc.lines().take(3).collect();
                    let preview = if doc.lines().count() > 3 {
                        format!("{}...", doc_preview.join(" "))
                    } else {
                        doc_preview.join(" ")
                    };
                    result.push_str(&format!("Documentation: {preview}\n"));
                }

                if let Some(ref sig) = symbol.signature {
                    result.push_str(&format!("Signature: {sig}\n"));
                }
            }
        }

        // Add system guidance
        if let Some(guidance) =
            generate_mcp_guidance(indexer.settings(), "find_symbol", symbols.len())
        {
            result.push_str("\n---\n💡 ");
            result.push_str(&guidance);
            result.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Get functions that a given function CALLS (invokes with parentheses).\n\nShows: function_name() → what it calls\nDoes NOT show: Type usage, component rendering, or who calls this function.\n\nUse analyze_impact for: Type dependencies, component usage (JSX), or reverse lookups."
    )]
    pub async fn get_calls(
        &self,
        Parameters(GetCallsRequest {
            function_name,
            symbol_id,
        }): Parameters<GetCallsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        // Get the symbol either by ID or by name
        let (symbol, identifier) = if let Some(id) = symbol_id {
            // Direct lookup by symbol ID
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => (sym, format!("symbol_id:{id}")),
                None => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: symbol_id:{id}"
                    ))]));
                }
            }
        } else if let Some(name) = function_name {
            // Lookup by name
            let symbols = indexer.find_symbols_by_name(&name, None);

            if symbols.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Function not found: {name}"
                ))]));
            }

            if symbols.len() > 1 {
                // Multiple symbols found - return error with list
                let mut msg = format!(
                    "Ambiguous: found {} symbol(s) named '{}':\n",
                    symbols.len(),
                    name
                );
                for (i, sym) in symbols.iter().take(10).enumerate() {
                    msg.push_str(&format!(
                        "  {}. symbol_id:{} - {:?} at {}:{}\n",
                        i + 1,
                        sym.id.value(),
                        sym.kind,
                        sym.file_path,
                        sym.range.start_line + 1
                    ));
                }
                if symbols.len() > 10 {
                    msg.push_str(&format!("  ... and {} more\n", symbols.len() - 10));
                }
                msg.push_str("\nUse: get_calls symbol_id:<id> for specific symbol");
                return Ok(CallToolResult::success(vec![Content::text(msg)]));
            }

            // Single match - use it
            (symbols.into_iter().next().unwrap(), name)
        } else {
            return Ok(CallToolResult::success(vec![Content::text(
                "Error: Either function_name or symbol_id must be provided".to_string(),
            )]));
        };

        // Get calls for this specific symbol
        let all_called_with_metadata = indexer.get_called_functions_with_metadata(symbol.id);

        if all_called_with_metadata.is_empty() {
            let mut output = format!("{identifier} doesn't call any functions");
            // Add guidance for no results
            if let Some(guidance) = generate_mcp_guidance(indexer.settings(), "get_calls", 0) {
                output.push_str("\n\n---\n💡 ");
                output.push_str(&guidance);
                output.push('\n');
            }
            return Ok(CallToolResult::success(vec![Content::text(output)]));
        }

        let result_count = all_called_with_metadata.len();
        let mut result = format!("{identifier} calls {result_count} function(s):\n");
        for (callee, metadata) in all_called_with_metadata {
            // Parse metadata to extract receiver info and call site location
            let (call_display, call_line) = if let Some(ref meta) = metadata {
                let display = if let Some(context) = &meta.context {
                    if context.contains("receiver:") && context.contains("static:") {
                        // Parse "receiver:{receiver},static:{is_static}"
                        let parts: Vec<&str> = context.split(',').collect();
                        let mut receiver = "";
                        let mut is_static = false;

                        for part in parts {
                            if let Some(r) = part.strip_prefix("receiver:") {
                                receiver = r;
                            } else if let Some(s) = part.strip_prefix("static:") {
                                is_static = s == "true";
                            }
                        }

                        if !receiver.is_empty() {
                            if is_static {
                                format!("{}::{}", receiver, callee.name)
                            } else {
                                format!("{}.{}", receiver, callee.name)
                            }
                        } else {
                            callee.name.to_string()
                        }
                    } else {
                        callee.name.to_string()
                    }
                } else {
                    callee.name.to_string()
                };

                // Use call site line if available, otherwise definition line
                let line = meta
                    .line
                    .map(|l| l + 1)
                    .unwrap_or(callee.range.start_line + 1);
                (display, line)
            } else {
                (callee.name.to_string(), callee.range.start_line + 1)
            };

            result.push_str(&format!(
                "  -> {:?} {} at {}:{}\n",
                callee.kind, call_display, callee.file_path, call_line
            ));
            if let Some(ref sig) = callee.signature {
                result.push_str(&format!("     Signature: {sig}\n"));
            }
        }

        // Add system guidance
        if let Some(guidance) = generate_mcp_guidance(indexer.settings(), "get_calls", result_count)
        {
            result.push_str("\n---\n💡 ");
            result.push_str(&guidance);
            result.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Find functions that CALL a given function (invoke it with parentheses).\n\nShows: what calls → function_name()\nDoes NOT show: Type references, component rendering, or what this function calls.\n\nUse analyze_impact for: Complete dependency graph including type usage and composition."
    )]
    pub async fn find_callers(
        &self,
        Parameters(FindCallersRequest {
            function_name,
            symbol_id,
        }): Parameters<FindCallersRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        // Get the symbol either by ID or by name
        let (symbol, identifier) = if let Some(id) = symbol_id {
            // Direct lookup by symbol ID - UNAMBIGUOUS
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => (sym, format!("symbol_id:{id}")),
                None => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: symbol_id:{id}"
                    ))]));
                }
            }
        } else if let Some(name) = function_name {
            let symbols = indexer.find_symbols_by_name(&name, None);

            if symbols.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Function not found: {name}"
                ))]));
            }

            if symbols.len() > 1 {
                // MULTIPLE MATCHES - Return error with list of symbol IDs
                let mut msg = format!(
                    "Ambiguous: found {} symbol(s) named '{}':\n",
                    symbols.len(),
                    name
                );
                for (i, sym) in symbols.iter().take(10).enumerate() {
                    msg.push_str(&format!(
                        "  {}. symbol_id:{} - {:?} at {}:{}\n",
                        i + 1,
                        sym.id.value(),
                        sym.kind,
                        sym.file_path,
                        sym.range.start_line + 1
                    ));
                }
                if symbols.len() > 10 {
                    msg.push_str(&format!("  ... and {} more\n", symbols.len() - 10));
                }
                msg.push_str("\nUse: find_callers symbol_id:<id> for specific symbol");
                return Ok(CallToolResult::success(vec![Content::text(msg)]));
            }

            // SINGLE MATCH - use it
            (symbols.into_iter().next().unwrap(), name)
        } else {
            return Ok(CallToolResult::success(vec![Content::text(
                "Error: Either function_name or symbol_id must be provided".to_string(),
            )]));
        };

        // Get callers for THIS SPECIFIC symbol only (no aggregation)
        let all_callers_with_metadata = indexer.get_calling_functions_with_metadata(symbol.id);

        if all_callers_with_metadata.is_empty() {
            let mut output = format!("No functions call {identifier}");
            // Add guidance for no results
            if let Some(guidance) = generate_mcp_guidance(indexer.settings(), "find_callers", 0) {
                output.push_str("\n\n---\n💡 ");
                output.push_str(&guidance);
                output.push('\n');
            }
            return Ok(CallToolResult::success(vec![Content::text(output)]));
        }

        // Build structured text response with rich metadata
        let result_count = all_callers_with_metadata.len();
        let mut result = format!("{result_count} function(s) call {identifier}:\n");

        for (caller, metadata) in all_callers_with_metadata {
            // Parse metadata to extract receiver info and call site location
            let (call_info, call_line) = if let Some(ref meta) = metadata {
                let info = if let Some(context) = &meta.context {
                    if context.contains("receiver:") && context.contains("static:") {
                        // Parse "receiver:{receiver},static:{is_static}"
                        let parts: Vec<&str> = context.split(',').collect();
                        let mut receiver = "";
                        let mut is_static = false;

                        for part in parts {
                            if let Some(r) = part.strip_prefix("receiver:") {
                                receiver = r;
                            } else if let Some(s) = part.strip_prefix("static:") {
                                is_static = s == "true";
                            }
                        }

                        if !receiver.is_empty() {
                            let qualified_name = if is_static {
                                format!("{receiver}::{}", symbol.name)
                            } else {
                                format!("{receiver}.{}", symbol.name)
                            };
                            format!(" (calls {qualified_name})")
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                // Use call site line if available, otherwise definition line
                let line = meta
                    .line
                    .map(|l| l + 1)
                    .unwrap_or(caller.range.start_line + 1);
                (info, line)
            } else {
                (String::new(), caller.range.start_line + 1)
            };

            result.push_str(&format!(
                "  <- {:?} {} at {}:{}{}\n",
                caller.kind, caller.name, caller.file_path, call_line, call_info
            ));

            if let Some(ref sig) = caller.signature {
                result.push_str(&format!("     Signature: {sig}\n"));
            }
        }

        // Add system guidance
        if let Some(guidance) =
            generate_mcp_guidance(indexer.settings(), "find_callers", result_count)
        {
            result.push_str("\n---\n💡 ");
            result.push_str(&guidance);
            result.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Analyze complete impact of changing a symbol. Shows ALL relationships: function calls, type usage, composition.\n\nShows:\n- What CALLS this function\n- What USES this as a type (fields, parameters, returns)\n- What RENDERS/COMPOSES this (JSX: <Component>, Rust: struct fields, etc.)\n- Full dependency graph across files\n\nUse this when: You need to see everything that depends on a symbol."
    )]
    pub async fn analyze_impact(
        &self,
        Parameters(AnalyzeImpactRequest {
            symbol_name,
            symbol_id,
            max_depth,
        }): Parameters<AnalyzeImpactRequest>,
    ) -> Result<CallToolResult, McpError> {
        use crate::symbol::context::ContextIncludes;

        let indexer = self.indexer.read().await;

        // Get the symbol either by ID or by name
        let (symbol, identifier) = if let Some(id) = symbol_id {
            // Direct lookup by symbol ID - UNAMBIGUOUS
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => (sym, format!("symbol_id:{id}")),
                None => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: symbol_id:{id}"
                    ))]));
                }
            }
        } else if let Some(name) = symbol_name {
            let symbols = indexer.find_symbols_by_name(&name, None);

            if symbols.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Symbol not found: {name}"
                ))]));
            }

            if symbols.len() > 1 {
                // MULTIPLE MATCHES - Return error with list of symbol IDs
                let mut msg = format!(
                    "Ambiguous: found {} symbol(s) named '{}':\n",
                    symbols.len(),
                    name
                );
                for (i, sym) in symbols.iter().take(10).enumerate() {
                    msg.push_str(&format!(
                        "  {}. symbol_id:{} - {:?} at {}:{}\n",
                        i + 1,
                        sym.id.value(),
                        sym.kind,
                        sym.file_path,
                        sym.range.start_line + 1
                    ));
                }
                if symbols.len() > 10 {
                    msg.push_str(&format!("  ... and {} more\n", symbols.len() - 10));
                }
                msg.push_str("\nUse: analyze_impact symbol_id:<id> for specific symbol");
                return Ok(CallToolResult::success(vec![Content::text(msg)]));
            }

            // SINGLE MATCH - use it
            (symbols.into_iter().next().unwrap(), name)
        } else {
            return Ok(CallToolResult::success(vec![Content::text(
                "Error: Either symbol_name or symbol_id must be provided".to_string(),
            )]));
        };

        // Analyze impact for THIS SPECIFIC symbol only (no aggregation)
        let impacted = indexer.get_impact_radius(symbol.id, Some(max_depth as usize));

        if impacted.is_empty() {
            let mut output = format!("No symbols would be impacted by changing {identifier}");
            // Add guidance for no results
            if let Some(guidance) = generate_mcp_guidance(indexer.settings(), "analyze_impact", 0) {
                output.push_str("\n\n---\n💡 ");
                output.push_str(&guidance);
                output.push('\n');
            }
            return Ok(CallToolResult::success(vec![Content::text(output)]));
        }

        let mut result = format!("Analyzing impact of changing: {identifier}\n");

        // Show the specific symbol being analyzed
        if let Some(ctx) = indexer.get_symbol_context(symbol.id, ContextIncludes::CALLERS) {
            let location = ctx.format_location();
            let direct_callers = ctx
                .relationships
                .called_by
                .as_ref()
                .map(|c| c.len())
                .unwrap_or(0);
            result.push_str(&format!(
                "Symbol: {:?} at {} (direct callers: {})\n\n",
                symbol.kind, location, direct_callers
            ));
        }

        let impact_count = impacted.len();
        result.push_str(&format!(
            "Total impact: {impact_count} symbol(s) would be affected (max depth: {max_depth})\n"
        ));

        // Group by symbol kind
        let mut by_kind: std::collections::HashMap<crate::SymbolKind, Vec<Symbol>> =
            std::collections::HashMap::new();

        for id in impacted {
            if let Some(sym) = indexer.get_symbol(id) {
                by_kind.entry(sym.kind).or_default().push(sym);
            }
        }

        // Display grouped by kind with locations
        for (kind, symbols) in by_kind {
            result.push_str(&format!("\n{kind:?} ({}): \n", symbols.len()));
            for sym in symbols {
                result.push_str(&format!(
                    "  - {} at {}:{}\n",
                    sym.name,
                    sym.file_path,
                    sym.range.start_line + 1
                ));
            }
        }

        // Add system guidance
        if let Some(guidance) =
            generate_mcp_guidance(indexer.settings(), "analyze_impact", impact_count)
        {
            result.push_str("\n---\n💡 ");
            result.push_str(&guidance);
            result.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get information about the indexed codebase")]
    pub async fn get_index_info(
        &self,
        Parameters(_params): Parameters<GetIndexInfoRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        let symbol_count = indexer.symbol_count();
        let file_count = indexer.file_count();
        let relationship_count = indexer.relationship_count();

        // Efficiently count symbols by kind in one pass
        let mut kind_counts = std::collections::HashMap::new();
        for symbol in indexer.get_all_symbols() {
            *kind_counts.entry(symbol.kind).or_insert(0) += 1;
        }

        // Build symbol kinds display dynamically
        let mut kinds_display = String::new();

        // Sort by kind name for consistent output
        let mut sorted_kinds: Vec<_> = kind_counts.iter().collect();
        sorted_kinds.sort_by_key(|(kind, _)| format!("{kind:?}"));

        for (kind, count) in sorted_kinds {
            kinds_display.push_str(&format!("\n  - {kind:?}s: {count}"));
        }

        // Get semantic search info
        let semantic_info = if let Some(metadata) = indexer.get_semantic_metadata() {
            format!(
                "\n\nSemantic Search:\n  - Status: Enabled\n  - Model: {}\n  - Embeddings: {}\n  - Dimensions: {}\n  - Created: {}\n  - Updated: {}",
                metadata.model_name,
                metadata.embedding_count,
                metadata.dimension,
                format_relative_time(metadata.created_at),
                format_relative_time(metadata.updated_at)
            )
        } else {
            "\n\nSemantic Search:\n  - Status: Disabled".to_string()
        };

        let result = format!(
            "Index contains {symbol_count} symbols across {file_count} files.\n\nBreakdown:\n  - Symbols: {symbol_count}\n  - Relationships: {relationship_count}\n\nSymbol Kinds:{kinds_display}{semantic_info}"
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Search documentation using natural language semantic search")]
    pub async fn semantic_search_docs(
        &self,
        Parameters(SemanticSearchRequest {
            query,
            limit,
            threshold,
            lang,
        }): Parameters<SemanticSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        // Use MCP debug flag for cleaner output
        if indexer.settings().mcp.debug {
            eprintln!("MCP DEBUG: semantic_search_docs called");
            eprintln!(
                "MCP DEBUG: Indexer symbol count: {}",
                indexer.symbol_count()
            );
            eprintln!("MCP DEBUG: Has semantic: {}", indexer.has_semantic_search());
        }

        if !indexer.has_semantic_search() {
            // Check if semantic files exist
            let semantic_path = indexer.settings().index_path.join("semantic");
            let metadata_exists = semantic_path.join("metadata.json").exists();
            let vectors_exist = semantic_path.join("segment_0.vec").exists();
            let symbol_count = indexer.symbol_count();

            // Get current working directory for debugging
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string());

            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Semantic search is not enabled. The index needs to be rebuilt with semantic search enabled.\n\nDEBUG INFO:\n- Index path: {}\n- Symbol count: {}\n- Semantic files exist: {}\n- Has semantic search: {}\n- Working dir: {}",
                indexer.settings().index_path.display(),
                symbol_count,
                metadata_exists && vectors_exist,
                indexer.has_semantic_search(),
                cwd
            ))]));
        }

        let results = match threshold {
            Some(t) => indexer.semantic_search_docs_with_threshold_and_language(
                &query,
                limit as usize,
                t,
                lang.as_deref(),
            ),
            None => {
                indexer.semantic_search_docs_with_language(&query, limit as usize, lang.as_deref())
            }
        };

        match results {
            Ok(results) => {
                if results.is_empty() {
                    let mut output =
                        format!("No semantically similar documentation found for: {query}");
                    // Add guidance for no results
                    if let Some(guidance) =
                        generate_mcp_guidance(indexer.settings(), "semantic_search_docs", 0)
                    {
                        output.push_str("\n\n---\n💡 ");
                        output.push_str(&guidance);
                        output.push('\n');
                    }
                    return Ok(CallToolResult::success(vec![Content::text(output)]));
                }

                let mut result = format!(
                    "Found {} semantically similar result(s) for '{}':\n\n",
                    results.len(),
                    query
                );

                for (i, (symbol, score)) in results.iter().enumerate() {
                    result.push_str(&format!(
                        "{}. {} ({:?}) - Similarity: {:.3}\n",
                        i + 1,
                        symbol.name,
                        symbol.kind,
                        score
                    ));
                    result.push_str(&format!(
                        "   File: {}:{}\n",
                        symbol.file_path,
                        symbol.range.start_line + 1
                    ));

                    if let Some(ref doc) = symbol.doc_comment {
                        // Show first 3 lines of doc
                        let preview: Vec<&str> = doc.lines().take(3).collect();
                        let doc_preview = if doc.lines().count() > 3 {
                            format!("{}...", preview.join(" "))
                        } else {
                            preview.join(" ")
                        };
                        result.push_str(&format!("   Doc: {doc_preview}\n"));
                    }

                    if let Some(ref sig) = symbol.signature {
                        result.push_str(&format!("   Signature: {sig}\n"));
                    }

                    result.push('\n');
                }

                // Add system guidance
                if let Some(guidance) =
                    generate_mcp_guidance(indexer.settings(), "semantic_search_docs", results.len())
                {
                    result.push_str("\n---\n💡 ");
                    result.push_str(&guidance);
                    result.push('\n');
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Semantic search failed: {e}"
            ))])),
        }
    }

    #[tool(
        description = "Search by natural language and get full context: documentation, dependencies, callers, impact.\n\nReturns symbols with:\n- Their documentation\n- What calls them\n- What they call\n- Complete impact graph (includes ALL relationships: calls, type usage, composition)\n\nUse this when: You want to find and understand symbols with their complete usage context."
    )]
    pub async fn semantic_search_with_context(
        &self,
        Parameters(SemanticSearchWithContextRequest {
            query,
            limit,
            threshold,
            lang,
        }): Parameters<SemanticSearchWithContextRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        if !indexer.has_semantic_search() {
            if indexer.settings().mcp.debug {
                eprintln!("DEBUG: Semantic search check failed in semantic_search_with_context");
                eprintln!(
                    "DEBUG: Indexer settings index_path: {}",
                    indexer.settings().index_path.display()
                );
                eprintln!(
                    "DEBUG: Indexer has_semantic_search: {}",
                    indexer.has_semantic_search()
                );
            }
            // Check if semantic files exist
            let semantic_path = indexer.settings().index_path.join("semantic");
            let metadata_exists = semantic_path.join("metadata.json").exists();
            let vectors_exist = semantic_path.join("segment_0.vec").exists();

            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Semantic search is not enabled. The index needs to be rebuilt with semantic search enabled.\n\nDEBUG INFO:\n- Index path: {}\n- Has semantic search: {}\n- Semantic path: {}\n- Metadata exists: {}\n- Vectors exist: {}",
                indexer.settings().index_path.display(),
                indexer.has_semantic_search(),
                semantic_path.display(),
                metadata_exists,
                vectors_exist
            ))]));
        }

        // First, perform semantic search
        let search_results = match threshold {
            Some(t) => indexer.semantic_search_docs_with_threshold_and_language(
                &query,
                limit as usize,
                t,
                lang.as_deref(),
            ),
            None => {
                indexer.semantic_search_docs_with_language(&query, limit as usize, lang.as_deref())
            }
        };

        match search_results {
            Ok(results) => {
                if results.is_empty() {
                    let mut output = format!("No documentation found matching query: {query}");
                    // Add guidance for no results
                    if let Some(guidance) =
                        generate_mcp_guidance(indexer.settings(), "semantic_search_with_context", 0)
                    {
                        output.push_str("\n\n---\n💡 ");
                        output.push_str(&guidance);
                        output.push('\n');
                    }
                    return Ok(CallToolResult::success(vec![Content::text(output)]));
                }

                let mut output = String::new();
                output.push_str(&format!(
                    "Found {} results for query: '{}'\n\n",
                    results.len(),
                    query
                ));

                // For each result, gather comprehensive context
                for (idx, (symbol, score)) in results.iter().enumerate() {
                    // Basic symbol information - matching find_symbol format
                    output.push_str(&format!(
                        "{}. {} - {:?} at {} [symbol_id:{}]\n",
                        idx + 1,
                        symbol.name,
                        symbol.kind,
                        crate::symbol::context::SymbolContext::symbol_location(symbol),
                        symbol.id.value()
                    ));
                    output.push_str(&format!("   Similarity Score: {score:.3}\n"));

                    // Documentation
                    if let Some(ref doc) = symbol.doc_comment {
                        output.push_str("   Documentation:\n");
                        for line in doc.lines().take(5) {
                            output.push_str(&format!("     {line}\n"));
                        }
                        if doc.lines().count() > 5 {
                            output.push_str("     ...\n");
                        }
                    }

                    // Only gather additional context for functions/methods
                    if matches!(
                        symbol.kind,
                        crate::SymbolKind::Function | crate::SymbolKind::Method
                    ) {
                        // Dependencies (what this function calls) - using logic from get_calls
                        let called_with_metadata =
                            indexer.get_called_functions_with_metadata(symbol.id);
                        if !called_with_metadata.is_empty() {
                            output.push_str(&format!(
                                "\n   {} calls {} function(s):\n",
                                symbol.name,
                                called_with_metadata.len()
                            ));
                            for (i, (called, metadata)) in
                                called_with_metadata.iter().take(10).enumerate()
                            {
                                // Parse receiver information from metadata and get call site location
                                let (call_display, call_line) = if let Some(meta) = metadata {
                                    let display = if let Some(context) = &meta.context {
                                        if context.contains("receiver:")
                                            && context.contains("static:")
                                        {
                                            let parts: Vec<&str> = context.split(',').collect();
                                            let mut receiver = None;
                                            let mut is_static = false;

                                            for part in parts {
                                                if let Some(recv) = part.strip_prefix("receiver:") {
                                                    receiver = Some(recv.trim());
                                                } else if let Some(static_val) =
                                                    part.strip_prefix("static:")
                                                {
                                                    is_static = static_val.trim() == "true";
                                                }
                                            }

                                            match (receiver, is_static) {
                                                (Some("self"), false) => {
                                                    format!("(self.{})", called.name)
                                                }
                                                (Some(recv), true) if recv != "self" => {
                                                    format!("({}::{})", recv, called.name)
                                                }
                                                (Some(recv), false) if recv != "self" => {
                                                    format!("({}.{})", recv, called.name)
                                                }
                                                _ => called.name.to_string(),
                                            }
                                        } else {
                                            called.name.to_string()
                                        }
                                    } else {
                                        called.name.to_string()
                                    };

                                    // Use call site line if available
                                    let line = meta
                                        .line
                                        .map(|l| l + 1)
                                        .unwrap_or(called.range.start_line + 1);
                                    (display, line)
                                } else {
                                    (called.name.to_string(), called.range.start_line + 1)
                                };

                                output.push_str(&format!(
                                    "     -> {:?} {} at {}:{} [symbol_id:{}]\n",
                                    called.kind,
                                    call_display,
                                    called.file_path,
                                    call_line,
                                    called.id.value()
                                ));
                                if i == 9 && called_with_metadata.len() > 10 {
                                    output.push_str(&format!(
                                        "     ... and {} more\n",
                                        called_with_metadata.len() - 10
                                    ));
                                }
                            }
                        }

                        // Callers (who uses this function) - using logic from find_callers
                        let calling_functions_with_metadata =
                            indexer.get_calling_functions_with_metadata(symbol.id);
                        if !calling_functions_with_metadata.is_empty() {
                            output.push_str(&format!(
                                "\n   {} function(s) call {}:\n",
                                calling_functions_with_metadata.len(),
                                symbol.name
                            ));
                            for (i, (caller, metadata)) in
                                calling_functions_with_metadata.iter().take(10).enumerate()
                            {
                                // Parse metadata to extract receiver info and call site location
                                let (call_info, call_line) = if let Some(meta) = metadata {
                                    let info = if let Some(context) = &meta.context {
                                        if context.contains("receiver:")
                                            && context.contains("static:")
                                        {
                                            // Parse "receiver:{receiver},static:{is_static}"
                                            let parts: Vec<&str> = context.split(',').collect();
                                            let mut receiver = "";
                                            let mut is_static = false;

                                            for part in parts {
                                                if let Some(r) = part.strip_prefix("receiver:") {
                                                    receiver = r;
                                                } else if let Some(s) = part.strip_prefix("static:")
                                                {
                                                    is_static = s == "true";
                                                }
                                            }

                                            if !receiver.is_empty() {
                                                let qualified_name = if is_static {
                                                    format!("{}::{}", receiver, symbol.name)
                                                } else {
                                                    format!("{}.{}", receiver, symbol.name)
                                                };
                                                format!(" (calls {qualified_name})")
                                            } else {
                                                String::new()
                                            }
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        String::new()
                                    };

                                    // Use call site line if available
                                    let line = meta
                                        .line
                                        .map(|l| l + 1)
                                        .unwrap_or(caller.range.start_line + 1);
                                    (info, line)
                                } else {
                                    (String::new(), caller.range.start_line + 1)
                                };

                                output.push_str(&format!(
                                    "     <- {:?} {} at {}:{}{} [symbol_id:{}]\n",
                                    caller.kind,
                                    caller.name,
                                    caller.file_path,
                                    call_line,
                                    call_info,
                                    caller.id.value()
                                ));
                                if i == 9 && calling_functions_with_metadata.len() > 10 {
                                    output.push_str(&format!(
                                        "     ... and {} more\n",
                                        calling_functions_with_metadata.len() - 10
                                    ));
                                }
                            }
                        }

                        // Impact analysis - using logic from analyze_impact
                        let impacted = indexer.get_impact_radius(symbol.id, Some(2));
                        if !impacted.is_empty() {
                            output.push_str(&format!(
                                "\n   Changing {} would impact {} symbol(s) (max depth: 2):\n",
                                symbol.name,
                                impacted.len()
                            ));

                            // Get details and group by kind
                            let impacted_details: Vec<_> = impacted
                                .iter()
                                .filter_map(|id| indexer.get_symbol(*id))
                                .collect();

                            // Group by kind
                            let mut methods = Vec::new();
                            let mut functions = Vec::new();
                            let mut other = Vec::new();

                            for sym in impacted_details {
                                match sym.kind {
                                    crate::SymbolKind::Method => methods.push(sym),
                                    crate::SymbolKind::Function => functions.push(sym),
                                    _ => other.push(sym),
                                }
                            }

                            if !methods.is_empty() {
                                output.push_str(&format!("\n     methods ({}):\n", methods.len()));
                                for method in methods.iter().take(5) {
                                    output.push_str(&format!(
                                        "       - {} [symbol_id:{}]\n",
                                        method.name,
                                        method.id.value()
                                    ));
                                }
                                if methods.len() > 5 {
                                    output.push_str(&format!(
                                        "       ... and {} more\n",
                                        methods.len() - 5
                                    ));
                                }
                            }

                            if !functions.is_empty() {
                                output.push_str(&format!(
                                    "\n     functions ({}):\n",
                                    functions.len()
                                ));
                                for func in functions.iter().take(5) {
                                    output.push_str(&format!(
                                        "       - {} [symbol_id:{}]\n",
                                        func.name,
                                        func.id.value()
                                    ));
                                }
                                if functions.len() > 5 {
                                    output.push_str(&format!(
                                        "       ... and {} more\n",
                                        functions.len() - 5
                                    ));
                                }
                            }

                            if !other.is_empty() {
                                output.push_str(&format!("\n     other ({}):\n", other.len()));
                                for sym in other.iter().take(3) {
                                    output.push_str(&format!(
                                        "       - {} ({:?}) [symbol_id:{}]\n",
                                        sym.name,
                                        sym.kind,
                                        sym.id.value()
                                    ));
                                }
                            }
                        }
                    }

                    output.push('\n');
                }

                // Add system guidance
                if let Some(guidance) = generate_mcp_guidance(
                    indexer.settings(),
                    "semantic_search_with_context",
                    results.len(),
                ) {
                    output.push_str("\n---\n💡 ");
                    output.push_str(&guidance);
                    output.push('\n');
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Semantic search failed: {e}"
            ))])),
        }
    }

    #[tool(description = "Search for symbols using full-text search with fuzzy matching")]
    pub async fn search_symbols(
        &self,
        Parameters(SearchSymbolsRequest {
            query,
            limit,
            kind,
            module,
            lang,
        }): Parameters<SearchSymbolsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        // Parse the kind filter if provided
        let kind_filter = kind.as_ref().and_then(|k| match k.to_lowercase().as_str() {
            "function" => Some(crate::SymbolKind::Function),
            "struct" => Some(crate::SymbolKind::Struct),
            "trait" => Some(crate::SymbolKind::Trait),
            "method" => Some(crate::SymbolKind::Method),
            "field" => Some(crate::SymbolKind::Field),
            "module" => Some(crate::SymbolKind::Module),
            "constant" => Some(crate::SymbolKind::Constant),
            _ => None,
        });

        match indexer.search(
            &query,
            limit as usize,
            kind_filter,
            module.as_deref(),
            lang.as_deref(),
        ) {
            Ok(results) => {
                if results.is_empty() {
                    let mut output = format!("No results found for query: {query}");
                    // Add guidance for no results
                    if let Some(guidance) =
                        generate_mcp_guidance(indexer.settings(), "search_symbols", 0)
                    {
                        output.push_str("\n\n---\n💡 ");
                        output.push_str(&guidance);
                        output.push('\n');
                    }
                    return Ok(CallToolResult::success(vec![Content::text(output)]));
                }

                let mut result = format!(
                    "Found {} result(s) for query '{}':\n\n",
                    results.len(),
                    query
                );

                for (i, search_result) in results.iter().enumerate() {
                    result.push_str(&format!(
                        "{}. {} ({:?})\n",
                        i + 1,
                        search_result.name,
                        search_result.kind
                    ));
                    result.push_str(&format!(
                        "   File: {}:{}\n",
                        search_result.file_path, search_result.line
                    ));

                    if !search_result.module_path.is_empty() {
                        result.push_str(&format!("   Module: {}\n", search_result.module_path));
                    }

                    if let Some(ref doc) = search_result.doc_comment {
                        // Show first line of doc comment
                        let first_line = doc.lines().next().unwrap_or("");
                        result.push_str(&format!("   Doc: {first_line}\n"));
                    }

                    if let Some(ref sig) = search_result.signature {
                        result.push_str(&format!("   Signature: {sig}\n"));
                    }

                    result.push_str(&format!("   Score: {:.2}\n", search_result.score));
                    result.push('\n');
                }

                // Add system guidance
                if let Some(guidance) =
                    generate_mcp_guidance(indexer.settings(), "search_symbols", results.len())
                {
                    result.push_str("\n---\n💡 ");
                    result.push_str(&guidance);
                    result.push('\n');
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {e}"
            ))])),
        }
    }
}

#[tool_handler]
impl ServerHandler for CodeIntelligenceServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "codanna".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Codanna Code Intelligence".to_string()),
                website_url: Some("https://github.com/bartolli/codanna".to_string()),
                icons: None,
            },
            instructions: Some(
                "This server provides code intelligence tools for analyzing this codebase. \
                WORKFLOW: Start with 'semantic_search_with_context' or 'semantic_search_docs' to anchor on the right files and APIs - they provide the highest-quality context. \
                Then use 'find_symbol' and 'search_symbols' to lock onto exact files and kinds. \
                Treat 'get_calls', 'find_callers', and 'analyze_impact' as hints; confirm with code reading or tighter queries (unique names, kind filters). \
                Use 'get_index_info' to understand what's indexed."
                .to_string()
            ),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        // Store the peer reference for sending notifications
        let mut peer_guard = self.peer.lock().await;
        *peer_guard = Some(context.peer.clone());

        // Return the server info
        Ok(self.get_info())
    }
}
