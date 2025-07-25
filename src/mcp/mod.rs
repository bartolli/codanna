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

use std::sync::Arc;
use std::future::Future;
use rmcp::{
    model::{ErrorData as McpError, *},
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    schemars,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{SimpleIndexer, IndexPersistence, Settings, Symbol};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FindSymbolRequest {
    /// Name of the symbol to find
    pub name: String,
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
    pub max_depth: usize,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchSymbolsRequest {
    /// Search query (supports fuzzy matching)
    pub query: String,
    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Filter by symbol kind (e.g., "Function", "Struct", "Trait")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Filter by module path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
}

fn default_depth() -> usize {
    3
}

fn default_limit() -> usize {
    10
}

#[derive(Clone)]
pub struct CodeIntelligenceServer {
    indexer: Arc<RwLock<SimpleIndexer>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl CodeIntelligenceServer {
    pub fn new(indexer: SimpleIndexer) -> Self {
        Self {
            indexer: Arc::new(RwLock::new(indexer)),
            tool_router: Self::tool_router(),
        }
    }
    
    /// Create server from an already-loaded indexer (most efficient)
    pub fn from_indexer(indexer: Arc<RwLock<SimpleIndexer>>) -> Self {
        Self {
            indexer,
            tool_router: Self::tool_router(),
        }
    }

    pub async fn from_persistence(settings: &Settings) -> Result<Self, Box<dyn std::error::Error>> {
        let persistence = IndexPersistence::new(settings.index_path.clone());
        
        let indexer = if persistence.exists() {
            eprintln!("Loading existing index from {}", settings.index_path.display());
            match persistence.load() {
                Ok(loaded) => {
                    eprintln!("Loaded index with {} symbols", loaded.symbol_count());
                    loaded
                }
                Err(e) => {
                    eprintln!("Warning: Could not load index: {}. Creating new index.", e);
                    SimpleIndexer::new()
                }
            }
        } else {
            eprintln!("No existing index found. Please run 'index' command first.");
            SimpleIndexer::new()
        };

        Ok(Self::new(indexer))
    }

    #[tool(description = "Find a symbol by name in the indexed codebase")]
    pub async fn find_symbol(
        &self,
        Parameters(FindSymbolRequest { name }): Parameters<FindSymbolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        let symbols = indexer.find_symbols_by_name(&name);
        
        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                format!("No symbols found with name: {}", name)
            )]));
        }

        let mut result = format!("Found {} symbol(s) named '{}':\n", symbols.len(), name);
        for symbol in symbols {
            let file_path = indexer.get_file_path(symbol.file_id)
                .unwrap_or_else(|| "<unknown>".to_string());
            result.push_str(&format!(
                "- {:?} at {}:{}\n", 
                symbol.kind, 
                file_path,
                symbol.range.start_line + 1
            ));
            
            // Add documentation if available
            if let Some(ref doc) = symbol.doc_comment {
                // Show first 3 lines of documentation
                let doc_preview: Vec<&str> = doc.lines().take(3).collect();
                let preview = if doc.lines().count() > 3 {
                    format!("{}...", doc_preview.join(" "))
                } else {
                    doc_preview.join(" ")
                };
                result.push_str(&format!("  Documentation: {}\n", preview));
            }
            
            // Add signature if available
            if let Some(ref sig) = symbol.signature {
                result.push_str(&format!("  Signature: {}\n", sig));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get all functions that a given function calls")]
    pub async fn get_calls(
        &self,
        Parameters(GetCallsRequest { function_name }): Parameters<GetCallsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        
        let symbols = indexer.find_symbols_by_name(&function_name);
        
        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                format!("Function not found: {}", function_name)
            )]));
        }
        
        let mut all_called = Vec::new();
        let mut checked_symbols = 0;
        
        // Check all symbols with this name
        for symbol in &symbols {
            checked_symbols += 1;
            let called = indexer.get_called_functions(symbol.id);
            for callee in called {
                if !all_called.iter().any(|c: &Symbol| c.id == callee.id) {
                    all_called.push(callee);
                }
            }
        }
        
        if all_called.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                format!("{} doesn't call any functions (checked {} symbol(s) with this name)", function_name, checked_symbols)
            )]));
        }
        
        let mut result = format!("{} calls {} function(s):\n", function_name, all_called.len());
        for callee in all_called {
            result.push_str(&format!("  -> {}\n", callee.name));
        }
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Find all functions that call a given function")]
    pub async fn find_callers(
        &self,
        Parameters(FindCallersRequest { function_name }): Parameters<FindCallersRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        
        let symbols = indexer.find_symbols_by_name(&function_name);
        
        if symbols.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                format!("Function not found: {}", function_name)
            )]));
        }
        
        let mut all_callers = Vec::new();
        let mut checked_symbols = 0;
        
        // Check all symbols with this name
        for symbol in &symbols {
            checked_symbols += 1;
            let callers = indexer.get_calling_functions(symbol.id);
            for caller in callers {
                if !all_callers.iter().any(|c: &Symbol| c.id == caller.id) {
                    all_callers.push(caller);
                }
            }
        }
        
        if all_callers.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                format!("No functions call {} (checked {} symbol(s) with this name)", function_name, checked_symbols)
            )]));
        }
        
        let mut result = format!("{} function(s) call {}:\n", all_callers.len(), function_name);
        for caller in all_callers {
            result.push_str(&format!("  <- {}\n", caller.name));
        }
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Analyze the impact radius of changing a symbol")]
    pub async fn analyze_impact(
        &self,
        Parameters(AnalyzeImpactRequest { symbol_name, max_depth }): Parameters<AnalyzeImpactRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        
        match indexer.find_symbol(&symbol_name) {
            Some(symbol_id) => {
                let impacted = indexer.get_impact_radius(symbol_id, Some(max_depth));
                
                if impacted.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        format!("No symbols would be impacted by changing {}", symbol_name)
                    )]));
                }

                let mut result = format!(
                    "Changing {} would impact {} symbol(s) (max depth: {}):\n",
                    symbol_name, impacted.len(), max_depth
                );
                
                // Group by symbol kind for better readability
                let mut by_kind: std::collections::HashMap<crate::SymbolKind, Vec<_>> = 
                    std::collections::HashMap::new();
                
                for id in impacted {
                    if let Some(sym) = indexer.get_symbol(id) {
                        by_kind.entry(sym.kind).or_default().push(sym);
                    }
                }
                
                // Display grouped by kind
                for (kind, symbols) in by_kind {
                    result.push_str(&format!("\n{}s:\n", format!("{:?}", kind).to_lowercase()));
                    for sym in symbols {
                        result.push_str(&format!("  - {}\n", sym.name));
                    }
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            None => {
                Ok(CallToolResult::success(vec![Content::text(
                    format!("Symbol not found: {}", symbol_name)
                )]))
            }
        }
    }

    #[tool(description = "Get information about the indexed codebase")]
    pub async fn get_index_info(&self) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        let symbol_count = indexer.symbol_count();
        
        let symbols = indexer.get_all_symbols();
        let functions = symbols.iter()
            .filter(|s| s.kind == crate::SymbolKind::Function)
            .count();
        let methods = symbols.iter()
            .filter(|s| s.kind == crate::SymbolKind::Method)
            .count();
        let structs = symbols.iter()
            .filter(|s| s.kind == crate::SymbolKind::Struct)
            .count();
        let traits = symbols.iter()
            .filter(|s| s.kind == crate::SymbolKind::Trait)
            .count();

        let result = format!(
            "Index contains {} symbols:\n  Functions: {}\n  Methods: {}\n  Structs: {}\n  Traits: {}",
            symbol_count, functions, methods, structs, traits
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Search for symbols using full-text search with fuzzy matching")]
    pub async fn search_symbols(
        &self,
        Parameters(SearchSymbolsRequest { query, limit, kind, module }): Parameters<SearchSymbolsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.indexer.read().await;
        
        // Parse the kind filter if provided
        let kind_filter = kind.as_ref().and_then(|k| {
            match k.to_lowercase().as_str() {
                "function" => Some(crate::SymbolKind::Function),
                "struct" => Some(crate::SymbolKind::Struct),
                "trait" => Some(crate::SymbolKind::Trait),
                "method" => Some(crate::SymbolKind::Method),
                "field" => Some(crate::SymbolKind::Field),
                "module" => Some(crate::SymbolKind::Module),
                "constant" => Some(crate::SymbolKind::Constant),
                _ => None
            }
        });
        
        match indexer.search(&query, limit, kind_filter, module.as_deref()) {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        format!("No results found for query: {}", query)
                    )]));
                }
                
                let mut result = format!("Found {} result(s) for query '{}':\n\n", results.len(), query);
                
                for (i, search_result) in results.iter().enumerate() {
                    result.push_str(&format!("{}. {} ({})\n", i + 1, search_result.name, format!("{:?}", search_result.kind)));
                    result.push_str(&format!("   File: {}:{}\n", search_result.file_path, search_result.line));
                    
                    if !search_result.module_path.is_empty() {
                        result.push_str(&format!("   Module: {}\n", search_result.module_path));
                    }
                    
                    if let Some(ref doc) = search_result.doc_comment {
                        // Show first line of doc comment
                        let first_line = doc.lines().next().unwrap_or("");
                        result.push_str(&format!("   Doc: {}\n", first_line));
                    }
                    
                    if let Some(ref sig) = search_result.signature {
                        result.push_str(&format!("   Signature: {}\n", sig));
                    }
                    
                    result.push_str(&format!("   Score: {:.2}\n", search_result.score));
                    result.push('\n');
                }
                
                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => {
                Ok(CallToolResult::error(vec![Content::text(
                    format!("Search failed: {}", e)
                )]))
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
            },
            instructions: Some(
                "This server provides code intelligence tools for analyzing Rust codebases. \
                Use 'search_symbols' for full-text search with fuzzy matching, 'find_symbol' to locate specific symbols, \
                'get_calls' to see what a function calls, 'find_callers' to see what calls a function, \
                and 'analyze_impact' to understand the impact of changes. \
                Use 'get_index_info' to see what's in the index."
                .to_string()
            ),
        }
    }
}