use codanna::parsing::LanguageBehavior;
use codanna::parsing::php::PhpBehavior;
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
fn test_php_dollar_this_resolves_to_caller_class() {
    let behavior = PhpBehavior::new();
    let candidate = make_class_member("process", Some("Foo"));
    let caller = make_class_member("bar", Some("Foo"));
    assert!(
        behavior.is_receiver_compatible(&candidate, "$this", Some(&caller)),
        "$this should resolve to caller's class_name (Foo) and match candidate's class_name (Foo)"
    );
}

#[test]
fn test_php_dollar_prefix_stripped_before_default_match() {
    let behavior = PhpBehavior::new();
    let candidate = make_class_member("save", Some("user"));
    assert!(
        behavior.is_receiver_compatible(&candidate, "$user", None),
        "$user should strip $ to user and match candidate class_name user"
    );
}

#[test]
fn test_php_dollar_this_with_no_class_caller_returns_false() {
    let behavior = PhpBehavior::new();
    let candidate = make_class_member("process", Some("Foo"));
    let mut caller = Symbol::new(
        SymbolId(2),
        "topLevelFn",
        SymbolKind::Function,
        FileId(1),
        Range::new(0, 0, 0, 0),
    );
    caller.scope_context = None;
    assert!(
        !behavior.is_receiver_compatible(&candidate, "$this", Some(&caller)),
        "$this should not match when caller has no ClassMember scope"
    );
}

#[test]
fn test_php_self_receiver_aliases_is_dollar_this() {
    let behavior = PhpBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["$this"]);
}
