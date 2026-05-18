use codanna::parsing::LanguageBehavior;
use codanna::parsing::php::{PhpBehavior, PhpInheritanceResolver};
use codanna::parsing::resolution::InheritanceResolver;
use codanna::symbol::ScopeContext;
use codanna::types::Range;
use codanna::{FileId, Symbol, SymbolId, SymbolKind};

fn make_class_member(name: &str, class_name: Option<&str>) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId(1),
        name,
        SymbolKind::Method,
        FileId(1),
        Range::new(0, 0, 0, 0),
    );
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: class_name.map(|c| c.to_string().into()),
    });
    sym
}

#[test]
fn test_parent_expands_to_direct_parent_class() {
    let behavior = PhpBehavior::new();
    let mut resolver = PhpInheritanceResolver::new();
    resolver.add_inheritance("Child".into(), "Base".into(), "extends");

    let caller = make_class_member("go", Some("Child"));
    let expanded = behavior.expand_static_class_keyword("parent", Some(&caller), &resolver);
    assert_eq!(expanded.as_deref(), Some("Base"));
}

#[test]
fn test_static_expands_identically_to_self() {
    let behavior = PhpBehavior::new();
    let resolver = PhpInheritanceResolver::new();
    let caller = make_class_member("bump", Some("Counter"));
    let expanded = behavior.expand_static_class_keyword("static", Some(&caller), &resolver);
    assert_eq!(expanded.as_deref(), Some("Counter"));
}

#[test]
fn test_non_class_caller_yields_none() {
    let behavior = PhpBehavior::new();
    let mut resolver = PhpInheritanceResolver::new();
    resolver.add_inheritance("Child".into(), "Base".into(), "extends");

    let mut caller = Symbol::new(
        SymbolId(2),
        "topLevelFn",
        SymbolKind::Function,
        FileId(1),
        Range::new(0, 0, 0, 0),
    );
    caller.scope_context = None;

    for kw in ["parent", "self", "static"] {
        let expanded = behavior.expand_static_class_keyword(kw, Some(&caller), &resolver);
        assert_eq!(
            expanded, None,
            "keyword {kw} should yield None for non-class caller"
        );
    }
}

#[test]
fn test_parent_with_no_extends_yields_none() {
    let behavior = PhpBehavior::new();
    let resolver = PhpInheritanceResolver::new();
    let caller = make_class_member("go", Some("Lonely"));
    let expanded = behavior.expand_static_class_keyword("parent", Some(&caller), &resolver);
    assert_eq!(expanded, None);
}

#[test]
fn test_php_static_class_keywords() {
    let behavior = PhpBehavior::new();
    assert_eq!(
        behavior.static_class_keywords(),
        &["parent", "self", "static"]
    );
}

#[test]
fn test_self_expands_to_caller_class() {
    let behavior = PhpBehavior::new();
    let resolver = PhpInheritanceResolver::new();
    let caller = make_class_member("bump", Some("Counter"));
    let expanded = behavior.expand_static_class_keyword("self", Some(&caller), &resolver);
    assert_eq!(expanded.as_deref(), Some("Counter"));
}
