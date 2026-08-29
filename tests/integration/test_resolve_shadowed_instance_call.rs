//! A module-level free function whose name equals the called method's must not
//! sink the instance-call edge.
//!
//! The tier in `symbol_cache.resolve()` returns the caller-local same-name
//! candidate ahead of any non-local match. For an instance call that candidate
//! is a free `Function`, never a member of the receiver's type, so
//! `is_instance_type_compatible` rejects it. A rejected tier pick is not
//! evidence that the call has no target: the receiver-typed arm
//! (`resolve_typed_receiver_global`) still owns the evidence, and both the
//! `Found` and `Ambiguous` arms must reach it before failing closed.
//!
//! Witnessed on the self index as the whole language-registration spine — the
//! fifteen `src/parsing/<lang>/definition.rs` `register` functions emitted no
//! edge into `LanguageRegistry::register` because each module defines a free
//! `register` of its own.

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

/// The shadow: a module-level free function sharing the method's name.
fn make_free_function(id: u32, name: &str, file_id: FileId) -> Symbol {
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
    sym
}

fn make_method_on_class(id: u32, name: &str, file_id: FileId, class: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(rust_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.into()),
    });
    sym
}

fn make_caller_with_signature(id: u32, name: &str, file_id: FileId, signature: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Function,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(rust_lang());
    sym.visibility = Visibility::Public;
    sym.signature = Some(signature.into());
    sym
}

fn instance_call_unresolved(
    from_id: u32,
    from_name: &str,
    to_name: &str,
    file_id: FileId,
    receiver: &str,
) -> UnresolvedRelationship {
    let meta = RelationshipMetadata::new()
        .at_position(42, 4)
        .with_receiver(receiver)
        .static_call(false);
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(from_id).unwrap()),
        from_name: from_name.into(),
        to_name: to_name.into(),
        file_id,
        kind: RelationKind::Calls,
        metadata: Some(meta),
        to_range: None,
    }
}

fn context_for(caller_file: FileId, unresolved: Vec<UnresolvedRelationship>) -> ResolutionContext {
    ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: unresolved,
        variable_bindings: vec![],
        this_barrier_spans: vec![],
    }
}

#[test]
fn shadowed_instance_call_resolves_to_receiver_member() {
    // The registration-spine shape: `fn seed(r: &mut Registry) { r.register(1) }`
    // in a module that also defines a free `register`. One free candidate plus
    // one member candidate — the tier returns Found on the caller-local free fn.
    let caller_file = FileId::new(1).unwrap();
    let registry_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        "seed",
        caller_file,
        "fn seed(r: &mut Registry)",
    ));
    // The shadow, same file as the caller.
    cache.insert(make_free_function(2, "register", caller_file));
    let member_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class(
        3,
        "register",
        registry_file,
        "Registry",
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = context_for(
        caller_file,
        vec![instance_call_unresolved(
            1,
            "seed",
            "register",
            caller_file,
            "r",
        )],
    );

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "a same-module free fn must not sink the instance-call edge"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, member_id,
        "the edge must target Registry::register, not the free function"
    );
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn shadowed_ambiguous_instance_call_resolves_to_receiver_member() {
    // Two free `register` candidates make the tier return Ambiguous instead of
    // Found. `disambiguate` has no evidence to pick between two free functions,
    // so this arm must reach the receiver-typed arm too.
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();
    let registry_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        "seed",
        caller_file,
        "fn seed(r: &mut Registry)",
    ));
    cache.insert(make_free_function(2, "register", caller_file));
    cache.insert(make_free_function(3, "register", other_file));
    let member_id = SymbolId::new(4).unwrap();
    cache.insert(make_method_on_class(
        4,
        "register",
        registry_file,
        "Registry",
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = context_for(
        caller_file,
        vec![instance_call_unresolved(
            1,
            "seed",
            "register",
            caller_file,
            "r",
        )],
    );

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "multiple free-fn shadows must not sink the instance-call edge either"
    );
    assert_eq!(
        batch
            .relationships
            .first()
            .expect("one resolved relationship")
            .to_id,
        member_id,
        "the edge must target Registry::register"
    );
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn unshadowed_instance_call_unchanged() {
    // Control: without the shadow the edge already resolved. The fall-through
    // must not change this path.
    let caller_file = FileId::new(1).unwrap();
    let registry_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        "seed",
        caller_file,
        "fn seed(r: &mut Registry)",
    ));
    let member_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(
        2,
        "register",
        registry_file,
        "Registry",
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = context_for(
        caller_file,
        vec![instance_call_unresolved(
            1,
            "seed",
            "register",
            caller_file,
            "r",
        )],
    );

    let (batch, _) = stage.resolve(&context);

    assert_eq!(batch.len(), 1, "unshadowed call must still resolve");
    assert_eq!(
        batch
            .relationships
            .first()
            .expect("one resolved relationship")
            .to_id,
        member_id
    );
}

#[test]
fn shadowed_call_fails_closed_when_receiver_type_uninferrable() {
    // The fall-through carries no license to guess: with a receiver that names
    // no parameter, the receiver-typed arm has nothing to infer from and the
    // row must stay closed rather than land on the free function.
    let caller_file = FileId::new(1).unwrap();
    let registry_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        "seed",
        caller_file,
        "fn seed(r: &mut Registry)",
    ));
    cache.insert(make_free_function(2, "register", caller_file));
    cache.insert(make_method_on_class(
        3,
        "register",
        registry_file,
        "Registry",
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = context_for(
        caller_file,
        vec![instance_call_unresolved(
            1,
            "seed",
            "register",
            caller_file,
            "unknown_var",
        )],
    );

    let (batch, stats) = stage.resolve(&context);

    assert!(
        batch.is_empty(),
        "an uninferrable receiver must not resolve to the free function"
    );
    assert_eq!(stats.resolved, 0);
}
