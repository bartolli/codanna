//! Retrieve command implementations using UnifiedOutput schema

use crate::Symbol;
use crate::indexing::facade::IndexFacade;
use crate::io::{
    EntityType, ExitCode, OutputFormat, OutputManager, OutputStatus,
    schema::{OutputData, OutputMetadata, UnifiedOutput, UnifiedOutputBuilder},
};
use crate::symbol::context::SymbolContext;
use std::borrow::Cow;

/// Execute retrieve symbol command
pub fn retrieve_symbol(
    indexer: &IndexFacade,
    name: &str,
    language: Option<&str>,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);

    // Check if name is a symbol_id (format: "symbol_id:123")
    let symbols = if let Some(id_str) = name.strip_prefix("symbol_id:") {
        // Direct symbol_id lookup
        if let Ok(id) = id_str.parse::<u32>() {
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => vec![sym],
                None => vec![],
            }
        } else {
            vec![]
        }
    } else {
        // Name-based lookup
        indexer.find_symbols_by_name(name, language)
    };

    if symbols.is_empty() {
        // Build not found output
        let unified = UnifiedOutput {
            status: OutputStatus::NotFound,
            entity_type: EntityType::Symbol,
            count: 0,
            data: OutputData::<SymbolContext>::Empty,
            metadata: Some(OutputMetadata {
                query: Some(Cow::Borrowed(name)),
                tool: None,
                timing_ms: None,
                truncated: None,
                extra: Default::default(),
            }),
            guidance: None,
            exit_code: ExitCode::NotFound,
        };

        match output.unified(unified) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("Error writing output: {e}");
                ExitCode::GeneralError
            }
        }
    } else {
        // Transform symbols to SymbolContext with file paths and relationships
        use crate::symbol::context::ContextIncludes;

        let symbols_with_path: Vec<SymbolContext> = symbols
            .into_iter()
            .filter_map(|symbol| {
                // Get full context with relationships (same as MCP find_symbol)
                indexer.get_symbol_context(
                    symbol.id,
                    ContextIncludes::IMPLEMENTATIONS
                        | ContextIncludes::DEFINITIONS
                        | ContextIncludes::CALLERS,
                )
            })
            .collect();

        let unified = UnifiedOutputBuilder::items(symbols_with_path, EntityType::Symbol)
            .with_metadata(OutputMetadata {
                query: Some(Cow::Borrowed(name)),
                tool: None,
                timing_ms: None,
                truncated: None,
                extra: Default::default(),
            })
            .build();

        match output.unified(unified) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("Error writing output: {e}");
                ExitCode::GeneralError
            }
        }
    }
}

/// Execute retrieve callers command
pub fn retrieve_callers(
    indexer: &IndexFacade,
    function: &str,
    language: Option<&str>,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);

    // Check if function is a symbol_id (format: "symbol_id:123")
    let (symbol, query_str) = if let Some(id_str) = function.strip_prefix("symbol_id:") {
        // Direct symbol_id lookup
        if let Ok(id) = id_str.parse::<u32>() {
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => (sym, format!("symbol_id:{id}")),
                None => {
                    let unified = UnifiedOutput {
                        status: OutputStatus::NotFound,
                        entity_type: EntityType::Function,
                        count: 0,
                        data: OutputData::<SymbolContext>::Empty,
                        metadata: Some(OutputMetadata {
                            query: Some(Cow::Owned(format!("symbol_id:{id}"))),
                            tool: None,
                            timing_ms: None,
                            truncated: None,
                            extra: Default::default(),
                        }),
                        guidance: None,
                        exit_code: ExitCode::NotFound,
                    };
                    return match output.unified(unified) {
                        Ok(code) => code,
                        Err(e) => {
                            eprintln!("Error writing output: {e}");
                            ExitCode::GeneralError
                        }
                    };
                }
            }
        } else {
            eprintln!("Invalid symbol_id format: {id_str}");
            return ExitCode::GeneralError;
        }
    } else {
        // Lookup by name
        let symbols = indexer.find_symbols_by_name(function, language);

        if symbols.is_empty() {
            let unified = UnifiedOutput {
                status: OutputStatus::NotFound,
                entity_type: EntityType::Function,
                count: 0,
                data: OutputData::<SymbolContext>::Empty,
                metadata: Some(OutputMetadata {
                    query: Some(Cow::Borrowed(function)),
                    tool: None,
                    timing_ms: None,
                    truncated: None,
                    extra: Default::default(),
                }),
                guidance: None,
                exit_code: ExitCode::NotFound,
            };

            return match output.unified(unified) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("Error writing output: {e}");
                    ExitCode::GeneralError
                }
            };
        }

        if symbols.len() > 1 {
            // AMBIGUOUS - return error with list of symbol IDs
            eprintln!(
                "Ambiguous: found {} symbol(s) named '{}':",
                symbols.len(),
                function
            );
            for (i, sym) in symbols.iter().take(10).enumerate() {
                eprintln!(
                    "  {}. symbol_id:{} - {:?} at {}:{}",
                    i + 1,
                    sym.id.value(),
                    sym.kind,
                    sym.file_path,
                    sym.range.start_line + 1
                );
            }
            if symbols.len() > 10 {
                eprintln!("  ... and {} more", symbols.len() - 10);
            }
            eprintln!("\nUse: codanna retrieve callers symbol_id:<id>");
            return ExitCode::GeneralError;
        }

        // Single match - use it
        (symbols.into_iter().next().unwrap(), function.to_string())
    };

    // Get callers for THIS SPECIFIC symbol only (no aggregation)
    let callers = indexer.get_calling_functions_with_metadata(symbol.id);
    let all_callers: Vec<Symbol> = callers
        .into_iter()
        .map(|(caller, _metadata)| caller)
        .collect();

    // Transform to SymbolContext with relationships
    use crate::symbol::context::ContextIncludes;

    let callers_with_path: Vec<SymbolContext> = all_callers
        .into_iter()
        .filter_map(|symbol| {
            // Get context for each caller symbol (what it calls and defines)
            indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::CALLS | ContextIncludes::DEFINITIONS,
            )
        })
        .collect();

    let unified = UnifiedOutputBuilder::items(callers_with_path, EntityType::Function)
        .with_metadata(OutputMetadata {
            query: Some(Cow::Owned(query_str)),
            tool: None,
            timing_ms: None,
            truncated: None,
            extra: Default::default(),
        })
        .build();

    match output.unified(unified) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error writing output: {e}");
            ExitCode::GeneralError
        }
    }
}

/// Execute retrieve calls command
pub fn retrieve_calls(
    indexer: &IndexFacade,
    function: &str,
    language: Option<&str>,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);

    // Check if function is a symbol_id (format: "symbol_id:123" or just "123" if numeric)
    let (symbol, query_str) = if let Some(id_str) = function.strip_prefix("symbol_id:") {
        // Direct symbol_id lookup
        if let Ok(id) = id_str.parse::<u32>() {
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => (sym, format!("symbol_id:{id}")),
                None => {
                    let unified = UnifiedOutput {
                        status: OutputStatus::NotFound,
                        entity_type: EntityType::Function,
                        count: 0,
                        data: OutputData::<SymbolContext>::Empty,
                        metadata: Some(OutputMetadata {
                            query: Some(Cow::Owned(format!("symbol_id:{id}"))),
                            tool: None,
                            timing_ms: None,
                            truncated: None,
                            extra: Default::default(),
                        }),
                        guidance: None,
                        exit_code: ExitCode::NotFound,
                    };
                    return match output.unified(unified) {
                        Ok(code) => code,
                        Err(e) => {
                            eprintln!("Error writing output: {e}");
                            ExitCode::GeneralError
                        }
                    };
                }
            }
        } else {
            eprintln!("Invalid symbol_id format: {id_str}");
            return ExitCode::GeneralError;
        }
    } else {
        // Lookup by name
        let symbols = indexer.find_symbols_by_name(function, language);

        if symbols.is_empty() {
            let unified = UnifiedOutput {
                status: OutputStatus::NotFound,
                entity_type: EntityType::Function,
                count: 0,
                data: OutputData::<SymbolContext>::Empty,
                metadata: Some(OutputMetadata {
                    query: Some(Cow::Borrowed(function)),
                    tool: None,
                    timing_ms: None,
                    truncated: None,
                    extra: Default::default(),
                }),
                guidance: None,
                exit_code: ExitCode::NotFound,
            };

            return match output.unified(unified) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("Error writing output: {e}");
                    ExitCode::GeneralError
                }
            };
        }

        if symbols.len() > 1 {
            // AMBIGUOUS - return error with list of symbol IDs
            eprintln!(
                "Ambiguous: found {} symbol(s) named '{}':",
                symbols.len(),
                function
            );
            for (i, sym) in symbols.iter().take(10).enumerate() {
                eprintln!(
                    "  {}. symbol_id:{} - {:?} at {}:{}",
                    i + 1,
                    sym.id.value(),
                    sym.kind,
                    sym.file_path,
                    sym.range.start_line + 1
                );
            }
            if symbols.len() > 10 {
                eprintln!("  ... and {} more", symbols.len() - 10);
            }
            eprintln!("\nUse: codanna retrieve calls symbol_id:<id>");
            return ExitCode::GeneralError;
        }

        // Single match - use it
        (symbols.into_iter().next().unwrap(), function.to_string())
    };

    // Get calls for THIS SPECIFIC symbol only (no aggregation)
    let calls = indexer.get_called_functions_with_metadata(symbol.id);
    let all_calls: Vec<Symbol> = calls
        .into_iter()
        .map(|(called, _metadata)| called)
        .collect();

    // Transform to SymbolContext with relationships
    use crate::symbol::context::ContextIncludes;

    let calls_with_path: Vec<SymbolContext> = all_calls
        .into_iter()
        .filter_map(|symbol| {
            // Get context for each called function (who calls it, what it defines)
            indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::CALLERS | ContextIncludes::DEFINITIONS,
            )
        })
        .collect();

    let unified = UnifiedOutputBuilder::items(calls_with_path, EntityType::Function)
        .with_metadata(OutputMetadata {
            query: Some(Cow::Owned(query_str)),
            tool: None,
            timing_ms: None,
            truncated: None,
            extra: Default::default(),
        })
        .build();

    match output.unified(unified) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error writing output: {e}");
            ExitCode::GeneralError
        }
    }
}

/// Execute retrieve implementations command
pub fn retrieve_implementations(
    indexer: &IndexFacade,
    trait_name: &str,
    language: Option<&str>,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);

    // Find the trait symbol first
    let trait_symbols = indexer.find_symbols_by_name(trait_name, language);
    let implementations = if let Some(trait_symbol) = trait_symbols.first() {
        indexer.get_implementations(trait_symbol.id)
    } else {
        vec![]
    };

    // Transform implementations to SymbolContext with relationships
    use crate::symbol::context::ContextIncludes;

    let impls_with_path: Vec<SymbolContext> = implementations
        .into_iter()
        .filter_map(|symbol| {
            // Get context for each implementation (what it defines, what calls it)
            indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::DEFINITIONS | ContextIncludes::CALLERS,
            )
        })
        .collect();

    let unified = UnifiedOutputBuilder::items(impls_with_path, EntityType::Trait)
        .with_metadata(OutputMetadata {
            query: Some(Cow::Borrowed(trait_name)),
            tool: None,
            timing_ms: None,
            truncated: None,
            extra: Default::default(),
        })
        .build();

    match output.unified(unified) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error writing output: {e}");
            ExitCode::GeneralError
        }
    }
}

/// Execute retrieve search command
pub fn retrieve_search(
    indexer: &IndexFacade,
    query: &str,
    limit: usize,
    kind: Option<&str>,
    module: Option<&str>,
    language: Option<&str>,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);

    // Parse the kind filter if provided
    let kind_filter = kind.and_then(|k| match k.to_lowercase().as_str() {
        "function" => Some(crate::SymbolKind::Function),
        "struct" => Some(crate::SymbolKind::Struct),
        "trait" => Some(crate::SymbolKind::Trait),
        "interface" => Some(crate::SymbolKind::Interface),
        "class" => Some(crate::SymbolKind::Class),
        "method" => Some(crate::SymbolKind::Method),
        "field" => Some(crate::SymbolKind::Field),
        "variable" => Some(crate::SymbolKind::Variable),
        "constant" => Some(crate::SymbolKind::Constant),
        "module" => Some(crate::SymbolKind::Module),
        "typealias" => Some(crate::SymbolKind::TypeAlias),
        "enum" => Some(crate::SymbolKind::Enum),
        _ => {
            eprintln!("Warning: Unknown symbol kind '{k}', ignoring filter");
            None
        }
    });

    let search_results = indexer
        .search(query, limit, kind_filter, module, language)
        .unwrap_or_default();

    // Transform search results to SymbolContext with relationships
    use crate::symbol::context::ContextIncludes;

    let results_with_path: Vec<SymbolContext> = search_results
        .into_iter()
        .filter_map(|result| {
            // Get full context for each search result
            indexer.get_symbol_context(
                result.symbol_id,
                ContextIncludes::IMPLEMENTATIONS
                    | ContextIncludes::DEFINITIONS
                    | ContextIncludes::CALLERS,
            )
        })
        .collect();

    let unified = UnifiedOutputBuilder::items(results_with_path, EntityType::SearchResult)
        .with_metadata(OutputMetadata {
            query: Some(Cow::Borrowed(query)),
            tool: None,
            timing_ms: None,
            truncated: None,
            extra: Default::default(),
        })
        .build();

    match output.unified(unified) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error writing output: {e}");
            ExitCode::GeneralError
        }
    }
}

/// Execute retrieve impact command
// DEPRECATED: This function has been disabled.
// Use MCP semantic_search_with_context or slash commands instead.
// The impact command had fundamental flaws:
// - Only worked for functions, not structs/traits/enums
// - Returned empty results for valid symbols
// - Conceptually wrong (not all symbols have "impact")
#[allow(dead_code)]
pub fn retrieve_impact(
    indexer: &IndexFacade,
    symbol_name: &str,
    max_depth: usize,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);
    let symbols = indexer.find_symbols_by_name(symbol_name, None);

    if symbols.is_empty() {
        let unified = UnifiedOutput {
            status: OutputStatus::NotFound,
            entity_type: EntityType::Impact,
            count: 0,
            data: OutputData::<SymbolContext>::Empty,
            metadata: Some(OutputMetadata {
                query: Some(Cow::Borrowed(symbol_name)),
                tool: None,
                timing_ms: None,
                truncated: None,
                extra: Default::default(),
            }),
            guidance: None,
            exit_code: ExitCode::NotFound,
        };

        match output.unified(unified) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("Error writing output: {e}");
                ExitCode::GeneralError
            }
        }
    } else {
        // Get impact analysis for the first matching symbol
        let symbol = &symbols[0];
        let impact_symbol_ids = indexer.get_impact_radius(symbol.id, Some(max_depth));

        // Transform impact symbols to SymbolContext with relationships
        use crate::symbol::context::ContextIncludes;

        let impact_with_path: Vec<SymbolContext> = impact_symbol_ids
            .into_iter()
            .filter_map(|symbol_id| {
                // Get full context for each impacted symbol
                indexer.get_symbol_context(
                    symbol_id,
                    ContextIncludes::CALLERS | ContextIncludes::CALLS,
                )
            })
            .collect();

        let unified = UnifiedOutputBuilder::items(impact_with_path, EntityType::Impact)
            .with_metadata(OutputMetadata {
                query: Some(Cow::Borrowed(symbol_name)),
                tool: None,
                timing_ms: None,
                truncated: None,
                extra: Default::default(),
            })
            .build();

        match output.unified(unified) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("Error writing output: {e}");
                ExitCode::GeneralError
            }
        }
    }
}

/// Execute retrieve describe command
pub fn retrieve_describe(
    indexer: &IndexFacade,
    symbol_name: &str,
    language: Option<&str>,
    format: OutputFormat,
) -> ExitCode {
    let mut output = OutputManager::new(format);

    // Check if symbol_name is a symbol_id (format: "symbol_id:123")
    let (symbol, query_str) = if let Some(id_str) = symbol_name.strip_prefix("symbol_id:") {
        // Direct symbol_id lookup
        if let Ok(id) = id_str.parse::<u32>() {
            match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => (sym, format!("symbol_id:{id}")),
                None => {
                    let unified = UnifiedOutput {
                        status: OutputStatus::NotFound,
                        entity_type: EntityType::Symbol,
                        count: 0,
                        data: OutputData::<SymbolContext>::Empty,
                        metadata: Some(OutputMetadata {
                            query: Some(Cow::Owned(format!("symbol_id:{id}"))),
                            tool: None,
                            timing_ms: None,
                            truncated: None,
                            extra: Default::default(),
                        }),
                        guidance: None,
                        exit_code: ExitCode::NotFound,
                    };
                    return match output.unified(unified) {
                        Ok(code) => code,
                        Err(e) => {
                            eprintln!("Error writing output: {e}");
                            ExitCode::GeneralError
                        }
                    };
                }
            }
        } else {
            eprintln!("Invalid symbol_id format: {id_str}");
            return ExitCode::GeneralError;
        }
    } else {
        // Lookup by name
        let symbols = indexer.find_symbols_by_name(symbol_name, language);

        if symbols.is_empty() {
            let unified = UnifiedOutput {
                status: OutputStatus::NotFound,
                entity_type: EntityType::Symbol,
                count: 0,
                data: OutputData::<SymbolContext>::Empty,
                metadata: Some(OutputMetadata {
                    query: Some(Cow::Borrowed(symbol_name)),
                    tool: None,
                    timing_ms: None,
                    truncated: None,
                    extra: Default::default(),
                }),
                guidance: None,
                exit_code: ExitCode::NotFound,
            };

            return match output.unified(unified) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("Error writing output: {e}");
                    ExitCode::GeneralError
                }
            };
        }

        if symbols.len() > 1 {
            // AMBIGUOUS - return error with list of symbol IDs
            eprintln!(
                "Ambiguous: found {} symbol(s) named '{}':",
                symbols.len(),
                symbol_name
            );
            for (i, sym) in symbols.iter().take(10).enumerate() {
                eprintln!(
                    "  {}. symbol_id:{} - {:?} at {}:{}",
                    i + 1,
                    sym.id.value(),
                    sym.kind,
                    sym.file_path,
                    sym.range.start_line + 1
                );
            }
            if symbols.len() > 10 {
                eprintln!("  ... and {} more", symbols.len() - 10);
            }
            eprintln!("\nUse: codanna retrieve describe symbol_id:<id>");
            return ExitCode::GeneralError;
        }

        // Single match - use it
        (symbols.into_iter().next().unwrap(), symbol_name.to_string())
    };

    // Get relationships for THIS SPECIFIC symbol only (no aggregation)
    let file_path = SymbolContext::symbol_location(&symbol);

    let mut context = SymbolContext {
        symbol: symbol.clone(),
        file_path,
        relationships: Default::default(),
    };

    // Get calls for this specific symbol
    let calls = indexer.get_called_functions_with_metadata(symbol.id);
    if !calls.is_empty() {
        context.relationships.calls = Some(calls);
    }

    // Get callers for this specific symbol
    let callers = indexer.get_calling_functions_with_metadata(symbol.id);
    if !callers.is_empty() {
        context.relationships.called_by = Some(callers);
    }

    // Get defines for this specific symbol
    let deps = indexer.get_dependencies(symbol.id);
    if let Some(defines) = deps.get(&crate::RelationKind::Defines) {
        context.relationships.defines = Some(defines.clone());
    }

    // Load implementations (for traits/interfaces) and implements (for types)
    use crate::SymbolKind;
    match symbol.kind {
        SymbolKind::Trait | SymbolKind::Interface => {
            let implementations = indexer.get_implementations(symbol.id);
            if !implementations.is_empty() {
                context.relationships.implemented_by = Some(implementations);
            }
        }
        SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Class => {
            // What traits does this type implement?
            let impls = indexer.get_implemented_traits(symbol.id);
            if !impls.is_empty() {
                context.relationships.implements = Some(impls);
            }
        }
        _ => {}
    }

    // Load extends relationships (for classes)
    match symbol.kind {
        SymbolKind::Class | SymbolKind::Struct => {
            // What does this class extend?
            let extends = indexer.get_extends(symbol.id);
            if !extends.is_empty() {
                context.relationships.extends = Some(extends);
            }

            // What classes extend this class?
            let extended_by = indexer.get_extended_by(symbol.id);
            if !extended_by.is_empty() {
                context.relationships.extended_by = Some(extended_by);
            }
        }
        _ => {}
    }

    // Load uses relationships (for all symbols)
    let uses = indexer.get_uses(symbol.id);
    if !uses.is_empty() {
        context.relationships.uses = Some(uses);
    }

    let used_by = indexer.get_used_by(symbol.id);
    if !used_by.is_empty() {
        context.relationships.used_by = Some(used_by);
    }

    let unified = UnifiedOutput {
        status: OutputStatus::Success,
        entity_type: EntityType::Symbol,
        count: 1,
        data: OutputData::Single {
            item: Box::new(context),
        },
        metadata: Some(OutputMetadata {
            query: Some(Cow::Owned(query_str)),
            tool: None,
            timing_ms: None,
            truncated: None,
            extra: Default::default(),
        }),
        guidance: None,
        exit_code: ExitCode::Success,
    };

    match output.unified(unified) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error writing output: {e}");
            ExitCode::GeneralError
        }
    }
}
