//! `LanguageBehavior::expand_static_class_keyword` pre-gate hook in
//! `ResolveStage::resolve_static_call` (story-1 slice 2a).
//!
//! Slice 2a wires the hook with a throwaway `GenericInheritanceResolver`.
//! `self::` / `static::` arms of the PHP override read only the caller's
//! `scope_context.class_name` and never touch the resolver — they ship here.
//! `parent::` requires `InheritanceResolver::parent_of` against a populated
//! resolver; slice 2b pins those scenarios.

use codanna::config::Settings;
use codanna::indexing::pipeline::ResolveStage;
use codanna::indexing::pipeline::types::{
    ResolutionContext, SymbolLookupCache, UnresolvedRelationship,
};
use codanna::parsing::php::PhpInheritanceResolver;
use codanna::parsing::resolution::{GenericResolutionContext, InheritanceResolver};
use codanna::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use codanna::relationship::RelationshipMetadata;
use codanna::symbol::ScopeContext;
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::sync::Arc;

fn php_lang() -> LanguageId {
    LanguageId::new("php")
}

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    let behavior: Arc<dyn LanguageBehavior> =
        Arc::from(factory.create_behavior_from_registry(php_lang()));
    map.insert(php_lang(), behavior);
    map
}

fn make_method_on_class(id: u32, name: &str, file_id: FileId, class: Option<&str>) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(php_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: class.map(Into::into),
    });
    sym
}

fn make_caller_on_class(id: u32, name: &str, file_id: FileId, class: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(php_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.into()),
    });
    sym
}

fn static_call_unresolved(
    from_id: u32,
    to_name: &str,
    file_id: FileId,
    receiver: &str,
) -> UnresolvedRelationship {
    let meta = RelationshipMetadata::new()
        .at_position(42, 4)
        .with_receiver(receiver)
        .static_call(true);
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(from_id).unwrap()),
        from_name: "bump".into(),
        to_name: to_name.into(),
        file_id,
        kind: RelationKind::Calls,
        metadata: Some(meta),
        to_range: None,
    }
}

#[test]
fn self_keyword_resolves_to_caller_class() {
    let file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_on_class(1, "bump", file, "Counter"));
    // Wrong-class first: absent keyword expansion the gate empties the
    // candidate set (class != "self") and resolution returns NotFound.
    cache.insert(make_method_on_class(2, "reset", other_file, Some("Other")));
    let counter_reset_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class(3, "reset", file, Some("Counter")));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: file,
        language_id: php_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(file)),
        unresolved_rels: vec![static_call_unresolved(1, "reset", file, "self")],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("self::reset must resolve to Counter::reset");
    assert_eq!(
        rel.to_id, counter_reset_id,
        "self:: expands to caller class Counter; Counter::reset must win over Other::reset"
    );
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn static_keyword_resolves_identically_to_self() {
    // `static::` ≡ `self::` at index time; PHP late-binding is runtime-only.
    let file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_on_class(1, "bump", file, "Counter"));
    cache.insert(make_method_on_class(2, "reset", other_file, Some("Other")));
    let counter_reset_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class(3, "reset", file, Some("Counter")));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: file,
        language_id: php_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(file)),
        unresolved_rels: vec![static_call_unresolved(1, "reset", file, "static")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("static::reset must resolve to Counter::reset");
    assert_eq!(rel.to_id, counter_reset_id);
}

#[test]
fn parent_keyword_resolves_to_parent_class_method() {
    // Slice 2b: populated `PhpInheritanceResolver` (Child extends Base) wired
    // through `with_inheritance_resolvers` ⇒ `parent::hello()` from Child::go
    // expands to "Base" and resolves to Base::hello, even when Base lives in a
    // different file from the caller.
    let file = FileId::new(1).unwrap();
    let base_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_on_class(1, "go", file, "Child"));
    let base_hello_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(2, "hello", base_file, Some("Base")));

    let mut php_resolver = PhpInheritanceResolver::new();
    php_resolver.add_class_extends("Child".into(), "Base".into());
    let mut resolvers: HashMap<LanguageId, Arc<dyn InheritanceResolver>> = HashMap::new();
    resolvers.insert(php_lang(), Arc::new(php_resolver));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors())
        .with_inheritance_resolvers(resolvers);

    let context = ResolutionContext {
        file_id: file,
        language_id: php_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(file)),
        unresolved_rels: vec![static_call_unresolved(1, "hello", file, "parent")],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("parent::hello with populated resolver must resolve to Base::hello");
    assert_eq!(rel.to_id, base_hello_id);
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn parent_keyword_with_empty_resolver_does_not_resolve() {
    // Empty `GenericInheritanceResolver` ⇒ `parent_of` returns None ⇒
    // expansion is None ⇒ raw "parent" reaches the gate ⇒ no candidate
    // has `class_name == "parent"` ⇒ NotFound. Pins the slice-2a contract.
    let file = FileId::new(1).unwrap();
    let base_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_on_class(1, "go", file, "Child"));
    cache.insert(make_method_on_class(2, "hello", base_file, Some("Base")));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: file,
        language_id: php_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(file)),
        unresolved_rels: vec![static_call_unresolved(1, "hello", file, "parent")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "parent:: without a populated InheritanceResolver must NotFound (slice 2b ships parent::)"
    );
}

#[test]
fn non_keyword_receiver_passes_through_unchanged() {
    // Non-keyword receiver: expansion is None; raw "MyClass" flows to the
    // gate unchanged. Pins no-regression for class-name receivers.
    let file = FileId::new(1).unwrap();
    let target_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_on_class(1, "go", file, "Caller"));
    let target_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(
        2,
        "staticMethod",
        target_file,
        Some("MyClass"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: file,
        language_id: php_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(file)),
        unresolved_rels: vec![static_call_unresolved(1, "staticMethod", file, "MyClass")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("MyClass::staticMethod must resolve");
    assert_eq!(rel.to_id, target_id);
}
