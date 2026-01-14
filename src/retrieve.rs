//! Retrieve command implementations using Envelope schema for JSON output.
//!
//! This module uses QueryContext to reduce duplication across retrieve functions.

use crate::Symbol;
use crate::indexing::facade::IndexFacade;
use crate::io::{
    EntityType, ExitCode, OutputFormat, OutputManager, OutputStatus,
    envelope::{EntityType as EnvelopeEntityType, Envelope, ResultCode},
    schema::{OutputData, OutputMetadata, UnifiedOutput, UnifiedOutputBuilder},
};
use crate::symbol::context::SymbolContext;
use serde::Serialize;
use std::borrow::Cow;
use std::fmt::Display;

// =============================================================================
// QueryContext - Shared abstraction for retrieve commands
// =============================================================================

/// Result of symbol resolution.
pub enum ResolveResult {
    /// Found exactly one symbol
    Found(Symbol),
    /// Symbol not found
    NotFound,
    /// Multiple symbols match (ambiguous)
    Ambiguous(Vec<Symbol>),
    /// Invalid symbol_id format
    InvalidId(String),
}

/// Shared query execution context for retrieve commands.
///
/// Reduces code duplication by centralizing:
/// - Symbol resolution (name or symbol_id:XXX)
/// - Not-found / ambiguous handling
/// - Envelope construction for JSON output
/// - Text formatting for terminal output
pub struct QueryContext<'a> {
    indexer: &'a IndexFacade,
    format: OutputFormat,
    fields: Option<Vec<String>>,
    entity_type: EnvelopeEntityType,
    command_name: &'static str,
}

impl<'a> QueryContext<'a> {
    /// Create a new query context.
    pub fn new(
        indexer: &'a IndexFacade,
        format: OutputFormat,
        fields: Option<Vec<String>>,
        entity_type: EnvelopeEntityType,
        command_name: &'static str,
    ) -> Self {
        Self {
            indexer,
            format,
            fields,
            entity_type,
            command_name,
        }
    }

    /// Resolve a symbol by name or symbol_id:XXX format.
    pub fn resolve_symbol(&self, query: &str, language: Option<&str>) -> ResolveResult {
        // Check for symbol_id:XXX format
        if let Some(id_str) = query.strip_prefix("symbol_id:") {
            match id_str.parse::<u32>() {
                Ok(id) => match self.indexer.get_symbol(crate::SymbolId(id)) {
                    Some(sym) => ResolveResult::Found(sym),
                    None => ResolveResult::NotFound,
                },
                Err(_) => ResolveResult::InvalidId(id_str.to_string()),
            }
        } else {
            // Name-based lookup
            let symbols = self.indexer.find_symbols_by_name(query, language);
            match symbols.len() {
                0 => ResolveResult::NotFound,
                1 => ResolveResult::Found(symbols.into_iter().next().unwrap()),
                _ => ResolveResult::Ambiguous(symbols),
            }
        }
    }

    /// Handle resolution errors and return appropriate exit code.
    pub fn handle_resolve_error(&self, result: ResolveResult, query: &str) -> ExitCode {
        match result {
            ResolveResult::Found(_) => ExitCode::Success, // Should not happen
            ResolveResult::NotFound => self.output_not_found(query),
            ResolveResult::Ambiguous(symbols) => self.output_ambiguous(query, &symbols),
            ResolveResult::InvalidId(id) => self.output_invalid_id(&id),
        }
    }

    /// Output not-found result.
    pub fn output_not_found(&self, query: &str) -> ExitCode {
        if self.format == OutputFormat::Json {
            let envelope: Envelope<()> = Envelope::not_found(format!(
                "No symbol found for '{query}'"
            ))
            .with_entity_type(self.entity_type)
            .with_query(query)
            .with_hint(
                "Use codanna retrieve symbol <name> to search, or try semantic_search_with_context"
                    .to_string(),
            );

            println!("{}", envelope.to_json().expect("envelope serialization"));
            ExitCode::NotFound
        } else {
            eprintln!("Not found: '{query}'");
            ExitCode::NotFound
        }
    }

    /// Output ambiguous match result.
    pub fn output_ambiguous(&self, query: &str, symbols: &[Symbol]) -> ExitCode {
        if self.format == OutputFormat::Json {
            // In JSON mode, return an error with suggestions
            let suggestions: Vec<String> = symbols
                .iter()
                .take(10)
                .map(|s| format!("symbol_id:{}", s.id.value()))
                .collect();

            let envelope: Envelope<()> = Envelope::error(
                ResultCode::InvalidQuery,
                format!(
                    "Ambiguous: found {} symbol(s) named '{query}'",
                    symbols.len()
                ),
            )
            .with_entity_type(self.entity_type)
            .with_query(query)
            .with_hint(format!(
                "Use: codanna retrieve {} symbol_id:<id>",
                self.command_name
            ));

            // Add context with symbol details
            let context: Vec<serde_json::Value> = symbols
                .iter()
                .take(10)
                .map(|s| {
                    serde_json::json!({
                        "symbol_id": s.id.value(),
                        "kind": format!("{:?}", s.kind),
                        "file_path": s.file_path,
                        "line": s.range.start_line + 1
                    })
                })
                .collect();

            let envelope = envelope.with_error_details(crate::io::envelope::ErrorDetails {
                suggestions,
                context: Some(serde_json::json!(context)),
            });

            println!("{}", envelope.to_json().expect("envelope serialization"));
            ExitCode::GeneralError
        } else {
            // Text mode - print to stderr
            eprintln!(
                "Ambiguous: found {} symbol(s) named '{}':",
                symbols.len(),
                query
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
            eprintln!(
                "\nUse: codanna retrieve {} symbol_id:<id>",
                self.command_name
            );
            ExitCode::GeneralError
        }
    }

    /// Output invalid symbol_id error.
    pub fn output_invalid_id(&self, id: &str) -> ExitCode {
        if self.format == OutputFormat::Json {
            let envelope: Envelope<()> = Envelope::error(
                ResultCode::InvalidQuery,
                format!("Invalid symbol_id format: '{id}'"),
            )
            .with_hint("symbol_id must be a positive integer");

            println!("{}", envelope.to_json().expect("envelope serialization"));
        } else {
            eprintln!("Invalid symbol_id format: {id}");
        }
        ExitCode::GeneralError
    }

    /// Output success with data items.
    pub fn output_success<T: Serialize + Display>(
        &self,
        data: Vec<T>,
        query: &str,
        hint: Option<&str>,
    ) -> ExitCode {
        let count = data.len();

        if self.format == OutputFormat::Json {
            let mut envelope = Envelope::success(data)
                .with_entity_type(self.entity_type)
                .with_count(count)
                .with_query(query)
                .with_message(format!("Found {count} result(s)"));

            if let Some(h) = hint {
                envelope = envelope.with_hint(h);
            }

            let json = if let Some(ref fields) = self.fields {
                envelope.to_json_with_fields(fields)
            } else {
                envelope.to_json()
            };

            println!("{}", json.expect("envelope serialization"));
            ExitCode::Success
        } else {
            // Text mode - use Display trait
            for item in &data {
                println!("{item}");
            }
            ExitCode::Success
        }
    }

    /// Output empty success (symbol found but no results).
    pub fn output_empty(&self, query: &str, message: &str) -> ExitCode {
        if self.format == OutputFormat::Json {
            let envelope: Envelope<Vec<()>> = Envelope::success(vec![])
                .with_entity_type(self.entity_type)
                .with_count(0)
                .with_query(query)
                .with_message(message);

            println!("{}", envelope.to_json().expect("envelope serialization"));
        } else {
            println!("{message}");
        }
        ExitCode::Success
    }
}

/// Execute retrieve symbol command
///
/// Unlike callers/calls, this returns ALL matching symbols (not ambiguous error).
pub fn retrieve_symbol(
    indexer: &IndexFacade,
    name: &str,
    language: Option<&str>,
    format: OutputFormat,
    fields: Option<Vec<String>>,
) -> ExitCode {
    use crate::symbol::context::ContextIncludes;

    // Check if name is a symbol_id (format: "symbol_id:123")
    let symbols = if let Some(id_str) = name.strip_prefix("symbol_id:") {
        // Direct symbol_id lookup
        match id_str.parse::<u32>() {
            Ok(id) => match indexer.get_symbol(crate::SymbolId(id)) {
                Some(sym) => vec![sym],
                None => vec![],
            },
            Err(_) => {
                // Invalid symbol_id format
                if format == OutputFormat::Json {
                    let envelope: Envelope<()> = Envelope::error(
                        ResultCode::InvalidQuery,
                        format!("Invalid symbol_id format: '{id_str}'"),
                    )
                    .with_hint("symbol_id must be a positive integer");
                    println!("{}", envelope.to_json().expect("envelope serialization"));
                } else {
                    eprintln!("Invalid symbol_id format: {id_str}");
                }
                return ExitCode::GeneralError;
            }
        }
    } else {
        // Name-based lookup
        indexer.find_symbols_by_name(name, language)
    };

    if symbols.is_empty() {
        // Not found
        if format == OutputFormat::Json {
            let envelope: Envelope<()> = Envelope::not_found(format!("No symbol found for '{name}'"))
                .with_entity_type(EnvelopeEntityType::Symbol)
                .with_query(name)
                .with_hint("Use codanna retrieve search <query> for fuzzy matching, or try semantic_search_with_context");
            println!("{}", envelope.to_json().expect("envelope serialization"));
        } else {
            eprintln!("Not found: '{name}'");
        }
        return ExitCode::NotFound;
    }

    // Transform symbols to SymbolContext with file paths and relationships
    let symbols_with_context: Vec<SymbolContext> = symbols
        .into_iter()
        .filter_map(|symbol| {
            indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::IMPLEMENTATIONS
                    | ContextIncludes::DEFINITIONS
                    | ContextIncludes::CALLERS,
            )
        })
        .collect();

    let count = symbols_with_context.len();

    if format == OutputFormat::Json {
        let mut envelope = Envelope::success(symbols_with_context)
            .with_entity_type(EnvelopeEntityType::Symbol)
            .with_count(count)
            .with_query(name)
            .with_message(format!("Found {count} symbol(s)"))
            .with_hint("Use symbol_id for precise lookup in subsequent queries");

        // Include language filter in metadata if specified
        if let Some(lang) = language {
            envelope = envelope.with_lang(lang);
        }

        let json = if let Some(ref f) = fields {
            envelope.to_json_with_fields(f)
        } else {
            envelope.to_json()
        };

        println!("{}", json.expect("envelope serialization"));
        ExitCode::Success
    } else {
        // Text output
        for ctx in &symbols_with_context {
            println!("{ctx}");
        }
        ExitCode::Success
    }
}

/// Execute retrieve callers command
///
/// Uses QueryContext for symbol resolution with ambiguous handling.
pub fn retrieve_callers(
    indexer: &IndexFacade,
    function: &str,
    language: Option<&str>,
    format: OutputFormat,
    fields: Option<Vec<String>>,
) -> ExitCode {
    use crate::symbol::context::ContextIncludes;

    // Use QueryContext for symbol resolution
    let ctx = QueryContext::new(
        indexer,
        format,
        fields.clone(),
        EnvelopeEntityType::Callers,
        "callers",
    );

    // Resolve symbol (handles not-found, ambiguous, invalid id)
    let symbol = match ctx.resolve_symbol(function, language) {
        ResolveResult::Found(s) => s,
        other => return ctx.handle_resolve_error(other, function),
    };

    // Get callers for this specific symbol
    let callers = indexer.get_calling_functions_with_metadata(symbol.id);

    // Handle empty results: symbol exists but has no callers
    if callers.is_empty() {
        return ctx.output_empty(function, &format!("No functions call '{function}'"));
    }

    // Transform to SymbolContext with relationships
    let callers_with_context: Vec<SymbolContext> = callers
        .into_iter()
        .filter_map(|(caller, _metadata)| {
            indexer.get_symbol_context(
                caller.id,
                ContextIncludes::CALLS | ContextIncludes::DEFINITIONS,
            )
        })
        .collect();

    let count = callers_with_context.len();

    if format == OutputFormat::Json {
        let mut envelope = Envelope::success(callers_with_context)
            .with_entity_type(EnvelopeEntityType::Callers)
            .with_count(count)
            .with_query(function)
            .with_message(format!("Found {count} caller(s)"))
            .with_hint("Use symbol_id for precise lookup");

        if let Some(lang) = language {
            envelope = envelope.with_lang(lang);
        }

        let json = if let Some(ref f) = fields {
            envelope.to_json_with_fields(f)
        } else {
            envelope.to_json()
        };

        println!("{}", json.expect("envelope serialization"));
        ExitCode::Success
    } else {
        // Text output
        for ctx in &callers_with_context {
            println!("{ctx}");
        }
        ExitCode::Success
    }
}

/// Execute retrieve calls command
///
/// Uses QueryContext for symbol resolution with ambiguous handling.
pub fn retrieve_calls(
    indexer: &IndexFacade,
    function: &str,
    language: Option<&str>,
    format: OutputFormat,
    fields: Option<Vec<String>>,
) -> ExitCode {
    use crate::symbol::context::ContextIncludes;

    // Use QueryContext for symbol resolution
    let ctx = QueryContext::new(
        indexer,
        format,
        fields.clone(),
        EnvelopeEntityType::Calls,
        "calls",
    );

    // Resolve symbol (handles not-found, ambiguous, invalid id)
    let symbol = match ctx.resolve_symbol(function, language) {
        ResolveResult::Found(s) => s,
        other => return ctx.handle_resolve_error(other, function),
    };

    // Get calls for this specific symbol
    let calls = indexer.get_called_functions_with_metadata(symbol.id);

    // Handle empty results: symbol exists but makes no calls
    if calls.is_empty() {
        return ctx.output_empty(function, &format!("'{function}' makes no function calls"));
    }

    // Transform to SymbolContext with relationships
    let calls_with_context: Vec<SymbolContext> = calls
        .into_iter()
        .filter_map(|(called, _metadata)| {
            indexer.get_symbol_context(
                called.id,
                ContextIncludes::CALLERS | ContextIncludes::DEFINITIONS,
            )
        })
        .collect();

    let count = calls_with_context.len();

    if format == OutputFormat::Json {
        let mut envelope = Envelope::success(calls_with_context)
            .with_entity_type(EnvelopeEntityType::Calls)
            .with_count(count)
            .with_query(function)
            .with_message(format!("Found {count} call(s)"))
            .with_hint("Use symbol_id for precise lookup");

        if let Some(lang) = language {
            envelope = envelope.with_lang(lang);
        }

        let json = if let Some(ref f) = fields {
            envelope.to_json_with_fields(f)
        } else {
            envelope.to_json()
        };

        println!("{}", json.expect("envelope serialization"));
        ExitCode::Success
    } else {
        // Text output
        for ctx in &calls_with_context {
            println!("{ctx}");
        }
        ExitCode::Success
    }
}

/// Execute retrieve implementations command
///
/// Uses QueryContext for symbol resolution with ambiguous handling.
pub fn retrieve_implementations(
    indexer: &IndexFacade,
    trait_name: &str,
    language: Option<&str>,
    format: OutputFormat,
    fields: Option<Vec<String>>,
) -> ExitCode {
    use crate::symbol::context::ContextIncludes;

    // Use QueryContext for symbol resolution
    let ctx = QueryContext::new(
        indexer,
        format,
        fields.clone(),
        EnvelopeEntityType::Symbol, // Implementations are symbols
        "implementations",
    );

    // Resolve trait symbol (handles not-found, ambiguous, invalid id)
    let trait_symbol = match ctx.resolve_symbol(trait_name, language) {
        ResolveResult::Found(s) => s,
        other => return ctx.handle_resolve_error(other, trait_name),
    };

    // Get implementations for this trait
    let implementations = indexer.get_implementations(trait_symbol.id);

    // Handle empty results
    if implementations.is_empty() {
        return ctx.output_empty(
            trait_name,
            &format!("No implementations found for '{trait_name}'"),
        );
    }

    // Transform to SymbolContext with relationships
    let impls_with_context: Vec<SymbolContext> = implementations
        .into_iter()
        .filter_map(|symbol| {
            indexer.get_symbol_context(
                symbol.id,
                ContextIncludes::DEFINITIONS | ContextIncludes::CALLERS,
            )
        })
        .collect();

    let count = impls_with_context.len();

    if format == OutputFormat::Json {
        let mut envelope = Envelope::success(impls_with_context)
            .with_entity_type(EnvelopeEntityType::Symbol)
            .with_count(count)
            .with_query(trait_name)
            .with_message(format!("Found {count} implementation(s)"))
            .with_hint("Use symbol_id for precise lookup");

        if let Some(lang) = language {
            envelope = envelope.with_lang(lang);
        }

        let json = if let Some(ref f) = fields {
            envelope.to_json_with_fields(f)
        } else {
            envelope.to_json()
        };

        println!("{}", json.expect("envelope serialization"));
        ExitCode::Success
    } else {
        // Text output
        for ctx in &impls_with_context {
            println!("{ctx}");
        }
        ExitCode::Success
    }
}

/// Execute retrieve search command
///
/// Full-text search with optional filters. Uses Envelope for JSON output.
pub fn retrieve_search(
    indexer: &IndexFacade,
    query: &str,
    limit: usize,
    kind: Option<&str>,
    module: Option<&str>,
    language: Option<&str>,
    format: OutputFormat,
    fields: Option<Vec<String>>,
) -> ExitCode {
    use crate::symbol::context::ContextIncludes;

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
    let results_with_context: Vec<SymbolContext> = search_results
        .into_iter()
        .filter_map(|result| {
            indexer.get_symbol_context(
                result.symbol_id,
                ContextIncludes::IMPLEMENTATIONS
                    | ContextIncludes::DEFINITIONS
                    | ContextIncludes::CALLERS,
            )
        })
        .collect();

    let count = results_with_context.len();

    if format == OutputFormat::Json {
        // Build envelope
        let envelope = if results_with_context.is_empty() {
            Envelope::not_found(format!("No results for '{query}'"))
                .with_entity_type(EnvelopeEntityType::SearchResult)
                .with_query(query)
                .with_hint("Try broader search terms or use semantic_search_with_context")
        } else {
            let mut env = Envelope::success(results_with_context)
                .with_entity_type(EnvelopeEntityType::SearchResult)
                .with_count(count)
                .with_query(query)
                .with_message(format!("Found {count} result(s)"))
                .with_hint("Use symbol_id for precise lookup");

            if let Some(lang) = language {
                env = env.with_lang(lang);
            }
            env
        };

        let json = if let Some(ref f) = fields {
            envelope.to_json_with_fields(f)
        } else {
            envelope.to_json()
        };

        println!("{}", json.expect("envelope serialization"));

        if count == 0 {
            ExitCode::NotFound
        } else {
            ExitCode::Success
        }
    } else {
        // Text output
        if results_with_context.is_empty() {
            eprintln!("No results for '{query}'");
            ExitCode::NotFound
        } else {
            for ctx in &results_with_context {
                println!("{ctx}");
            }
            ExitCode::Success
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
///
/// Uses QueryContext for symbol resolution with ambiguous handling.
/// Returns full symbol context with all relationships.
pub fn retrieve_describe(
    indexer: &IndexFacade,
    symbol_name: &str,
    language: Option<&str>,
    format: OutputFormat,
    fields: Option<Vec<String>>,
) -> ExitCode {
    // Use QueryContext for symbol resolution
    let ctx = QueryContext::new(
        indexer,
        format,
        fields.clone(),
        EnvelopeEntityType::Symbol,
        "describe",
    );

    // Resolve symbol (handles not-found, ambiguous, invalid id)
    let symbol = match ctx.resolve_symbol(symbol_name, language) {
        ResolveResult::Found(s) => s,
        other => return ctx.handle_resolve_error(other, symbol_name),
    };

    // Build rich context with all relationships
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

    // Output
    if format == OutputFormat::Json {
        let mut envelope = Envelope::success(context)
            .with_entity_type(EnvelopeEntityType::Symbol)
            .with_count(1)
            .with_query(symbol_name)
            .with_message(format!("Symbol '{}' described", symbol.name))
            .with_hint("Use callers/calls commands to explore relationships further");

        if let Some(lang) = language {
            envelope = envelope.with_lang(lang);
        }

        let json = if let Some(ref f) = fields {
            envelope.to_json_with_fields(f)
        } else {
            envelope.to_json()
        };

        println!("{}", json.expect("envelope serialization"));
        ExitCode::Success
    } else {
        // Text output
        println!("{context}");
        ExitCode::Success
    }
}
