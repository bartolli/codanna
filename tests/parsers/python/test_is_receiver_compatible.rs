use codanna::parsing::LanguageBehavior;
use codanna::parsing::python::PythonBehavior;
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
fn test_python_default_suffix_uses_module_separator_dot() {
    let behavior = PythonBehavior::new();
    let candidate = make_symbol_with_module_path("process", "examples.python.models.User");
    assert!(
        behavior.is_receiver_compatible(&candidate, "User", None),
        "Python module_path fallback must match via `.User` suffix, not hardcoded `::User`"
    );
}

#[test]
fn test_python_default_suffix_rejects_non_matching_module_path() {
    let behavior = PythonBehavior::new();
    let candidate = make_symbol_with_module_path("process", "examples.python.models.Other");
    assert!(
        !behavior.is_receiver_compatible(&candidate, "User", None),
        "Python module_path fallback must reject when suffix does not match"
    );
}
