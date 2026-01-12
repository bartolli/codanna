//! MCP direct tool invocation command.

use crate::Symbol;
use crate::config::Settings;
use crate::indexing::facade::IndexFacade;
use crate::io::args::parse_positional_args;
use serde::Serialize;

// MCP tool JSON output structures
#[derive(Debug, Serialize)]
struct IndexInfo {
    symbol_count: usize,
    file_count: usize,
    relationship_count: usize,
    symbol_kinds: SymbolKindBreakdown,
    semantic_search: SemanticSearchInfo,
}

#[derive(Debug, Serialize)]
struct SymbolKindBreakdown {
    functions: usize,
    methods: usize,
    structs: usize,
    traits: usize,
}

#[derive(Debug, Serialize)]
struct SemanticSearchInfo {
    enabled: bool,
    model_name: Option<String>,
    embeddings: Option<usize>,
    dimensions: Option<usize>,
    created: Option<String>,
    updated: Option<String>,
}

/// Run the MCP direct tool invocation command.
pub async fn run(
    tool: String,
    positional: Vec<String>,
    args: Option<String>,
    json: bool,
    facade: IndexFacade,
    config: &Settings,
) {
    // Build arguments from both positional and --args
    let mut arguments = if let Some(args_str) = &args {
        // Parse JSON arguments if provided (backward compatibility)
        match serde_json::from_str::<serde_json::Value>(args_str) {
            Ok(serde_json::Value::Object(map)) => Some(map),
            Ok(_) => {
                eprintln!("Error: Arguments must be a JSON object");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error parsing arguments: {e}");
                std::process::exit(1);
            }
        }
    } else {
        // Start with empty map if no --args
        Some(serde_json::Map::new())
    };

    // Process positional arguments using unified parser
    if !positional.is_empty() {
        if let Some(ref mut args_map) = arguments {
            // Use the unified parser from args.rs
            let (first_positional, params) = parse_positional_args(&positional);

            // Handle the first positional argument based on tool type
            if let Some(pos_arg) = first_positional {
                match tool.as_str() {
                    "find_symbol" => {
                        args_map.insert(
                            "name".to_string(),
                            serde_json::Value::String(pos_arg.clone()),
                        );
                    }
                    "get_calls" | "find_callers" => {
                        args_map.insert(
                            "function_name".to_string(),
                            serde_json::Value::String(pos_arg.clone()),
                        );
                    }
                    "analyze_impact" => {
                        args_map.insert(
                            "symbol_name".to_string(),
                            serde_json::Value::String(pos_arg.clone()),
                        );
                    }
                    "semantic_search_docs"
                    | "semantic_search_with_context"
                    | "search_documents" => {
                        args_map.insert(
                            "query".to_string(),
                            serde_json::Value::String(pos_arg.clone()),
                        );
                    }
                    "search_symbols" => {
                        args_map.insert(
                            "query".to_string(),
                            serde_json::Value::String(pos_arg.clone()),
                        );
                    }
                    _ => {
                        eprintln!("Warning: Unknown tool '{tool}', ignoring positional argument");
                    }
                }
            }

            // Special handling: find_symbol supports symbol_id:XXX as positional
            // If symbol_id is in params but name wasn't set, use it as the name
            if tool == "find_symbol" && !args_map.contains_key("name") {
                if let Some(id) = params.get("symbol_id") {
                    args_map.insert(
                        "name".to_string(),
                        serde_json::Value::String(format!("symbol_id:{id}")),
                    );
                }
            }

            // Add all key:value pairs from params
            for (key, value) in params {
                // Try to parse as number first, then boolean, fallback to string
                let json_value = if let Ok(n) = value.parse::<i64>() {
                    serde_json::Value::Number(n.into())
                } else if let Ok(f) = value.parse::<f64>() {
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(f)
                            .unwrap_or_else(|| serde_json::Number::from(0)),
                    )
                } else if let Ok(b) = value.parse::<bool>() {
                    serde_json::Value::Bool(b)
                } else {
                    serde_json::Value::String(value)
                };
                args_map.insert(key, json_value);
            }
        }
    }

    // Convert to Option<Map> only if we have arguments
    let arguments = arguments.filter(|map| !map.is_empty());

    // Collect data for find_symbol if JSON output is requested
    let find_symbol_data = if json && tool == "find_symbol" {
        let name = arguments
            .as_ref()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str());
        let language = arguments
            .as_ref()
            .and_then(|m| m.get("lang"))
            .and_then(|v| v.as_str());

        if let Some(symbol_name) = name {
            let symbols = facade.find_symbols_by_name(symbol_name, language);
            if !symbols.is_empty() {
                use crate::symbol::context::ContextIncludes;
                let mut results = Vec::new();

                for symbol in symbols {
                    // Get full context with callers using the same approach as MCP
                    let context = facade.get_symbol_context(
                        symbol.id,
                        ContextIncludes::CALLERS
                            | ContextIncludes::IMPLEMENTATIONS
                            | ContextIncludes::DEFINITIONS,
                    );

                    // Build result with context if available
                    if let Some(ctx) = context {
                        results.push(ctx);
                    } else {
                        // Fallback: create minimal context
                        let file_path = facade
                            .get_file_path(symbol.file_id)
                            .unwrap_or_else(|| "unknown".to_string());

                        results.push(crate::symbol::context::SymbolContext {
                            symbol,
                            file_path,
                            relationships: Default::default(),
                        });
                    }
                }
                Some(results)
            } else {
                Some(Vec::new())
            }
        } else {
            None
        }
    } else {
        None
    };

    // Collect data for get_calls if JSON output is requested
    let get_calls_data = if json && tool == "get_calls" {
        let symbol_id = arguments
            .as_ref()
            .and_then(|m| m.get("symbol_id"))
            .and_then(|v| v.as_u64())
            .map(|id| id as u32);
        let function_name = arguments
            .as_ref()
            .and_then(|m| m.get("function_name"))
            .and_then(|v| v.as_str());
        let language = arguments
            .as_ref()
            .and_then(|m| m.get("lang"))
            .and_then(|v| v.as_str());

        if let Some(id) = symbol_id {
            use crate::symbol::context::ContextIncludes;

            // Direct lookup by symbol ID
            if let Some(symbol) = facade.get_symbol(crate::SymbolId(id)) {
                let mut all_calls = Vec::new();

                let context = facade.get_symbol_context(symbol.id, ContextIncludes::CALLS);
                if let Some(ctx) = context {
                    if let Some(calls) = ctx.relationships.calls {
                        for (called, metadata) in calls {
                            all_calls.push((called, metadata));
                        }
                    }
                }

                Some(all_calls)
            } else {
                None // Symbol not found
            }
        } else if let Some(func_name) = function_name {
            use crate::symbol::context::ContextIncludes;
            use std::collections::HashSet;

            // Find ALL symbols with this name
            let symbols = facade.find_symbols_by_name(func_name, language);
            let function_symbols: Vec<_> = symbols
                .into_iter()
                .filter(|s| {
                    matches!(
                        s.kind,
                        crate::SymbolKind::Function | crate::SymbolKind::Method
                    )
                })
                .collect();

            if function_symbols.is_empty() {
                None // Function not found
            } else {
                // Aggregate calls from ALL symbols with this name (same as MCP handler)
                let mut all_calls = Vec::new();
                let mut seen_ids = HashSet::new();

                for symbol in function_symbols {
                    let context = facade.get_symbol_context(symbol.id, ContextIncludes::CALLS);
                    if let Some(ctx) = context {
                        if let Some(calls) = ctx.relationships.calls {
                            for (called, metadata) in calls {
                                // Deduplicate by symbol ID
                                if seen_ids.insert(called.id) {
                                    all_calls.push((called, metadata));
                                }
                            }
                        }
                    }
                }

                Some(all_calls)
            }
        } else {
            None
        }
    } else {
        None
    };

    // Collect data for find_callers if JSON output is requested
    let find_callers_data = if json && tool == "find_callers" {
        let symbol_id = arguments
            .as_ref()
            .and_then(|m| m.get("symbol_id"))
            .and_then(|v| v.as_u64())
            .map(|id| id as u32);
        let function_name = arguments
            .as_ref()
            .and_then(|m| m.get("function_name"))
            .and_then(|v| v.as_str());
        let language = arguments
            .as_ref()
            .and_then(|m| m.get("lang"))
            .and_then(|v| v.as_str());

        if let Some(id) = symbol_id {
            // Direct lookup by symbol ID
            if let Some(symbol) = facade.get_symbol(crate::SymbolId(id)) {
                let callers = facade.get_calling_functions_with_metadata(symbol.id);
                let all_callers: Vec<_> = callers.into_iter().collect();
                Some(all_callers)
            } else {
                None // Symbol not found
            }
        } else if let Some(func_name) = function_name {
            use std::collections::HashSet;

            // Find all functions with this name
            let symbols = facade.find_symbols_by_name(func_name, language);
            if !symbols.is_empty() {
                let mut all_callers = Vec::new();
                let mut seen_ids = HashSet::new();

                // Check all symbols with this name and deduplicate (same as MCP handler)
                for symbol in &symbols {
                    let callers = facade.get_calling_functions_with_metadata(symbol.id);
                    for (caller, metadata) in callers {
                        // Deduplicate by symbol ID
                        if seen_ids.insert(caller.id) {
                            all_callers.push((caller, metadata));
                        }
                    }
                }

                Some(all_callers)
            } else {
                None // Function not found
            }
        } else {
            None
        }
    } else {
        None
    };

    // Collect data for analyze_impact if JSON output is requested
    let analyze_impact_data = if json && tool == "analyze_impact" {
        let symbol_id = arguments
            .as_ref()
            .and_then(|m| m.get("symbol_id"))
            .and_then(|v| v.as_u64())
            .map(|id| id as u32);
        let symbol_name = arguments
            .as_ref()
            .and_then(|m| m.get("symbol_name"))
            .and_then(|v| v.as_str());
        let language = arguments
            .as_ref()
            .and_then(|m| m.get("lang"))
            .and_then(|v| v.as_str());

        if let Some(id) = symbol_id {
            // Direct lookup by symbol ID
            if let Some(symbol) = facade.get_symbol(crate::SymbolId(id)) {
                let max_depth = arguments
                    .as_ref()
                    .and_then(|m| m.get("max_depth"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as usize;

                let impacted_ids = facade.get_impact_radius(symbol.id, Some(max_depth));

                // Convert SymbolIds to full Symbols
                let mut impacted_symbols = Vec::new();
                for impact_id in impacted_ids {
                    if let Some(sym) = facade.get_symbol(impact_id) {
                        impacted_symbols.push(sym);
                    }
                }

                Some(impacted_symbols)
            } else {
                None // Symbol not found
            }
        } else if let Some(sym_name) = symbol_name {
            use std::collections::HashSet;

            // Find ALL symbols with this name (same as MCP handler)
            let symbols = facade.find_symbols_by_name(sym_name, language);

            if symbols.is_empty() {
                None // Symbol not found
            } else {
                let max_depth = arguments
                    .as_ref()
                    .and_then(|m| m.get("max_depth"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as usize;

                // Aggregate impact from ALL symbols with this name (same as MCP handler)
                let mut all_impacted_ids = HashSet::new();
                for symbol in &symbols {
                    let impacted_ids = facade.get_impact_radius(symbol.id, Some(max_depth));
                    all_impacted_ids.extend(impacted_ids);
                }

                // Convert SymbolIds to full Symbols
                let mut impacted_symbols = Vec::new();
                for id in all_impacted_ids {
                    if let Some(sym) = facade.get_symbol(id) {
                        impacted_symbols.push(sym);
                    }
                }

                Some(impacted_symbols)
            }
        } else {
            None
        }
    } else {
        None
    };

    // Collect data for search_symbols if JSON output is requested
    let search_symbols_data = if json && tool == "search_symbols" {
        let query = arguments
            .as_ref()
            .and_then(|m| m.get("query"))
            .and_then(|v| v.as_str());

        if let Some(q) = query {
            let limit = arguments
                .as_ref()
                .and_then(|m| m.get("limit"))
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as u32;
            let kind = arguments
                .as_ref()
                .and_then(|m| m.get("kind"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let module = arguments
                .as_ref()
                .and_then(|m| m.get("module"))
                .and_then(|v| v.as_str());
            let language = arguments
                .as_ref()
                .and_then(|m| m.get("lang"))
                .and_then(|v| v.as_str());

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

            match facade.search(q, limit as usize, kind_filter, module, language) {
                Ok(results) => Some(results),
                Err(_) => Some(Vec::new()),
            }
        } else {
            None
        }
    } else {
        None
    };

    // Collect data for semantic_search_docs if JSON output is requested
    #[derive(serde::Serialize)]
    struct SemanticSearchResult {
        symbol: Symbol,
        score: f32,
    }

    #[derive(serde::Serialize)]
    struct SemanticSearchWithContextResult {
        symbol: Symbol,
        score: f32,
        context: crate::symbol::context::SymbolContext,
    }

    // Get guidance config before moving indexer
    let guidance_config = facade.settings().guidance.clone();

    let semantic_search_docs_data = if json && tool == "semantic_search_docs" {
        if !facade.has_semantic_search() {
            None // Semantic search not enabled
        } else {
            let query = arguments
                .as_ref()
                .and_then(|m| m.get("query"))
                .and_then(|v| v.as_str());

            if let Some(q) = query {
                let limit = arguments
                    .as_ref()
                    .and_then(|m| m.get("limit"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;
                let threshold = arguments
                    .as_ref()
                    .and_then(|m| m.get("threshold"))
                    .and_then(|v| v.as_f64())
                    .map(|t| t as f32);
                let language = arguments
                    .as_ref()
                    .and_then(|m| m.get("lang"))
                    .and_then(|v| v.as_str());

                let results = match threshold {
                    Some(t) => facade
                        .semantic_search_docs_with_threshold_and_language(q, limit, t, language),
                    None => facade.semantic_search_docs_with_language(q, limit, language),
                };

                match results {
                    Ok(results) => {
                        let semantic_results: Vec<SemanticSearchResult> = results
                            .into_iter()
                            .map(|(symbol, score)| SemanticSearchResult { symbol, score })
                            .collect();
                        Some(semantic_results)
                    }
                    Err(_) => Some(Vec::new()),
                }
            } else {
                None
            }
        }
    } else {
        None
    };

    // Collect data for semantic_search_with_context if JSON output is requested
    let semantic_search_with_context_data = if json && tool == "semantic_search_with_context" {
        if !facade.has_semantic_search() {
            None // Semantic search not enabled
        } else {
            let query = arguments
                .as_ref()
                .and_then(|m| m.get("query"))
                .and_then(|v| v.as_str());

            if let Some(q) = query {
                let limit = arguments
                    .as_ref()
                    .and_then(|m| m.get("limit"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as u32; // Default 5 for context version
                let threshold = arguments
                    .as_ref()
                    .and_then(|m| m.get("threshold"))
                    .and_then(|v| v.as_f64())
                    .map(|t| t as f32);
                let language = arguments
                    .as_ref()
                    .and_then(|m| m.get("lang"))
                    .and_then(|v| v.as_str());

                let search_results = match threshold {
                    Some(t) => facade.semantic_search_docs_with_threshold_and_language(
                        q,
                        limit as usize,
                        t,
                        language,
                    ),
                    None => facade.semantic_search_docs_with_language(q, limit as usize, language),
                };

                match search_results {
                    Ok(results) => {
                        use crate::symbol::context::ContextIncludes;
                        let context_results: Vec<SemanticSearchWithContextResult> = results
                            .into_iter()
                            .filter_map(|(symbol, score)| {
                                // Get full context for each symbol
                                let context = facade.get_symbol_context(
                                    symbol.id,
                                    ContextIncludes::CALLERS
                                        | ContextIncludes::CALLS
                                        | ContextIncludes::IMPLEMENTATIONS
                                        | ContextIncludes::DEFINITIONS,
                                );

                                context.map(|ctx| SemanticSearchWithContextResult {
                                    symbol,
                                    score,
                                    context: ctx,
                                })
                            })
                            .collect();
                        Some(context_results)
                    }
                    Err(_) => Some(Vec::new()),
                }
            } else {
                None
            }
        }
    } else {
        None
    };

    // Check semantic search status before moving indexer
    let has_semantic_search = facade.has_semantic_search();

    // If we need JSON output for get_index_info, collect data before moving indexer
    let index_info_data = if json && tool == "get_index_info" {
        let symbol_count = facade.symbol_count();
        let file_count = facade.file_count();
        let relationship_count = facade.relationship_count();

        // Count symbols by kind
        let mut kind_counts = std::collections::HashMap::new();
        for symbol in facade.get_all_symbols() {
            *kind_counts.entry(symbol.kind).or_insert(0) += 1;
        }

        let functions = *kind_counts.get(&crate::SymbolKind::Function).unwrap_or(&0);
        let methods = *kind_counts.get(&crate::SymbolKind::Method).unwrap_or(&0);
        let structs = *kind_counts.get(&crate::SymbolKind::Struct).unwrap_or(&0);
        let traits = *kind_counts.get(&crate::SymbolKind::Trait).unwrap_or(&0);

        // Get semantic search info
        let semantic_search = if let Some(metadata) = facade.get_semantic_metadata() {
            SemanticSearchInfo {
                enabled: true,
                model_name: Some(metadata.model_name),
                embeddings: Some(metadata.embedding_count),
                dimensions: Some(metadata.dimension),
                created: Some(crate::mcp::format_relative_time(metadata.created_at)),
                updated: Some(crate::mcp::format_relative_time(metadata.updated_at)),
            }
        } else {
            SemanticSearchInfo {
                enabled: false,
                model_name: None,
                embeddings: None,
                dimensions: None,
                created: None,
                updated: None,
            }
        };

        Some(IndexInfo {
            symbol_count,
            file_count: file_count as usize,
            relationship_count,
            symbol_kinds: SymbolKindBreakdown {
                functions,
                methods,
                structs,
                traits,
            },
            semantic_search,
        })
    } else {
        None
    };

    // Embedded mode - use already loaded facade directly
    // Try to load DocumentStore for search_documents tool
    let server = {
        let server = crate::mcp::CodeIntelligenceServer::new(facade);

        // Add DocumentStore if documents are enabled and indexed
        if let Some(store_arc) = crate::documents::load_from_settings(config) {
            server.with_document_store_arc(store_arc)
        } else {
            server
        }
    };

    // Call the tool directly
    use crate::mcp::*;
    use rmcp::handler::server::wrapper::Parameters;

    let result = match tool.as_str() {
        "find_symbol" => {
            let name = arguments
                .as_ref()
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    eprintln!("Error: find_symbol requires 'name' parameter");
                    std::process::exit(1);
                });
            let lang = arguments
                .as_ref()
                .and_then(|m| m.get("lang"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            server
                .find_symbol(Parameters(FindSymbolRequest {
                    name: name.to_string(),
                    lang,
                }))
                .await
        }
        "get_calls" => {
            let function_name = arguments
                .as_ref()
                .and_then(|m| m.get("function_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let symbol_id = arguments
                .as_ref()
                .and_then(|m| m.get("symbol_id"))
                .and_then(|v| v.as_u64())
                .map(|id| id as u32);

            // Require either function_name or symbol_id
            if function_name.is_none() && symbol_id.is_none() {
                eprintln!(
                    "Error: get_calls requires either 'function_name' or 'symbol_id' parameter"
                );
                std::process::exit(1);
            }

            server
                .get_calls(Parameters(GetCallsRequest {
                    function_name,
                    symbol_id,
                }))
                .await
        }
        "find_callers" => {
            let function_name = arguments
                .as_ref()
                .and_then(|m| m.get("function_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let symbol_id = arguments
                .as_ref()
                .and_then(|m| m.get("symbol_id"))
                .and_then(|v| v.as_u64())
                .map(|id| id as u32);

            // Require either function_name or symbol_id
            if function_name.is_none() && symbol_id.is_none() {
                eprintln!(
                    "Error: find_callers requires either 'function_name' or 'symbol_id' parameter"
                );
                std::process::exit(1);
            }

            server
                .find_callers(Parameters(FindCallersRequest {
                    function_name,
                    symbol_id,
                }))
                .await
        }
        "analyze_impact" => {
            let symbol_name = arguments
                .as_ref()
                .and_then(|m| m.get("symbol_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let symbol_id = arguments
                .as_ref()
                .and_then(|m| m.get("symbol_id"))
                .and_then(|v| v.as_u64())
                .map(|id| id as u32);

            // Require either symbol_name or symbol_id
            if symbol_name.is_none() && symbol_id.is_none() {
                eprintln!(
                    "Error: analyze_impact requires either 'symbol_name' or 'symbol_id' parameter"
                );
                std::process::exit(1);
            }

            let max_depth = arguments
                .as_ref()
                .and_then(|m| m.get("max_depth"))
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32;
            server
                .analyze_impact(Parameters(AnalyzeImpactRequest {
                    symbol_name,
                    symbol_id,
                    max_depth,
                }))
                .await
        }
        "get_index_info" => {
            use crate::mcp::GetIndexInfoRequest;
            use rmcp::handler::server::wrapper::Parameters;
            server
                .get_index_info(Parameters(GetIndexInfoRequest {}))
                .await
        }
        "search_symbols" => {
            let query = arguments
                .as_ref()
                .and_then(|m| m.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    eprintln!("Error: search_symbols requires 'query' parameter");
                    std::process::exit(1);
                });
            let limit = arguments
                .as_ref()
                .and_then(|m| m.get("limit"))
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as u32;
            let kind = arguments
                .as_ref()
                .and_then(|m| m.get("kind"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let module = arguments
                .as_ref()
                .and_then(|m| m.get("module"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let lang = arguments
                .as_ref()
                .and_then(|m| m.get("lang"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            server
                .search_symbols(Parameters(SearchSymbolsRequest {
                    query: query.to_string(),
                    limit,
                    kind,
                    module,
                    lang,
                }))
                .await
        }
        "semantic_search_docs" => {
            let query = arguments
                .as_ref()
                .and_then(|m| m.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    eprintln!("Error: semantic_search_docs requires 'query' parameter");
                    std::process::exit(1);
                });
            let limit = arguments
                .as_ref()
                .and_then(|m| m.get("limit"))
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as u32;
            let threshold = arguments
                .as_ref()
                .and_then(|m| m.get("threshold"))
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);
            let lang = arguments
                .as_ref()
                .and_then(|m| m.get("lang"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            server
                .semantic_search_docs(Parameters(SemanticSearchRequest {
                    query: query.to_string(),
                    limit,
                    threshold,
                    lang,
                }))
                .await
        }
        "semantic_search_with_context" => {
            let query = arguments
                .as_ref()
                .and_then(|m| m.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    eprintln!("Error: semantic_search_with_context requires 'query' parameter");
                    std::process::exit(1);
                });
            let limit = arguments
                .as_ref()
                .and_then(|m| m.get("limit"))
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as u32;
            let threshold = arguments
                .as_ref()
                .and_then(|m| m.get("threshold"))
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);
            let lang = arguments
                .as_ref()
                .and_then(|m| m.get("lang"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            server
                .semantic_search_with_context(Parameters(SemanticSearchWithContextRequest {
                    query: query.to_string(),
                    limit,
                    threshold,
                    lang,
                }))
                .await
        }
        "search_documents" => {
            use crate::mcp::SearchDocumentsRequest;
            let query = arguments
                .as_ref()
                .and_then(|m| m.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    eprintln!("Error: search_documents requires 'query' parameter");
                    std::process::exit(1);
                })
                .to_string();
            let collection = arguments
                .as_ref()
                .and_then(|m| m.get("collection"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let limit = arguments
                .as_ref()
                .and_then(|m| m.get("limit"))
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as u32;
            server
                .search_documents(Parameters(SearchDocumentsRequest {
                    query,
                    collection,
                    limit,
                }))
                .await
        }
        _ => {
            if json {
                use crate::io::exit_code::ExitCode;
                use crate::io::format::JsonResponse;
                let response = JsonResponse::error(
                    ExitCode::GeneralError,
                    &format!("Unknown tool: {tool}"),
                    vec![
                        "Available tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info, search_symbols, semantic_search_docs, semantic_search_with_context, search_documents",
                    ],
                );
                println!("{}", serde_json::to_string_pretty(&response).unwrap());
            } else {
                eprintln!("Unknown tool: {tool}");
                eprintln!(
                    "Available tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info, search_symbols, semantic_search_docs, semantic_search_with_context, search_documents"
                );
            }
            std::process::exit(1);
        }
    };

    // Print result
    match result {
        Ok(call_result) => {
            if json && tool == "get_index_info" {
                // Use pre-collected data for JSON output
                if let Some(index_info) = index_info_data {
                    use crate::io::format::JsonResponse;
                    use crate::io::guidance_engine::generate_guidance_from_config;
                    let mut response = JsonResponse::success(index_info);

                    // Add system guidance (using single_result template since this returns stats)
                    if let Some(guidance) =
                        generate_guidance_from_config(&guidance_config, "get_index_info", None, 1)
                    {
                        // Use 1 to trigger single_result template
                        response = response.with_system_message(&guidance);
                    }

                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                }
            } else if json && tool == "find_symbol" {
                // Use pre-collected data for JSON output
                if let Some(symbol_contexts) = find_symbol_data {
                    use crate::io::format::JsonResponse;
                    if symbol_contexts.is_empty() {
                        let name = arguments
                            .as_ref()
                            .and_then(|m| m.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let response = JsonResponse::not_found("Symbol", name);
                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                        std::process::exit(3);
                    } else {
                        use crate::io::guidance_engine::generate_guidance_from_config;
                        let mut response = JsonResponse::success(symbol_contexts);

                        // Add system guidance
                        let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "find_symbol",
                            arguments
                                .as_ref()
                                .and_then(|m| m.get("name"))
                                .and_then(|v| v.as_str()),
                            result_count,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    }
                }
            } else if json && tool == "get_calls" {
                // Use pre-collected data for JSON output
                if let Some(calls) = get_calls_data {
                    use crate::io::format::JsonResponse;
                    use crate::io::guidance_engine::generate_guidance_from_config;
                    let mut response = JsonResponse::success(calls);

                    // Add system guidance
                    let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                    if let Some(guidance) = generate_guidance_from_config(
                        &guidance_config,
                        "get_calls",
                        arguments
                            .as_ref()
                            .and_then(|m| m.get("function_name"))
                            .and_then(|v| v.as_str()),
                        result_count,
                    ) {
                        response = response.with_system_message(&guidance);
                    }

                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                } else {
                    // Function not found
                    use crate::io::format::JsonResponse;
                    let response = if let Some(id) = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_id"))
                        .and_then(|v| v.as_u64())
                    {
                        JsonResponse::not_found("Symbol", &format!("symbol_id:{id}"))
                    } else {
                        let name = arguments
                            .as_ref()
                            .and_then(|m| m.get("function_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        JsonResponse::not_found("Function", name)
                    };
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(3);
                }
            } else if json && tool == "find_callers" {
                // Use pre-collected data for JSON output
                if let Some(callers) = find_callers_data {
                    use crate::io::format::JsonResponse;
                    use crate::io::guidance_engine::generate_guidance_from_config;
                    let mut response = JsonResponse::success(callers);

                    // Add system guidance
                    let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                    if let Some(guidance) = generate_guidance_from_config(
                        &guidance_config,
                        "find_callers",
                        arguments
                            .as_ref()
                            .and_then(|m| m.get("function_name"))
                            .and_then(|v| v.as_str()),
                        result_count,
                    ) {
                        response = response.with_system_message(&guidance);
                    }

                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                } else {
                    // Function not found
                    use crate::io::format::JsonResponse;
                    let response = if let Some(id) = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_id"))
                        .and_then(|v| v.as_u64())
                    {
                        JsonResponse::not_found("Symbol", &format!("symbol_id:{id}"))
                    } else {
                        let name = arguments
                            .as_ref()
                            .and_then(|m| m.get("function_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        JsonResponse::not_found("Function", name)
                    };
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(3);
                }
            } else if json && tool == "analyze_impact" {
                // Use pre-collected data for JSON output
                if let Some(impacted) = analyze_impact_data {
                    use crate::io::format::JsonResponse;
                    if impacted.is_empty() {
                        // No symbols would be impacted
                        let identifier = if let Some(id) = arguments
                            .as_ref()
                            .and_then(|m| m.get("symbol_id"))
                            .and_then(|v| v.as_u64())
                        {
                            format!("symbol_id:{id}")
                        } else {
                            arguments
                                .as_ref()
                                .and_then(|m| m.get("symbol_name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string()
                        };
                        use crate::io::guidance_engine::generate_guidance_from_config;

                        // Create a proper struct for the empty case
                        #[derive(serde::Serialize)]
                        struct EmptyImpactResult {
                            symbol: String,
                            impacted_count: usize,
                            impacted_symbols: Vec<String>,
                            message: String,
                        }

                        let impact_result = EmptyImpactResult {
                            symbol: identifier.clone(),
                            impacted_count: 0,
                            impacted_symbols: vec![],
                            message: "No symbols would be impacted by changes to this symbol"
                                .to_string(),
                        };

                        let mut response = JsonResponse::success(impact_result);

                        // Add guidance for no results case
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "analyze_impact",
                            Some(&identifier),
                            0,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    } else {
                        use crate::io::guidance_engine::generate_guidance_from_config;
                        let mut response = JsonResponse::success(impacted);

                        // Add system guidance
                        let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "analyze_impact",
                            arguments
                                .as_ref()
                                .and_then(|m| m.get("symbol_name"))
                                .and_then(|v| v.as_str()),
                            result_count,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    }
                } else {
                    // Symbol not found
                    use crate::io::format::JsonResponse;
                    let response = if let Some(id) = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_id"))
                        .and_then(|v| v.as_u64())
                    {
                        JsonResponse::not_found("Symbol", &format!("symbol_id:{id}"))
                    } else {
                        let name = arguments
                            .as_ref()
                            .and_then(|m| m.get("symbol_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        JsonResponse::not_found("Symbol", name)
                    };
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(3);
                }
            } else if json && tool == "search_symbols" {
                // Use pre-collected data for JSON output
                if let Some(results) = search_symbols_data {
                    use crate::io::format::JsonResponse;
                    use crate::io::guidance_engine::generate_guidance_from_config;
                    if results.is_empty() {
                        // Create proper struct for empty search results
                        #[derive(serde::Serialize)]
                        struct EmptySearchResult {
                            query: String,
                            result_count: usize,
                            results: Vec<String>,
                            message: String,
                        }

                        let query = arguments
                            .as_ref()
                            .and_then(|m| m.get("query"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        let search_result = EmptySearchResult {
                            query: query.to_string(),
                            result_count: 0,
                            results: vec![],
                            message: "No results found for query".to_string(),
                        };

                        let mut response = JsonResponse::success(search_result);

                        // Add guidance for no results
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "search_symbols",
                            Some(query),
                            0,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    } else {
                        use crate::io::guidance_engine::generate_guidance_from_config;
                        let mut response = JsonResponse::success(results);

                        // Add system guidance
                        let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "search_symbols",
                            arguments
                                .as_ref()
                                .and_then(|m| m.get("query"))
                                .and_then(|v| v.as_str()),
                            result_count,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    }
                } else {
                    use crate::io::exit_code::ExitCode;
                    use crate::io::format::JsonResponse;
                    let response = JsonResponse::error(
                        ExitCode::GeneralError,
                        "Failed to execute search",
                        vec!["Check query syntax"],
                    );
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(1);
                }
            } else if json && tool == "semantic_search_docs" {
                // Use pre-collected data for JSON output
                if let Some(results) = semantic_search_docs_data {
                    use crate::io::format::JsonResponse;
                    use crate::io::guidance_engine::generate_guidance_from_config;
                    if results.is_empty() {
                        // Create proper struct for empty semantic search
                        #[derive(serde::Serialize)]
                        struct EmptySemanticResult {
                            query: String,
                            result_count: usize,
                            results: Vec<String>,
                            message: String,
                        }

                        let query = arguments
                            .as_ref()
                            .and_then(|m| m.get("query"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        let semantic_result = EmptySemanticResult {
                            query: query.to_string(),
                            result_count: 0,
                            results: vec![],
                            message: "No semantically similar documentation found".to_string(),
                        };

                        let mut response = JsonResponse::success(semantic_result);

                        // Add guidance for no results
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "semantic_search_docs",
                            Some(query),
                            0,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    } else {
                        let mut response = JsonResponse::success(results);

                        // Add system guidance for AI assistants
                        let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "semantic_search_docs",
                            arguments
                                .as_ref()
                                .and_then(|m| m.get("query"))
                                .and_then(|v| v.as_str()),
                            result_count,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    }
                } else if !has_semantic_search {
                    use crate::io::exit_code::ExitCode;
                    use crate::io::format::JsonResponse;
                    let response = JsonResponse::error(
                        ExitCode::GeneralError,
                        "Semantic search is not enabled",
                        vec!["Enable semantic search in settings.toml and rebuild the index"],
                    );
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(1);
                } else {
                    use crate::io::exit_code::ExitCode;
                    use crate::io::format::JsonResponse;
                    let response = JsonResponse::error(
                        ExitCode::GeneralError,
                        "Failed to execute semantic search",
                        vec!["Check query syntax"],
                    );
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(1);
                }
            } else if json && tool == "semantic_search_with_context" {
                // Use pre-collected data for JSON output
                if let Some(results) = semantic_search_with_context_data {
                    use crate::io::format::JsonResponse;
                    use crate::io::guidance_engine::generate_guidance_from_config;
                    if results.is_empty() {
                        // Create proper struct for empty semantic search with context
                        #[derive(serde::Serialize)]
                        struct EmptyContextResult {
                            query: String,
                            result_count: usize,
                            results: Vec<String>,
                            message: String,
                        }

                        let query = arguments
                            .as_ref()
                            .and_then(|m| m.get("query"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        let context_result = EmptyContextResult {
                            query: query.to_string(),
                            result_count: 0,
                            results: vec![],
                            message: "No semantically similar documentation found".to_string(),
                        };

                        let mut response = JsonResponse::success(context_result);

                        // Add guidance for no results
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "semantic_search_with_context",
                            Some(query),
                            0,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    } else {
                        use crate::io::guidance_engine::generate_guidance_from_config;
                        let mut response = JsonResponse::success(results);

                        // Add system guidance
                        let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                        if let Some(guidance) = generate_guidance_from_config(
                            &guidance_config,
                            "semantic_search_with_context",
                            arguments
                                .as_ref()
                                .and_then(|m| m.get("query"))
                                .and_then(|v| v.as_str()),
                            result_count,
                        ) {
                            response = response.with_system_message(&guidance);
                        }

                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    }
                } else if !has_semantic_search {
                    use crate::io::exit_code::ExitCode;
                    use crate::io::format::JsonResponse;
                    let response = JsonResponse::error(
                        ExitCode::GeneralError,
                        "Semantic search is not enabled",
                        vec!["Enable semantic search in settings.toml and rebuild the index"],
                    );
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(1);
                } else {
                    use crate::io::exit_code::ExitCode;
                    use crate::io::format::JsonResponse;
                    let response = JsonResponse::error(
                        ExitCode::GeneralError,
                        "Failed to execute semantic search with context",
                        vec!["Check query syntax"],
                    );
                    println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    std::process::exit(1);
                }
            } else {
                // Default text output
                for content in &call_result.content {
                    match &**content {
                        rmcp::model::RawContent::Text(text_content) => {
                            println!("{}", text_content.text);
                        }
                        _ => {
                            eprintln!("Warning: Non-text content returned");
                        }
                    }
                }
            }
        }
        Err(e) => {
            if json {
                use crate::io::exit_code::ExitCode;
                use crate::io::format::JsonResponse;
                let response = JsonResponse::error(
                    ExitCode::GeneralError,
                    &e.message,
                    vec!["Check the tool name and arguments"],
                );
                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                std::process::exit(1);
            } else {
                eprintln!("Error calling tool: {}", e.message);
                std::process::exit(1);
            }
        }
    }
}
