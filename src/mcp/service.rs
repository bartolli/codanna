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
