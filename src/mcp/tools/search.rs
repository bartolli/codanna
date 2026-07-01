//! Search and info tools: get_index_info, semantic_search_docs,
//! semantic_search_with_context, search_symbols, search_documents.

use rmcp::model::ErrorData as McpError;
use rmcp::model::*;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

use crate::documents::SearchQuery as DocSearchQuery;

use crate::mcp::requests::{
    GetIndexInfoRequest, SearchDocumentsRequest, SearchSymbolsRequest, SemanticSearchRequest,
    SemanticSearchWithContextRequest,
};
use crate::mcp::server::{CodeIntelligenceServer, format_relative_time, generate_mcp_guidance};

#[tool_router(router = search_router, vis = "pub(crate)")]
impl CodeIntelligenceServer {
    #[tool(description = "Get information about the indexed codebase")]
    pub async fn get_index_info(
        &self,
        Parameters(_params): Parameters<GetIndexInfoRequest>,
    ) -> Result<CallToolResult, McpError> {
        let indexer = self.facade.read().await;
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
            let live_count = indexer.semantic_search_embedding_count();
            format!(
                "\n\nSemantic Search:\n  - Status: Enabled\n  - Model: {}\n  - Embeddings: {}\n  - Dimensions: {}\n  - Created: {}\n  - Updated: {}",
                metadata.model_name,
                live_count,
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
        let indexer = self.facade.read().await;

        tracing::debug!(
            target: "mcp",
            "semantic_search_docs called - symbols: {}, semantic: {}",
            indexer.symbol_count(),
            indexer.has_semantic_search()
        );

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
        let indexer = self.facade.read().await;

        if !indexer.has_semantic_search() {
            tracing::debug!(
                target: "mcp",
                "semantic search not available - index_path: {}, has_semantic: {}",
                indexer.settings().index_path.display(),
                indexer.has_semantic_search()
            );
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

                    // Signature
                    if let Some(ref sig) = symbol.signature {
                        output.push_str(&format!("   Signature: {sig}\n"));
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

                    // Show inheritance relationships for classes/structs/enums
                    if matches!(
                        symbol.kind,
                        crate::SymbolKind::Class
                            | crate::SymbolKind::Struct
                            | crate::SymbolKind::Enum
                    ) {
                        // What does this class extend?
                        let extends = indexer.get_extends(symbol.id);
                        if !extends.is_empty() {
                            output.push_str(&format!(
                                "\n   {} extends {} class(es):\n",
                                symbol.name,
                                extends.len()
                            ));
                            for (i, base_class) in extends.iter().take(5).enumerate() {
                                output.push_str(&format!(
                                    "     -> {:?} {} at {} [symbol_id:{}]\n",
                                    base_class.kind,
                                    base_class.name,
                                    crate::symbol::context::SymbolContext::symbol_location(
                                        base_class
                                    ),
                                    base_class.id.value()
                                ));
                                if i == 4 && extends.len() > 5 {
                                    output.push_str(&format!(
                                        "     ... and {} more\n",
                                        extends.len() - 5
                                    ));
                                }
                            }
                        }

                        // What classes extend this class?
                        let extended_by = indexer.get_extended_by(symbol.id);
                        if !extended_by.is_empty() {
                            output.push_str(&format!(
                                "\n   {} class(es) extend {}:\n",
                                extended_by.len(),
                                symbol.name
                            ));
                            for (i, derived_class) in extended_by.iter().take(5).enumerate() {
                                output.push_str(&format!(
                                    "     <- {:?} {} at {} [symbol_id:{}]\n",
                                    derived_class.kind,
                                    derived_class.name,
                                    crate::symbol::context::SymbolContext::symbol_location(
                                        derived_class
                                    ),
                                    derived_class.id.value()
                                ));
                                if i == 4 && extended_by.len() > 5 {
                                    output.push_str(&format!(
                                        "     ... and {} more\n",
                                        extended_by.len() - 5
                                    ));
                                }
                            }
                        }

                        // What traits does this type implement?
                        let implements = indexer.get_implemented_traits(symbol.id);
                        if !implements.is_empty() {
                            output.push_str(&format!(
                                "\n   {} implements {} trait(s):\n",
                                symbol.name,
                                implements.len()
                            ));
                            for (i, trait_sym) in implements.iter().take(5).enumerate() {
                                output.push_str(&format!(
                                    "     -> {:?} {} at {} [symbol_id:{}]\n",
                                    trait_sym.kind,
                                    trait_sym.name,
                                    crate::symbol::context::SymbolContext::symbol_location(
                                        trait_sym
                                    ),
                                    trait_sym.id.value()
                                ));
                                if i == 4 && implements.len() > 5 {
                                    output.push_str(&format!(
                                        "     ... and {} more\n",
                                        implements.len() - 5
                                    ));
                                }
                            }
                        }
                    }

                    // Show what implements this trait/interface
                    if matches!(
                        symbol.kind,
                        crate::SymbolKind::Trait | crate::SymbolKind::Interface
                    ) {
                        let implementations = indexer.get_implementations(symbol.id);
                        if !implementations.is_empty() {
                            output.push_str(&format!(
                                "\n   {} type(s) implement {}:\n",
                                implementations.len(),
                                symbol.name
                            ));
                            for (i, impl_sym) in implementations.iter().take(5).enumerate() {
                                output.push_str(&format!(
                                    "     <- {:?} {} at {} [symbol_id:{}]\n",
                                    impl_sym.kind,
                                    impl_sym.name,
                                    crate::symbol::context::SymbolContext::symbol_location(
                                        impl_sym
                                    ),
                                    impl_sym.id.value()
                                ));
                                if i == 4 && implementations.len() > 5 {
                                    output.push_str(&format!(
                                        "     ... and {} more\n",
                                        implementations.len() - 5
                                    ));
                                }
                            }
                        }
                    }

                    // Show uses relationships (for all symbols)
                    let uses = indexer.get_uses(symbol.id);
                    if !uses.is_empty() {
                        output.push_str(&format!(
                            "\n   {} uses {} type(s):\n",
                            symbol.name,
                            uses.len()
                        ));
                        for (i, used_type) in uses.iter().take(5).enumerate() {
                            output.push_str(&format!(
                                "     -> {:?} {} at {} [symbol_id:{}]\n",
                                used_type.kind,
                                used_type.name,
                                crate::symbol::context::SymbolContext::symbol_location(used_type),
                                used_type.id.value()
                            ));
                            if i == 4 && uses.len() > 5 {
                                output.push_str(&format!("     ... and {} more\n", uses.len() - 5));
                            }
                        }
                    }

                    // What symbols use this type?
                    let used_by = indexer.get_used_by(symbol.id);
                    if !used_by.is_empty() {
                        output.push_str(&format!(
                            "\n   {} type(s) use {}:\n",
                            used_by.len(),
                            symbol.name
                        ));
                        for (i, using_symbol) in used_by.iter().take(5).enumerate() {
                            output.push_str(&format!(
                                "     <- {:?} {} at {} [symbol_id:{}]\n",
                                using_symbol.kind,
                                using_symbol.name,
                                crate::symbol::context::SymbolContext::symbol_location(
                                    using_symbol
                                ),
                                using_symbol.id.value()
                            ));
                            if i == 4 && used_by.len() > 5 {
                                output.push_str(&format!(
                                    "     ... and {} more\n",
                                    used_by.len() - 5
                                ));
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
        let indexer = self.facade.read().await;

        // One kind vocabulary (SymbolKind::from_str); unknown kinds error
        // instead of silently returning unfiltered results.
        let kind_filter = match kind.as_deref().map(str::parse::<crate::SymbolKind>) {
            None => None,
            Some(Ok(k)) => Some(k),
            Some(Err(e)) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {e}"
                ))]));
            }
        };

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

    #[tool(
        description = "Search indexed documents (markdown, text files) using natural language queries. Returns relevant chunks with context and highlighted keywords."
    )]
    pub async fn search_documents(
        &self,
        Parameters(SearchDocumentsRequest {
            query,
            collection,
            limit,
        }): Parameters<SearchDocumentsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let store = match &self.document_store {
            Some(s) => s,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Document search not available. No document collections are indexed.\n\n\
                    To enable:\n\
                    1. Add a collection: codanna documents add-collection docs docs/\n\
                    2. Index it: codanna documents index\n\
                    3. Restart the MCP server",
                )]));
            }
        };

        let mut store = store.write().await;
        let indexer = self.facade.read().await;

        // Auto-sync: check for file changes in all collections before searching
        let settings = indexer.settings();
        for (name, config) in &settings.documents.collections {
            if let Err(e) = store.index_collection(name, config, &settings.documents.defaults) {
                tracing::warn!(target: "rag", "auto-sync failed for collection '{}': {}", name, e);
            }
        }

        let search_query = DocSearchQuery {
            text: query.clone(),
            collection,
            document: None,
            limit: limit as usize,
            preview_config: Some(indexer.settings().documents.search.clone()),
        };

        match store.search(search_query) {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No documents found for: {query}"
                    ))]));
                }

                let mut output = format!(
                    "Found {} document(s) matching '{}':\n\n",
                    results.len(),
                    query
                );

                for (i, result) in results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. {} (score: {:.3})\n",
                        i + 1,
                        result.source_path.display(),
                        result.similarity
                    ));

                    if !result.heading_context.is_empty() {
                        output.push_str(&format!(
                            "   Context: {}\n",
                            result.heading_context.join(" > ")
                        ));
                    }

                    // Preview is already KWIC-processed with highlighting
                    output.push_str(&format!("   Preview: {}\n\n", result.content_preview));
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Document search failed: {e}"
            ))])),
        }
    }
}
