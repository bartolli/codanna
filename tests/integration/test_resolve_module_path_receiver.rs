//! Anchored module-path receivers on the static-call path.
//!
//! A rust `qualifier::func()` call survives twin absorption as a method-call
//! record with `receiver = qualifier`, `static_call = true`. When the
//! qualifier is an anchored module path (`crate::a::b`, `super::x`, `self::m`,
//! or bare `super`/`crate`/`self`), `is_receiver_compatible` resolves it
//! against the caller's module identity and accepts only candidates whose
//! `module_path` matches exactly. Unresolvable anchors fail closed.

use codanna::config::Settings;
use codanna::indexing::pipeline::ResolveStage;
use codanna::indexing::pipeline::types::{
    ResolutionContext, SymbolLookupCache, UnresolvedRelationship,
};
use codanna::parsing::resolution::GenericResolutionContext;
use codanna::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use codanna::relationship::RelationshipMetadata;
use codanna::symbol::ScopeContext;
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::sync::Arc;

fn rust_lang() -> LanguageId {
    LanguageId::new("rust")
}

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    let behavior: Arc<dyn LanguageBehavior> =
        Arc::from(factory.create_behavior_from_registry(rust_lang()));
    map.insert(rust_lang(), behavior);
    map
}

fn make_module_fn(id: u32, name: &str, file_id: FileId, module: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Function,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(rust_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::Module);
    sym.module_path = Some(module.into());
    sym
}

fn make_caller(id: u32, file_id: FileId, module: Option<&str>) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        "caller",
        SymbolKind::Function,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(rust_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::Module);
    sym.module_path = module.map(Into::into);
    sym
}

fn qualified_call(
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
        from_name: "caller".into(),
        to_name: to_name.into(),
        file_id,
        kind: RelationKind::Calls,
        metadata: Some(meta),
        to_range: None,
    }
}

fn resolve_one(
    cache: Arc<SymbolLookupCache>,
    caller_file: FileId,
    unresolved: UnresolvedRelationship,
) -> Vec<codanna::indexing::pipeline::types::ResolvedRelationship> {
    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![unresolved],
        variable_bindings: vec![],
        this_barrier_spans: vec![],
    };
    let (batch, _stats) = stage.resolve(&context);
    batch.relationships
}

/// Two same-name candidates in sibling modules; `super::rust_lang::register`
/// from a caller in `crate::parsing::registry` must pick exactly the
/// `crate::parsing::rust_lang` one.
#[test]
fn super_anchored_receiver_picks_exact_module_among_twins() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(
        1,
        caller_file,
        Some("crate::parsing::registry"),
    ));
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::python_lang",
    ));
    let want = SymbolId::new(3).unwrap();
    cache.insert(make_module_fn(
        3,
        "register",
        FileId::new(3).unwrap(),
        "crate::parsing::rust_lang",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "super::rust_lang"),
    );
    assert_eq!(
        rels.len(),
        1,
        "super-anchored module receiver must resolve exactly one edge"
    );
    assert_eq!(
        rels[0].to_id, want,
        "must pick the register in crate::parsing::rust_lang"
    );
}

/// `crate::`-anchored path resolves independent of the caller's own module.
#[test]
fn crate_anchored_receiver_resolves() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(
        1,
        caller_file,
        Some("crate::parsing::registry"),
    ));
    let want = SymbolId::new(2).unwrap();
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::rust_lang",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "crate::parsing::rust_lang"),
    );
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].to_id, want);
}

/// A method caller (impl block) carries the same containing-module identity;
/// `super::stages::preflight` from it resolves the unique target.
#[test]
fn super_anchored_receiver_from_method_caller_resolves() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    let mut caller = make_caller(
        1,
        caller_file,
        Some("crate::indexing::pipeline::incremental"),
    );
    caller.kind = SymbolKind::Method;
    caller.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some("Pipe".into()),
    });
    cache.insert(caller);
    let want = SymbolId::new(2).unwrap();
    cache.insert(make_module_fn(
        2,
        "preflight",
        FileId::new(2).unwrap(),
        "crate::indexing::pipeline::stages",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "preflight", caller_file, "super::stages"),
    );
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].to_id, want);
}

/// An anchor that resolves to a module no candidate lives in fails closed.
#[test]
fn anchored_receiver_with_no_module_match_fails_closed() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(
        1,
        caller_file,
        Some("crate::parsing::registry"),
    ));
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::rust_lang",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "super::nosuch"),
    );
    assert!(
        rels.is_empty(),
        "no module match must fail closed, got {rels:?}"
    );
}

/// A caller without module identity cannot anchor `super`; fail closed.
#[test]
fn super_anchor_without_caller_module_fails_closed() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file, None));
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::rust_lang",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "super::rust_lang"),
    );
    assert!(rels.is_empty(), "unresolvable anchor must fail closed");
}

/// `super` above the crate root is unresolvable; fail closed.
#[test]
fn super_above_crate_root_fails_closed() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file, Some("crate")));
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::rust_lang",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "super::rust_lang"),
    );
    assert!(rels.is_empty(), "super above crate root must fail closed");
}

/// `self::`-anchored path is the caller's own module plus the tail.
#[test]
fn self_anchored_receiver_resolves() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file, Some("crate::parsing")));
    let want = SymbolId::new(2).unwrap();
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::rust_lang",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "self::rust_lang"),
    );
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].to_id, want);
}

/// The six-file language layout defines `register` one level below the module
/// the call names (`crate::parsing::rust::definition` behind a re-export at
/// `crate::parsing::rust`); the anchored receiver accepts descendants of the
/// resolved module.
#[test]
fn super_anchored_receiver_matches_descendant_module() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(
        1,
        caller_file,
        Some("crate::parsing::registry"),
    ));
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::python::definition",
    ));
    let want = SymbolId::new(3).unwrap();
    cache.insert(make_module_fn(
        3,
        "register",
        FileId::new(3).unwrap(),
        "crate::parsing::rust::definition",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "super::rust"),
    );
    assert_eq!(
        rels.len(),
        1,
        "descendant of the anchored module must resolve"
    );
    assert_eq!(rels[0].to_id, want);
}

/// Descendant matching is segment-aware: `crate::parsing::rustics` is NOT a
/// descendant of `crate::parsing::rust`.
#[test]
fn anchored_receiver_rejects_sibling_name_prefix() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(
        1,
        caller_file,
        Some("crate::parsing::registry"),
    ));
    cache.insert(make_module_fn(
        2,
        "register",
        FileId::new(2).unwrap(),
        "crate::parsing::rustics",
    ));

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "register", caller_file, "super::rust"),
    );
    assert!(
        rels.is_empty(),
        "a sibling module sharing the name prefix must fail closed, got {rels:?}"
    );
}

/// The existing type-receiver semantics are untouched: an uppercase type
/// receiver still matches through class_name, not module identity.
#[test]
fn type_receiver_arm_unchanged() {
    let caller_file = FileId::new(1).unwrap();
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(
        1,
        caller_file,
        Some("crate::parsing::registry"),
    ));
    let mut method = Symbol::new(
        SymbolId::new(2).unwrap(),
        "new",
        SymbolKind::Method,
        FileId::new(2).unwrap(),
        Range::new(2, 0, 3, 0),
    );
    method.language_id = Some(rust_lang());
    method.visibility = Visibility::Public;
    method.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some("RawSymbol".into()),
    });
    cache.insert(method);

    let rels = resolve_one(
        cache,
        caller_file,
        qualified_call(1, "new", caller_file, "RawSymbol"),
    );
    assert_eq!(rels.len(), 1, "type receiver must keep resolving");
}
