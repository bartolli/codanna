//! Symbol-target tools: find_symbol, get_calls, find_callers, analyze_impact.

use rmcp::model::ErrorData as McpError;
use rmcp::model::*;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

use crate::Symbol;
use crate::mcp::requests::{
    AnalyzeImpactRequest, FindCallersRequest, FindSymbolRequest, GetCallsRequest,
};
use crate::mcp::server::{CodeIntelligenceServer, generate_mcp_guidance};
use crate::mcp::service::{
    self, SymbolResolution, parse_receiver_context, qualified_call, render_ambiguity,
};

#[tool_router(router = symbols_router, vis = "pub(crate)")]
impl CodeIntelligenceServer {
    #[tool(description = "Find a symbol by name in the indexed codebase")]
    pub async fn find_symbol(
        &self,
        Parameters(FindSymbolRequest { name, lang }): Parameters<FindSymbolRequest>,
    ) -> Result<CallToolResult, McpError> {
        use crate::symbol::context::ContextIncludes;

        let indexer = self.facade.read().await;

        // Support symbol_id:XXX format for direct lookup (from semantic search results)
        let symbols = if let Some(id_str) = name.strip_prefix("symbol_id:") {
            if let Ok(id) = id_str.parse::<u32>() {
                indexer
                    .get_symbol(crate::SymbolId(id))
                    .map(|s| vec![s])
                    .unwrap_or_default()
            } else {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid symbol_id format: {id_str}"
                ))]));
            }
        } else {
            indexer.find_symbols_by_name(&name, lang.as_deref())
        };

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

            // Try to get full context with all relationship types
            if let Some(ctx) = indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::IMPLEMENTATIONS
                    | ContextIncludes::DEFINITIONS
                    | ContextIncludes::CALLERS
                    | ContextIncludes::EXTENDS
                    | ContextIncludes::USES,
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

                // What traits this type implements
                if let Some(impls) = &ctx.relationships.implements {
                    if !impls.is_empty() {
                        result.push_str(&format!("Implements: {} trait(s)\n", impls.len()));
                        for trait_sym in impls.iter().take(5) {
                            result.push_str(&format!(
                                "  -> {} at {}\n",
                                trait_sym.name,
                                crate::symbol::context::SymbolContext::symbol_location(trait_sym)
                            ));
                        }
                        if impls.len() > 5 {
                            result.push_str(&format!("  ... and {} more\n", impls.len() - 5));
                        }
                        has_relationships = true;
                    }
                }

                // What types implement this trait
                if let Some(impls) = &ctx.relationships.implemented_by {
                    if !impls.is_empty() {
                        result.push_str(&format!("Implemented by: {} type(s)\n", impls.len()));
                        for impl_sym in impls.iter().take(5) {
                            result.push_str(&format!(
                                "  <- {} at {}\n",
                                impl_sym.name,
                                crate::symbol::context::SymbolContext::symbol_location(impl_sym)
                            ));
                        }
                        if impls.len() > 5 {
                            result.push_str(&format!("  ... and {} more\n", impls.len() - 5));
                        }
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

                // What base class(es) this extends
                if let Some(extends) = &ctx.relationships.extends {
                    if !extends.is_empty() {
                        result.push_str(&format!("Extends: {} class(es)\n", extends.len()));
                        for base in extends.iter().take(3) {
                            result.push_str(&format!(
                                "  -> {} at {}\n",
                                base.name,
                                crate::symbol::context::SymbolContext::symbol_location(base)
                            ));
                        }
                        if extends.len() > 3 {
                            result.push_str(&format!("  ... and {} more\n", extends.len() - 3));
                        }
                        has_relationships = true;
                    }
                }

                // What classes extend this
                if let Some(extended_by) = &ctx.relationships.extended_by {
                    if !extended_by.is_empty() {
                        result.push_str(&format!("Extended by: {} class(es)\n", extended_by.len()));
                        for derived in extended_by.iter().take(3) {
                            result.push_str(&format!(
                                "  <- {} at {}\n",
                                derived.name,
                                crate::symbol::context::SymbolContext::symbol_location(derived)
                            ));
                        }
                        if extended_by.len() > 3 {
                            result.push_str(&format!("  ... and {} more\n", extended_by.len() - 3));
                        }
                        has_relationships = true;
                    }
                }

                // What types this symbol uses
                if let Some(uses) = &ctx.relationships.uses {
                    if !uses.is_empty() {
                        result.push_str(&format!("Uses: {} type(s)\n", uses.len()));
                        for used in uses.iter().take(3) {
                            result.push_str(&format!(
                                "  -> {} at {}\n",
                                used.name,
                                crate::symbol::context::SymbolContext::symbol_location(used)
                            ));
                        }
                        if uses.len() > 3 {
                            result.push_str(&format!("  ... and {} more\n", uses.len() - 3));
                        }
                        has_relationships = true;
                    }
                }

                // What symbols use this type
                if let Some(used_by) = &ctx.relationships.used_by {
                    if !used_by.is_empty() {
                        result.push_str(&format!("Used by: {} symbol(s)\n", used_by.len()));
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
        let indexer = self.facade.read().await;

        // Resolution policy is shared with the CLI JSON path via the
        // service layer; text renderings stay byte-identical.
        let (symbol, identifier) =
            match service::resolve_symbol_or_id(&indexer, symbol_id, function_name) {
                SymbolResolution::Resolved { symbol, identifier } => (symbol, identifier),
                SymbolResolution::NotFoundById(id) => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: symbol_id:{id}"
                    ))]));
                }
                SymbolResolution::NotFoundByName(name) => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Function not found: {name}"
                    ))]));
                }
                SymbolResolution::Ambiguous { name, candidates } => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        render_ambiguity("get_calls", &name, &candidates),
                    )]));
                }
                SymbolResolution::MissingParam => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "Error: Either function_name or symbol_id must be provided".to_string(),
                    )]));
                }
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
                let display = meta
                    .context
                    .as_deref()
                    .and_then(parse_receiver_context)
                    .map(|(receiver, is_static)| qualified_call(receiver, is_static, &callee.name))
                    .unwrap_or_else(|| callee.name.to_string());

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
        let indexer = self.facade.read().await;

        // Shared resolution policy; see service.rs.
        let (symbol, identifier) =
            match service::resolve_symbol_or_id(&indexer, symbol_id, function_name) {
                SymbolResolution::Resolved { symbol, identifier } => (symbol, identifier),
                SymbolResolution::NotFoundById(id) => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: symbol_id:{id}"
                    ))]));
                }
                SymbolResolution::NotFoundByName(name) => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Function not found: {name}"
                    ))]));
                }
                SymbolResolution::Ambiguous { name, candidates } => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        render_ambiguity("find_callers", &name, &candidates),
                    )]));
                }
                SymbolResolution::MissingParam => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "Error: Either function_name or symbol_id must be provided".to_string(),
                    )]));
                }
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
                let info = meta
                    .context
                    .as_deref()
                    .and_then(parse_receiver_context)
                    .map(|(receiver, is_static)| {
                        format!(
                            " (calls {})",
                            qualified_call(receiver, is_static, &symbol.name)
                        )
                    })
                    .unwrap_or_default();

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

        let indexer = self.facade.read().await;

        // Shared resolution policy; see service.rs.
        let (symbol, identifier) =
            match service::resolve_symbol_or_id(&indexer, symbol_id, symbol_name) {
                SymbolResolution::Resolved { symbol, identifier } => (symbol, identifier),
                SymbolResolution::NotFoundById(id) => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: symbol_id:{id}"
                    ))]));
                }
                SymbolResolution::NotFoundByName(name) => {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Symbol not found: {name}"
                    ))]));
                }
                SymbolResolution::Ambiguous { name, candidates } => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        render_ambiguity("analyze_impact", &name, &candidates),
                    )]));
                }
                SymbolResolution::MissingParam => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "Error: Either symbol_name or symbol_id must be provided".to_string(),
                    )]));
                }
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
        if let Some(ctx) = indexer.get_symbol_context(
            symbol.id,
            ContextIncludes::CALLERS | ContextIncludes::EXTENDS | ContextIncludes::USES,
        ) {
            let location = ctx.format_location();
            let direct_callers = ctx
                .relationships
                .called_by
                .as_ref()
                .map(|c| c.len())
                .unwrap_or(0);

            // For classes, also show inheritance info
            let inheritance_info = if matches!(
                symbol.kind,
                crate::SymbolKind::Class | crate::SymbolKind::Struct
            ) {
                let extends_count = ctx
                    .relationships
                    .extends
                    .as_ref()
                    .map(|e| e.len())
                    .unwrap_or(0);
                let extended_by_count = ctx
                    .relationships
                    .extended_by
                    .as_ref()
                    .map(|e| e.len())
                    .unwrap_or(0);

                if extends_count > 0 || extended_by_count > 0 {
                    format!(", extends: {extends_count}, extended by: {extended_by_count}")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            // Show uses info for all symbols
            let uses_count = ctx
                .relationships
                .uses
                .as_ref()
                .map(|u| u.len())
                .unwrap_or(0);
            let used_by_count = ctx
                .relationships
                .used_by
                .as_ref()
                .map(|u| u.len())
                .unwrap_or(0);

            let uses_info = if uses_count > 0 || used_by_count > 0 {
                format!(", uses: {uses_count}, used by: {used_by_count}")
            } else {
                String::new()
            };

            result.push_str(&format!(
                "Symbol: {:?} at {} (direct callers: {}{}{})\n\n",
                symbol.kind, location, direct_callers, inheritance_info, uses_info
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
}
