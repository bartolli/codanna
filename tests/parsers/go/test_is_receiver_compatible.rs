use codanna::parsing::LanguageBehavior;
use codanna::parsing::go::GoBehavior;
use codanna::types::Range;
use codanna::{FileId, Symbol, SymbolId, SymbolKind};

fn make_symbol_with_module_path(name: &str, module_path: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId(1),
        name,
        SymbolKind::Method,
        FileId(1),
        Range::new(0, 0, 0, 0),
    );
    sym.module_path = Some(module_path.to_string().into());
    sym.scope_context = None;
    sym
}

#[test]
fn test_go_default_suffix_uses_module_separator_slash() {
    let behavior = GoBehavior::new();
    let candidate = make_symbol_with_module_path("Walk", "github.com/foo/bar/Node");
    assert!(
        behavior.is_receiver_compatible(&candidate, "Node", None),
        "Go module_path fallback must match via `/Node` suffix"
    );
}
