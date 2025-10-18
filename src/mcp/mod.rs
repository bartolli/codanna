//! MCP (Model Context Protocol) server implementation for code intelligence
//!
//! This module provides MCP tools that allow AI assistants to query
//! the code intelligence index.
//!
//! ## Architecture
//!
//! The MCP server can run in two modes:
//!
//! 1. **Standalone Server Mode**: Run with `cargo run -- serve`
//!    - Loads index once into memory
//!    - Listens for client connections via stdio
//!    - Efficient for production use with AI assistants
//!
//! 2. **Embedded Mode**: Used by the CLI directly
//!    - No separate process needed
//!    - Direct access to already-loaded index
//!    - Most memory efficient for CLI operations

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

use crate::{IndexPersistence, Settings, SimpleIndexer, Symbol};

/// Simple glob pattern matching for file paths
/// Supports: *, **, ?, and exact matches
fn glob_match(pattern: &str, path: &str) -> bool {
    // Normalize path separators to forward slashes
    let mut path = path.replace('\\', "/");
    let pattern = pattern.replace('\\', "/");

    // Strip leading ./ from path for matching
    if path.starts_with("./") {
        path = path[2..].to_string();
    }

    // Handle ** (match any number of directories)
    if pattern.contains("**") {
        // For patterns like **/Processes/**, we need substring matching
        // Remove ** wildcards and check if remaining parts exist in path
        let pattern_parts: Vec<&str> = pattern.split("**").filter(|p| !p.is_empty()).collect();

        if pattern_parts.is_empty() {
            // Pattern is just "**" - match everything
            return true;
        }

        // Check each non-empty part exists in order in the path
        let mut search_from = 0;
        for part in pattern_parts {
            let trimmed = part.trim_matches('/');
            if trimmed.is_empty() {
                continue;
            }

            if let Some(pos) = path[search_from..].find(trimmed) {
                search_from += pos + trimmed.len();
            } else {
                return false;
            }
        }

        return true;
    }

    // Handle * (match within a directory)
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut pos = 0;

        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                // First part must match at start
                if !path[pos..].starts_with(part) {
                    return false;
                }
                pos += part.len();
            } else if i == parts.len() - 1 {
                // Last part must match at end
                return path[pos..].ends_with(part);
            } else {
                // Middle parts must exist in order
                if let Some(found_pos) = path[pos..].find(part) {
                    pos += found_pos + part.len();
                } else {
                    return false;
                }
            }
        }
        return true;
    }

    // Exact match
    path == pattern
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
    /// Name of the function to analyze
    pub function_name: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FindCallersRequest {
    /// Name of the function to find callers for
    pub function_name: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AnalyzeImpactRequest {
    /// Name of the symbol to analyze impact for
    pub symbol_name: String,
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
    /// Filter by file path pattern (glob syntax, e.g., "**/Base/**", "*.cs")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_pattern: Option<String>,
    /// Exclude file path pattern (glob syntax, e.g., "**/Test/**", "**/node_modules/**")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_pattern: Option<String>,
    /// Skip first N results for pagination (default: 0)
    #[serde(default)]
    pub offset: u32,
    /// Return compact summary only: name, kind, location (default: false)
    #[serde(default)]
    pub summary_only: bool,
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
pub struct GetSymbolDetailsRequest {
    /// Name of the symbol to get details for
    pub symbol_name: String,
    /// Optional file path to disambiguate (e.g., "src/module.rs:42")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Optional module path to disambiguate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
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

    pub async fn from_persistence(
        settings: Arc<Settings>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let persistence = IndexPersistence::new(settings.index_path.clone());

        let indexer = if persistence.exists() {
            eprintln!(
                "Loading existing index from {}",
                settings.index_path.display()
            );
            match persistence.load_with_settings(settings.clone(), false) {
                Ok(loaded) => {
                    eprintln!("Loaded index with {} symbols", loaded.symbol_count());
                    loaded
                }
                Err(e) => {
                    eprintln!("Warning: Could not load index: {e}. Creating new index.");
                    SimpleIndexer::with_settings(settings.clone())
                }
            }
        } else {
            eprintln!("No existing index found. Please run 'index' command first.");
            SimpleIndexer::with_settings(settings.clone())
        };

        Ok(Self::new(indexer))
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
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No symbols found with name: {name}\n\n\
                 💡 Try:\n\
                   - Partial name search with search_symbols (e.g., query:{})\n\
                   - Check spelling or try a similar name\n\
                   - Use semantic_search_docs for natural language search",
                &name[..name.len().min(5)]
            ))]));
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
                let file_path = indexer
                    .get_file_path(symbol.file_id)
                    .unwrap_or_else(|| "<unknown>".to_string());
                result.push_str(&format!(
                    "{:?} at {}:{}\n",
                    symbol.kind,
                    file_path,
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

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Get functions that a given function CALLS (invokes with parentheses).\n\nShows: function_name() → what it calls\nDoes NOT show: Type usage, component rendering, or who calls this function.\n\nUse analyze_impact for: Type dependencies, component usage (JSX), or reverse lookups."
    )]
    pub async fn get_calls(
        &self,
        Parameters(GetCallsRequest { function_name }): Parameters<GetCallsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        let symbols = indexer.find_symbols_by_name(&function_name, None);

        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Function not found: {function_name}\n\n\
                 💡 Try:\n\
                   - Use find_symbol to verify the exact function name\n\
                   - Search with search_symbols (e.g., query:{} kind:function)\n\
                   - Check if it's a method instead of a function",
                &function_name[..function_name.len().min(5)]
            ))]));
        }

        let mut all_called_with_metadata = Vec::new();
        let mut checked_symbols = 0;

        // Check all symbols with this name
        for symbol in &symbols {
            checked_symbols += 1;
            let called = indexer.get_called_functions_with_metadata(symbol.id);
            for (callee, metadata) in called {
                if !all_called_with_metadata
                    .iter()
                    .any(|(c, _): &(Symbol, _)| c.id == callee.id)
                {
                    all_called_with_metadata.push((callee, metadata));
                }
            }
        }

        if all_called_with_metadata.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "{function_name} doesn't call any functions (checked {checked_symbols} symbol(s) with this name)"
            ))]));
        }

        let mut result = format!(
            "{} calls {} function(s):\n",
            function_name,
            all_called_with_metadata.len()
        );
        for (callee, metadata) in all_called_with_metadata {
            let file_path = indexer
                .get_file_path(callee.file_id)
                .unwrap_or_else(|| "<unknown>".to_string());
            // Parse metadata to extract receiver info
            let call_display = if let Some(meta) = metadata {
                if meta.contains("receiver:") && meta.contains("static:") {
                    // Parse "receiver:{receiver},static:{is_static}"
                    let parts: Vec<&str> = meta.split(',').collect();
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

            result.push_str(&format!(
                "  -> {:?} {} at {}:{}\n",
                callee.kind,
                call_display,
                file_path,
                callee.range.start_line + 1
            ));
            if let Some(ref sig) = callee.signature {
                result.push_str(&format!("     Signature: {sig}\n"));
            }
        }
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Find functions that CALL a given function (invoke it with parentheses).\n\nShows: what calls → function_name()\nDoes NOT show: Type references, component rendering, or what this function calls.\n\nUse analyze_impact for: Complete dependency graph including type usage and composition."
    )]
    pub async fn find_callers(
        &self,
        Parameters(FindCallersRequest { function_name }): Parameters<FindCallersRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        let symbols = indexer.find_symbols_by_name(&function_name, None);

        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Function not found: {function_name}\n\n\
                 💡 Try:\n\
                   - Use find_symbol to verify the exact function name\n\
                   - Search with search_symbols (e.g., query:{} kind:method)\n\
                   - Check if it's spelled differently or in a different namespace",
                &function_name[..function_name.len().min(5)]
            ))]));
        }

        let mut all_callers_with_metadata = Vec::new();
        let mut checked_symbols = 0;

        // Check all symbols with this name
        for symbol in &symbols {
            checked_symbols += 1;
            let callers = indexer.get_calling_functions_with_metadata(symbol.id);
            for (caller, metadata) in callers {
                if !all_callers_with_metadata
                    .iter()
                    .any(|(c, _): &(Symbol, _)| c.id == caller.id)
                {
                    all_callers_with_metadata.push((caller, metadata));
                }
            }
        }

        if all_callers_with_metadata.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No functions call {function_name} (checked {checked_symbols} symbol(s) with this name)"
            ))]));
        }

        // Build structured text response with rich metadata
        let mut result = format!(
            "{} function(s) call {}:\n",
            all_callers_with_metadata.len(),
            function_name
        );

        for (caller, metadata) in all_callers_with_metadata {
            let file_path = indexer
                .get_file_path(caller.file_id)
                .unwrap_or_else(|| "<unknown>".to_string());

            // Parse metadata to extract receiver info
            let call_info = if let Some(meta) = metadata {
                if meta.contains("receiver:") && meta.contains("static:") {
                    // Parse "receiver:{receiver},static:{is_static}"
                    let parts: Vec<&str> = meta.split(',').collect();
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
                            format!("{receiver}::{function_name}")
                        } else {
                            format!("{receiver}.{function_name}")
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

            result.push_str(&format!(
                "  <- {:?} {} at {}:{}{}\n",
                caller.kind,
                caller.name,
                file_path,
                caller.range.start_line + 1,
                call_info
            ));

            if let Some(ref sig) = caller.signature {
                result.push_str(&format!("     Signature: {sig}\n"));
            }
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
            max_depth,
        }): Parameters<AnalyzeImpactRequest>,
    ) -> Result<CallToolResult, McpError> {
        use crate::symbol::context::ContextIncludes;

        let indexer = self.indexer.read().await;

        // Find ALL symbols with this name (like find_callers does)
        let symbols = indexer.find_symbols_by_name(&symbol_name, None);

        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Symbol not found: {symbol_name}"
            ))]));
        }

        // Collect impact from all symbols with this name and their locations
        let mut all_impacted = std::collections::HashSet::new();
        let mut symbol_locations = Vec::new();

        for symbol in &symbols {
            // Get context for each symbol to show its location
            if let Some(ctx) = indexer.get_symbol_context(symbol.id, ContextIncludes::CALLERS) {
                let location = ctx.format_location();
                let direct_callers = ctx
                    .relationships
                    .called_by
                    .as_ref()
                    .map(|c| c.len())
                    .unwrap_or(0);
                symbol_locations.push((symbol.kind, location, direct_callers));
            }

            let impacted = indexer.get_impact_radius(symbol.id, Some(max_depth as usize));
            all_impacted.extend(impacted);
        }

        if all_impacted.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No symbols would be impacted by changing {symbol_name} (checked {} symbol(s) with this name)",
                symbols.len()
            ))]));
        }

        let mut result = format!("Analyzing impact of changing: {symbol_name}\n");
        result.push_str(&format!(
            "Found {} symbol(s) with this name:\n",
            symbols.len()
        ));

        // Show locations of all symbols being analyzed
        for (kind, location, direct_callers) in &symbol_locations {
            result.push_str(&format!(
                "  - {kind:?} at {location} (direct callers: {direct_callers})\n"
            ));
        }
        result.push('\n');

        result.push_str(&format!(
            "\nTotal impact: {} symbol(s) would be affected (max depth: {})\n",
            all_impacted.len(),
            max_depth
        ));

        // Group by symbol kind with locations
        let mut by_kind: std::collections::HashMap<crate::SymbolKind, Vec<(Symbol, String)>> =
            std::collections::HashMap::new();

        for id in all_impacted {
            if let Some(sym) = indexer.get_symbol(id) {
                let file_path = indexer
                    .get_file_path(sym.file_id)
                    .unwrap_or_else(|| "<unknown>".to_string());
                by_kind.entry(sym.kind).or_default().push((sym, file_path));
            }
        }

        // Display grouped by kind with locations
        for (kind, symbols) in by_kind {
            result.push_str(&format!("\n{kind:?} ({}): \n", symbols.len()));
            for (sym, file_path) in symbols {
                result.push_str(&format!(
                    "  - {} at {}:{}\n",
                    sym.name,
                    file_path,
                    sym.range.start_line + 1
                ));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Get detailed information about a specific symbol. Use after search_symbols with summary_only=true to get full details."
    )]
    pub async fn get_symbol_details(
        &self,
        Parameters(GetSymbolDetailsRequest {
            symbol_name,
            file_path,
            module,
        }): Parameters<GetSymbolDetailsRequest>,
    ) -> Result<CallToolResult, McpError> {
        use crate::symbol::context::ContextIncludes;

        let indexer = self.indexer.read().await;
        let symbols = indexer.find_symbols_by_name(&symbol_name, None);

        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Symbol not found: {symbol_name}\n\n\
                 💡 Try: Use search_symbols to find available symbols first"
            ))]));
        }

        // Filter by file_path if provided
        let filtered_symbols: Vec<_> = if let Some(ref path) = file_path {
            // Normalize path separators for comparison
            let normalized_filter = path.replace('\\', "/");
            symbols
                .into_iter()
                .filter(|s| {
                    let sym_path = indexer.get_file_path(s.file_id).unwrap_or_default();
                    let normalized_sym_path = sym_path.replace('\\', "/");
                    normalized_sym_path.contains(&normalized_filter)
                        || normalized_filter.contains(&normalized_sym_path)
                })
                .collect()
        } else if let Some(ref mod_path) = module {
            symbols
                .into_iter()
                .filter(|s| {
                    s.as_module_path()
                        .map(|m| m.contains(mod_path))
                        .unwrap_or(false)
                })
                .collect()
        } else {
            symbols
        };

        if filtered_symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No symbols found matching: {symbol_name} with filters\n\n\
                 💡 Try: Remove file_path or module filter, or check the values"
            ))]));
        }

        let mut result = format!(
            "Found {} symbol(s) named '{}':\n\n",
            filtered_symbols.len(),
            symbol_name
        );

        for (idx, symbol) in filtered_symbols.iter().enumerate() {
            if idx > 0 {
                result.push_str("\n---\n\n");
            }

            // Get full context with all relationships
            if let Some(ctx) = indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::IMPLEMENTATIONS
                    | ContextIncludes::DEFINITIONS
                    | ContextIncludes::CALLERS,
            ) {
                result.push_str(&ctx.format_location_with_type());
                result.push('\n');

                // Module path
                if let Some(module) = symbol.as_module_path() {
                    result.push_str(&format!("Module: {module}\n"));
                }

                // Full signature
                if let Some(sig) = symbol.as_signature() {
                    result.push_str(&format!("Signature:\n{sig}\n"));
                }

                // Full documentation
                if let Some(doc) = symbol.as_doc_comment() {
                    result.push_str(&format!("\nDocumentation:\n{doc}\n"));
                }

                // Relationships
                if let Some(impls) = &ctx.relationships.implemented_by {
                    if !impls.is_empty() {
                        result.push_str(&format!("\nImplemented by {} type(s):\n", impls.len()));
                        for impl_sym in impls.iter().take(10) {
                            result.push_str(&format!("  - {}\n", impl_sym.name));
                        }
                        if impls.len() > 10 {
                            result.push_str(&format!("  ... and {} more\n", impls.len() - 10));
                        }
                    }
                }

                if let Some(defines) = &ctx.relationships.defines {
                    if !defines.is_empty() {
                        let methods: Vec<_> = defines
                            .iter()
                            .filter(|s| s.kind == crate::SymbolKind::Method)
                            .collect();
                        if !methods.is_empty() {
                            result.push_str(&format!("\nDefines {} method(s):\n", methods.len()));
                            for method in methods.iter().take(10) {
                                result.push_str(&format!("  - {}\n", method.name));
                            }
                            if methods.len() > 10 {
                                result
                                    .push_str(&format!("  ... and {} more\n", methods.len() - 10));
                            }
                        }
                    }
                }

                if let Some(callers) = &ctx.relationships.called_by {
                    if !callers.is_empty() {
                        result.push_str(&format!("\nCalled by {} function(s):\n", callers.len()));
                        for (caller_sym, _) in callers.iter().take(10) {
                            let caller_file = indexer
                                .get_file_path(caller_sym.file_id)
                                .unwrap_or_else(|| "<unknown>".to_string());
                            result.push_str(&format!(
                                "  - {} at {}:{}\n",
                                caller_sym.name,
                                caller_file,
                                caller_sym.range.start_line + 1
                            ));
                        }
                        if callers.len() > 10 {
                            result.push_str(&format!("  ... and {} more\n", callers.len() - 10));
                        }
                    }
                }
            } else {
                // Fallback to basic info
                let file_path = indexer
                    .get_file_path(symbol.file_id)
                    .unwrap_or_else(|| "<unknown>".to_string());
                result.push_str(&format!(
                    "{:?} at {}:{}\n",
                    symbol.kind,
                    file_path,
                    symbol.range.start_line + 1
                ));

                if let Some(ref doc) = symbol.doc_comment {
                    result.push_str(&format!("Documentation:\n{doc}\n"));
                }

                if let Some(ref sig) = symbol.signature {
                    result.push_str(&format!("Signature:\n{sig}\n"));
                }
            }
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
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No semantically similar documentation found for: {query}"
                    ))]));
                }

                let mut result = format!(
                    "Found {} semantically similar result(s) for '{}':\n\n",
                    results.len(),
                    query
                );

                for (i, (symbol, score)) in results.iter().enumerate() {
                    let file_path = indexer
                        .get_file_path(symbol.file_id)
                        .unwrap_or_else(|| "<unknown>".to_string());

                    result.push_str(&format!(
                        "{}. {} ({:?}) - Similarity: {:.3}\n",
                        i + 1,
                        symbol.name,
                        symbol.kind,
                        score
                    ));
                    result.push_str(&format!(
                        "   File: {}:{}\n",
                        file_path,
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
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No documentation found matching query: {query}"
                    ))]));
                }

                let mut output = String::new();
                output.push_str(&format!(
                    "Found {} results for query: '{}'\n\n",
                    results.len(),
                    query
                ));

                // For each result, gather comprehensive context
                for (idx, (symbol, score)) in results.iter().enumerate() {
                    let file_path = indexer
                        .get_file_path(symbol.file_id)
                        .unwrap_or_else(|| "<unknown>".to_string());

                    // Basic symbol information - matching find_symbol format
                    output.push_str(&format!(
                        "{}. {} - {:?} at {}:{}\n",
                        idx + 1,
                        symbol.name,
                        symbol.kind,
                        file_path,
                        symbol.range.start_line + 1
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
                                let called_file = indexer
                                    .get_file_path(called.file_id)
                                    .unwrap_or_else(|| "<unknown>".to_string());

                                // Parse receiver information from metadata
                                let call_display = if let Some(meta) = metadata {
                                    // Parse metadata context for receiver info
                                    if meta.contains("receiver:") && meta.contains("static:") {
                                        let parts: Vec<&str> = meta.split(',').collect();
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

                                output.push_str(&format!(
                                    "     -> {:?} {} at {}:{}\n",
                                    called.kind,
                                    call_display,
                                    called_file,
                                    called.range.start_line + 1
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
                                let caller_file = indexer
                                    .get_file_path(caller.file_id)
                                    .unwrap_or_else(|| "<unknown>".to_string());

                                // Parse metadata to extract receiver info
                                let call_info = if let Some(meta) = metadata {
                                    if meta.contains("receiver:") && meta.contains("static:") {
                                        // Parse "receiver:{receiver},static:{is_static}"
                                        let parts: Vec<&str> = meta.split(',').collect();
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

                                output.push_str(&format!(
                                    "     <- {:?} {} at {}:{}{}\n",
                                    caller.kind,
                                    caller.name,
                                    caller_file,
                                    caller.range.start_line + 1,
                                    call_info
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
                                    output.push_str(&format!("       - {}\n", method.name));
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
                                    output.push_str(&format!("       - {}\n", func.name));
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
                                        "       - {} ({:?})\n",
                                        sym.name, sym.kind
                                    ));
                                }
                            }
                        }
                    }

                    output.push('\n');
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Semantic search failed: {e}"
            ))])),
        }
    }

    #[tool(
        description = "Search for symbols using full-text search with fuzzy matching. Supports pagination via offset parameter."
    )]
    pub async fn search_symbols(
        &self,
        Parameters(SearchSymbolsRequest {
            query,
            limit,
            kind,
            module,
            lang,
            file_pattern,
            exclude_pattern,
            offset,
            summary_only,
        }): Parameters<SearchSymbolsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;

        // Parse the kind filter if provided
        let kind_filter = kind.as_ref().and_then(|k| match k.to_lowercase().as_str() {
            "function" => Some(crate::SymbolKind::Function),
            "method" => Some(crate::SymbolKind::Method),
            "struct" => Some(crate::SymbolKind::Struct),
            "class" => Some(crate::SymbolKind::Class),
            "enum" => Some(crate::SymbolKind::Enum),
            "trait" => Some(crate::SymbolKind::Trait),
            "interface" => Some(crate::SymbolKind::Interface),
            "module" => Some(crate::SymbolKind::Module),
            "field" => Some(crate::SymbolKind::Field),
            "variable" => Some(crate::SymbolKind::Variable),
            "constant" => Some(crate::SymbolKind::Constant),
            "parameter" => Some(crate::SymbolKind::Parameter),
            "typealias" | "type_alias" => Some(crate::SymbolKind::TypeAlias),
            "macro" => Some(crate::SymbolKind::Macro),
            _ => None,
        });

        // For pagination, we need to fetch more results than requested
        // so we can skip the offset and still return the requested limit
        let fetch_limit = (limit + offset) as usize;

        match indexer.search(
            &query,
            fetch_limit,
            kind_filter,
            module.as_deref(),
            lang.as_deref(),
        ) {
            Ok(mut results) => {
                // Apply file pattern filtering if specified
                if file_pattern.is_some() || exclude_pattern.is_some() {
                    results.retain(|r| {
                        let file_path = &r.file_path;

                        // Check inclusion pattern
                        if let Some(ref pattern) = file_pattern {
                            if !glob_match(pattern, file_path) {
                                return false;
                            }
                        }

                        // Check exclusion pattern
                        if let Some(ref pattern) = exclude_pattern {
                            if glob_match(pattern, file_path) {
                                return false;
                            }
                        }

                        true
                    });
                }

                let total_count = results.len();

                if total_count == 0 {
                    let suggestion = if query.len() > 5 {
                        format!(
                            "Try a shorter/partial query (e.g., query:{})",
                            &query[..query.len().min(5)]
                        )
                    } else {
                        "Try a different query or use semantic_search_docs for natural language"
                            .to_string()
                    };

                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No results found for query: {query}\n\n\
                         💡 Try:\n\
                           - {suggestion}\n\
                           - Remove kind/module filters if using them\n\
                           - Check spelling or try synonyms"
                    ))]));
                }

                // Apply pagination offset
                if offset as usize >= total_count {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Offset {offset} exceeds total results ({total_count}). Try a lower offset."
                    ))]));
                }

                // Skip offset and take limit
                results = results
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect();
                let page_count = results.len();

                // If summary_only mode, return compact output
                if summary_only {
                    let mut summary = if offset > 0 {
                        format!(
                            "Found {total_count} result(s) for query '{query}' (showing {page_count} from offset {offset}):\n"
                        )
                    } else {
                        format!("Found {total_count} result(s) for query '{query}':\n")
                    };

                    for search_result in &results {
                        summary.push_str(&format!(
                            "{} ({:?}) at {}:{}\n",
                            search_result.name,
                            search_result.kind,
                            search_result.file_path,
                            search_result.line
                        ));
                    }

                    // Add pagination hints if there are more results
                    let remaining =
                        total_count.saturating_sub((offset + page_count as u32) as usize);
                    if remaining > 0 {
                        summary.push_str(&format!(
                            "\n💡 {} more result(s) available. Use offset={} to see next page.",
                            remaining,
                            offset + page_count as u32
                        ));
                    }

                    return Ok(CallToolResult::success(vec![Content::text(summary)]));
                }

                // Build the full result string first with pagination info
                let mut result = if offset > 0 {
                    format!(
                        "Found {total_count} result(s) for query '{query}' (showing {page_count} result(s) from offset {offset}):\n\n"
                    )
                } else {
                    format!("Found {total_count} result(s) for query '{query}':\n\n")
                };

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

                // Add pagination hints if there are more results
                let remaining = total_count.saturating_sub((offset + page_count as u32) as usize);
                if remaining > 0 {
                    result.push_str(&format!(
                        "\n💡 {} more result(s) available. Use offset={} to see next page.",
                        remaining,
                        offset + page_count as u32
                    ));
                }

                // Token limit check - estimate ~4 chars per token
                const MAX_TOKENS: usize = 20000; // Leave 5K buffer for 25K Claude limit
                let estimated_tokens = result.len() / 4;

                if estimated_tokens > MAX_TOKENS {
                    // Auto-truncate to summary mode
                    let summary_count = 20.min(results.len());
                    let summary = results
                        .iter()
                        .take(summary_count)
                        .map(|r| format!("{} ({:?}) at {}:{}", r.name, r.kind, r.file_path, r.line))
                        .collect::<Vec<_>>()
                        .join("\n");

                    let mut truncated = format!(
                        "Found {total_count} symbols (showing first {summary_count} from offset {offset} due to size):\n{summary}"
                    );

                    if remaining > 0 {
                        truncated.push_str(&format!(
                            "\n\n💡 {} more result(s) available. Use offset={} to see next page.",
                            remaining,
                            offset + summary_count as u32
                        ));
                    }

                    truncated.push_str(
                        "\n💡 Tip: Use smaller limit or kind/module filters for detailed results",
                    );

                    return Ok(CallToolResult::success(vec![Content::text(truncated)]));
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => {
                let error_msg = format!("Search failed: {e}");
                let suggestion = if error_msg.contains("token") || error_msg.contains("size") {
                    "\n\n💡 Try: Use limit=10 or summary_only=true to reduce response size"
                } else if error_msg.contains("index") || error_msg.contains("not found") {
                    "\n\n💡 Try: Verify the index exists and is up to date with get_index_info"
                } else {
                    "\n\n💡 Try: Check query syntax or reduce complexity"
                };

                Ok(CallToolResult::error(vec![Content::text(format!(
                    "{error_msg}{suggestion}"
                ))]))
            }
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
