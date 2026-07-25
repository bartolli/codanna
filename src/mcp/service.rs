//! Shared query layer for MCP handlers and CLI JSON output.
//!
//! One resolution policy and one receiver-metadata codec, consumed by both
//! renderings (MCP text, CLI JSON envelopes) so the two cannot drift. The
//! ambiguity policy is refuse-and-list, never aggregate: relationships from
//! same-named but unrelated symbols must not merge into one result.

use crate::Symbol;
use crate::indexing::facade::IndexFacade;

/// Outcome of resolving a tool's target symbol from `symbol_id` or name.
pub enum SymbolResolution {
    Resolved {
        symbol: Symbol,
        /// Display identifier: the queried name, or `symbol_id:<id>`.
        identifier: String,
    },
    NotFoundById(u32),
    NotFoundByName(String),
    Ambiguous {
        name: String,
        candidates: Vec<Symbol>,
    },
    MissingParam,
}

/// Resolve a target symbol by `symbol_id` (unambiguous) or by name.
/// More than one name match is `Ambiguous` — callers present the candidate
/// list instead of picking or merging.
pub fn resolve_symbol_or_id(
    facade: &IndexFacade,
    symbol_id: Option<u32>,
    name: Option<String>,
) -> SymbolResolution {
    if let Some(id) = symbol_id {
        match facade.get_symbol(crate::SymbolId(id)) {
            Some(symbol) => SymbolResolution::Resolved {
                symbol,
                identifier: format!("symbol_id:{id}"),
            },
            None => SymbolResolution::NotFoundById(id),
        }
    } else if let Some(name) = name {
        let mut symbols = facade.find_symbols_by_name(&name, None);
        if symbols.is_empty() {
            symbols = find_dotted_members(&name, |n| facade.find_symbols_by_name(n, None));
        }
        match symbols.len() {
            0 => SymbolResolution::NotFoundByName(name),
            1 => SymbolResolution::Resolved {
                symbol: symbols.pop().expect("len checked"),
                identifier: name,
            },
            _ => SymbolResolution::Ambiguous {
                name,
                candidates: symbols,
            },
        }
    } else {
        SymbolResolution::MissingParam
    }
}

/// Outcome of resolving a `find_symbol` query string: `symbol_id:<id>`
/// resolves by direct id lookup, anything else by name with the
/// dotted-member fallback.
pub enum FindSymbolTarget {
    /// Matches, possibly empty. `label` is the queried name, or the
    /// resolved symbol's own name for the `symbol_id:` form.
    Symbols { symbols: Vec<Symbol>, label: String },
    /// `symbol_id:` prefix with a non-numeric id.
    InvalidId(String),
}

/// Resolve a `find_symbol` target. One policy for the MCP handler, the
/// CLI JSON collection, and the CLI text-mode exit plan — the id-lookup
/// arm must not diverge between renderings.
pub fn resolve_find_symbol_target(
    facade: &IndexFacade,
    name: &str,
    lang: Option<&str>,
) -> FindSymbolTarget {
    if let Some(id_str) = name.strip_prefix("symbol_id:") {
        let Ok(id) = id_str.parse::<u32>() else {
            return FindSymbolTarget::InvalidId(id_str.to_string());
        };
        let symbols: Vec<Symbol> = facade.get_symbol(crate::SymbolId(id)).into_iter().collect();
        let label = symbols
            .first()
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| name.to_string());
        FindSymbolTarget::Symbols { symbols, label }
    } else {
        let mut symbols = facade.find_symbols_by_name(name, lang);
        if symbols.is_empty() {
            symbols = find_dotted_members(name, |n| facade.find_symbols_by_name(n, lang));
        }
        FindSymbolTarget::Symbols {
            symbols,
            label: name.to_string(),
        }
    }
}

/// Per-tool argument vocabulary: accepted keys plus the required subset
/// (at least one must be present). One table for the CLI validation
/// surface and the serve-side handler arms. `find_symbol`'s `symbol_id`
/// and `analyze_impact`'s `depth` are CLI/alias sugar: the serve schema
/// carries them as the `name` string form and a serde alias respectively.
pub fn tool_param_spec(tool: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match tool {
        "find_symbol" => (&["name", "symbol_id", "lang"], &["name"]),
        "get_calls" | "find_callers" => (
            &["function_name", "symbol_id"],
            &["function_name", "symbol_id"],
        ),
        "analyze_impact" => (
            &["symbol_name", "symbol_id", "max_depth", "depth"],
            &["symbol_name", "symbol_id"],
        ),
        "get_index_info" => (&[], &[]),
        "search_symbols" => (&["query", "limit", "kind", "module", "lang"], &["query"]),
        "semantic_search_docs" | "semantic_search_with_context" => {
            (&["query", "limit", "threshold", "lang"], &["query"])
        }
        "search_documents" => (&["query", "collection", "limit"], &["query"]),
        _ => (&[], &[]),
    }
}

/// Message for a call missing its required parameter(s); one wording for
/// both transports.
pub fn missing_param_message(tool: &str) -> String {
    let (_, requires_one_of) = tool_param_spec(tool);
    if requires_one_of.len() == 1 {
        format!("{tool} requires '{}' parameter", requires_one_of[0])
    } else {
        format!(
            "{tool} requires either '{}' parameter",
            requires_one_of.join("' or '")
        )
    }
}

/// Trailing "Accepted parameters" line on argument errors, both renderings.
pub fn accepted_params_line(tool: &str) -> String {
    let (accepted, _) = tool_param_spec(tool);
    if accepted.is_empty() {
        format!("{tool} accepts no key:value parameters")
    } else {
        format!("Accepted parameters for {tool}: {}", accepted.join(", "))
    }
}

/// Class-scoped fallback for dotted queries: "Class.method" resolves the
/// method within the named type when no symbol matches the literal name.
/// Uniform across languages; `find` supplies name candidates (typically a
/// `find_symbols_by_name` closure so language filters carry through).
pub fn find_dotted_members(name: &str, find: impl Fn(&str) -> Vec<Symbol>) -> Vec<Symbol> {
    let Some((class, member)) = name.rsplit_once('.') else {
        return Vec::new();
    };
    if class.is_empty() || member.is_empty() {
        return Vec::new();
    }
    find(member)
        .into_iter()
        .filter(|sym| is_member_of(sym, class))
        .collect()
}

/// Whether `sym` is a member of type `class`: ClassMember scope with a
/// matching class name (rightmost segment matches for nested classes), or
/// a member-kind symbol whose module_path ends in the type for languages
/// that record the containing type there (mirrors the
/// `is_receiver_compatible` default). The kind bound keeps the vocabulary
/// at Type.member: without it, module-scoped queries like
/// "components.Button" resolve by accident of the suffix predicate.
fn is_member_of(sym: &Symbol, class: &str) -> bool {
    if let Some(crate::symbol::ScopeContext::ClassMember {
        class_name: Some(c),
    }) = &sym.scope_context
    {
        if c.as_ref() == class || c.rsplit('.').next() == Some(class) {
            return true;
        }
    }
    matches!(
        sym.kind,
        crate::SymbolKind::Method | crate::SymbolKind::Field | crate::SymbolKind::Constant
    ) && sym.module_path.as_deref().is_some_and(|mp| {
        mp.strip_suffix(class)
            .is_some_and(|rest| rest.ends_with("::") || rest.ends_with('.'))
    })
}

/// Text rendering of the ambiguity listing. `tool` appears in the trailing
/// usage hint; output must stay byte-identical across the three handlers.
pub fn render_ambiguity(tool: &str, name: &str, candidates: &[Symbol]) -> String {
    let mut msg = format!(
        "Ambiguous: found {} symbol(s) named '{}':\n",
        candidates.len(),
        name
    );
    for (i, sym) in candidates.iter().take(10).enumerate() {
        msg.push_str(&format!(
            "  {}. symbol_id:{} - {:?} at {}:{}\n",
            i + 1,
            sym.id.value(),
            sym.kind,
            sym.file_path,
            sym.range.start_line + 1
        ));
    }
    if candidates.len() > 10 {
        msg.push_str(&format!("  ... and {} more\n", candidates.len() - 10));
    }
    msg.push_str(&format!("\nUse: {tool} symbol_id:<id> for specific symbol"));
    msg
}

/// Parse the `receiver:{r},static:{s}` relationship context written by the
/// parsers. Returns `None` when the context lacks the pattern or the
/// receiver is empty.
pub fn parse_receiver_context(context: &str) -> Option<(&str, bool)> {
    if !(context.contains("receiver:") && context.contains("static:")) {
        return None;
    }
    let mut receiver = "";
    let mut is_static = false;
    for part in context.split(',') {
        if let Some(r) = part.strip_prefix("receiver:") {
            receiver = r;
        } else if let Some(s) = part.strip_prefix("static:") {
            is_static = s == "true";
        }
    }
    if receiver.is_empty() {
        None
    } else {
        Some((receiver, is_static))
    }
}

/// `Receiver::method` for static calls, `receiver.method` for instance calls.
pub fn qualified_call(receiver: &str, is_static: bool, name: &str) -> String {
    if is_static {
        format!("{receiver}::{name}")
    } else {
        format!("{receiver}.{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_context_parses_both_forms() {
        assert_eq!(
            parse_receiver_context("receiver:Foo,static:true"),
            Some(("Foo", true))
        );
        assert_eq!(
            parse_receiver_context("receiver:bar,static:false"),
            Some(("bar", false))
        );
        assert_eq!(parse_receiver_context("receiver:,static:true"), None);
        assert_eq!(parse_receiver_context("unrelated context"), None);
    }

    #[test]
    fn qualified_call_separator_follows_static_flag() {
        assert_eq!(qualified_call("Foo", true, "new"), "Foo::new");
        assert_eq!(qualified_call("foo", false, "run"), "foo.run");
    }

    fn method_symbol(id: u32, name: &str, class: Option<&str>, module_path: &str) -> Symbol {
        let mut sym = Symbol::new(
            crate::SymbolId::new(id).unwrap(),
            name,
            crate::SymbolKind::Method,
            crate::FileId::new(1).unwrap(),
            crate::Range::new(1, 0, 1, 10),
        );
        sym.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: class.map(Into::into),
        });
        sym.module_path = Some(module_path.into());
        sym
    }

    #[test]
    fn dotted_lookup_filters_by_class_member() {
        let a = method_symbol(1, "model_dump", Some("BaseModel"), "pydantic.main");
        let b = method_symbol(2, "model_dump", Some("RootModel"), "pydantic.root_model");
        let found = find_dotted_members("BaseModel.model_dump", |n| {
            if n == "model_dump" {
                vec![a.clone(), b.clone()]
            } else {
                vec![]
            }
        });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, a.id);
    }

    #[test]
    fn dotted_lookup_matches_module_path_suffix() {
        // Languages recording the containing type via module_path
        let mut sym = method_symbol(1, "new", None, "crate::types::RawSymbol");
        sym.scope_context = None;
        let found = find_dotted_members("RawSymbol.new", |n| {
            if n == "new" {
                vec![sym.clone()]
            } else {
                vec![]
            }
        });
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn dotted_lookup_rejects_module_scoped_symbols() {
        // "components.Button" where Button is a class in module
        // src.components: module-scoped queries are not Type.member
        // vocabulary and stay NOT_FOUND.
        let mut sym = method_symbol(1, "Button", None, "src.components");
        sym.scope_context = None;
        sym.kind = crate::SymbolKind::Class;
        let found = find_dotted_members("components.Button", |n| {
            if n == "Button" {
                vec![sym.clone()]
            } else {
                vec![]
            }
        });
        assert!(found.is_empty());
    }

    #[test]
    fn dotted_lookup_ignores_undotted_and_empty_segments() {
        assert!(find_dotted_members("plain", |_| unreachable!("no dot, no lookup")).is_empty());
        assert!(find_dotted_members(".x", |_| Vec::new()).is_empty());
        assert!(find_dotted_members("x.", |_| Vec::new()).is_empty());
    }

    #[test]
    fn param_vocabulary_messages_are_stable() {
        assert_eq!(
            missing_param_message("get_calls"),
            "get_calls requires either 'function_name' or 'symbol_id' parameter"
        );
        assert_eq!(
            missing_param_message("find_symbol"),
            "find_symbol requires 'name' parameter"
        );
        assert_eq!(
            accepted_params_line("get_calls"),
            "Accepted parameters for get_calls: function_name, symbol_id"
        );
        assert_eq!(
            accepted_params_line("get_index_info"),
            "get_index_info accepts no key:value parameters"
        );
    }

    #[test]
    fn symbol_kind_vocabulary_is_complete() {
        use crate::types::SymbolKind;
        for (input, expected) in [
            ("class", SymbolKind::Class),
            ("enum", SymbolKind::Enum),
            ("interface", SymbolKind::Interface),
            ("variable", SymbolKind::Variable),
            ("typealias", SymbolKind::TypeAlias),
            ("Function", SymbolKind::Function),
        ] {
            assert_eq!(input.parse::<SymbolKind>().unwrap(), expected);
        }
        assert!("widget".parse::<SymbolKind>().is_err());
    }
}
