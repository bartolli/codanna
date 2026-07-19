//! Instance-call disambiguation filter via parameter-type inference (story 4 slice 5).
//!
//! When an `UnresolvedRelationship` carries `metadata.static_call = false` and
//! `metadata.receiver = Some("var_name")`, `disambiguate()` must read the
//! receiver's declared type directly from the **caller's** signature via
//! `LanguageBehavior::extract_parameter_type` and filter ambiguous candidates by
//! `is_receiver_compatible` against that inferred type.
//!
//! Bypasses per-parser `SymbolKind::Parameter` emission (today only C/C++/Go/Lua
//! emit Parameter symbols). Caller-signature is universally available on
//! Function/Method symbols.

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
    build_behaviors_for(&[rust_lang()])
}

fn build_behaviors_for(langs: &[LanguageId]) -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    for lang in langs {
        let behavior: Arc<dyn LanguageBehavior> =
            Arc::from(factory.create_behavior_from_registry(*lang));
        map.insert(*lang, behavior);
    }
    map
}

fn make_method_on_class_lang(
    id: u32,
    name: &str,
    file_id: FileId,
    class: Option<&str>,
    lang: LanguageId,
) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(lang);
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: class.map(Into::into),
    });
    sym
}

fn make_caller_with_signature_lang(
    id: u32,
    file_id: FileId,
    signature: &str,
    lang: LanguageId,
) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        "caller",
        SymbolKind::Function,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(lang);
    sym.visibility = Visibility::Public;
    sym.signature = Some(signature.into());
    sym
}

fn make_method_on_class(id: u32, name: &str, file_id: FileId, class: Option<&str>) -> Symbol {
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
        class_name: class.map(Into::into),
    });
    sym
}

fn make_caller_with_signature(id: u32, file_id: FileId, signature: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        "caller",
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
        from_name: "caller".into(),
        to_name: to_name.into(),
        file_id,
        kind: RelationKind::Calls,
        metadata: Some(meta),
        to_range: None,
    }
}

#[test]
fn instance_call_filters_ambiguous_by_inferred_parameter_type() {
    let caller_file = FileId::new(1).unwrap();
    let other_store_file = FileId::new(2).unwrap();
    let document_store_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn caller(store: &DocumentStore)",
    ));

    // Wrong-class candidate inserted first so the location-priority fallback
    // would pick id=2 absent the inference filter.
    cache.insert(make_method_on_class(
        2,
        "process",
        other_store_file,
        Some("OtherStore"),
    ));
    let document_store_method_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class(
        3,
        "process",
        document_store_file,
        Some("DocumentStore"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "parameter-type filter must collapse Ambiguous candidates to the inferred type's class"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, document_store_method_id,
        "DocumentStore::process must be selected over OtherStore::process via inferred parameter type"
    );
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn instance_call_fails_closed_when_receiver_not_a_parameter() {
    // When `receiver` doesn't name a parameter on the caller's signature
    // (e.g., a non-parameter local), the receiver's type is unknown and
    // resolution fails closed: any same-name pick would be a guess that
    // attaches std/foreign-receiver calls to arbitrary user methods.
    let caller_file = FileId::new(1).unwrap();
    let other_store_file = FileId::new(2).unwrap();
    let document_store_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn caller(store: &DocumentStore)",
    ));

    cache.insert(make_method_on_class(
        2,
        "process",
        other_store_file,
        Some("OtherStore"),
    ));
    cache.insert(make_method_on_class(
        3,
        "process",
        document_store_file,
        Some("DocumentStore"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(
            1,
            "process",
            caller_file,
            "unknown_var",
        )],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert!(
        batch.is_empty(),
        "unknown-type receiver must not resolve to any same-name candidate"
    );
    assert_eq!(stats.resolved, 0);
}

#[test]
fn instance_call_single_candidate_type_match_resolves() {
    // Slice 5b symmetric positive: when the only candidate's containing class
    // matches the inferred type, the Found arm must resolve (not over-reject).
    // Guards against accidental `!` inversion in `is_instance_type_compatible`.
    let caller_file = FileId::new(1).unwrap();
    let store_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn caller(store: &DocumentStore)",
    ));
    let process_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(
        2,
        "process",
        store_file,
        Some("DocumentStore"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("single matching candidate must resolve via Found arm");
    assert_eq!(
        rel.to_id, process_id,
        "single candidate whose class matches inferred type must resolve, not be over-rejected"
    );
}

#[test]
fn instance_call_real_world_signature_with_attrs_pub_async_lifetimes() {
    // Production-shape signature: pub, generics on the fn, lifetimes, multi-line,
    // attribute. Verifies extract_parameter_type survives realistic input
    // (`extract_signature` emits the verbatim source slice up to the body).
    let caller_file = FileId::new(1).unwrap();
    let store_file = FileId::new(2).unwrap();

    let real_world_sig = "#[tracing::instrument(skip(self))]\n    pub async fn handle<'a>(\n        &'a mut self,\n        ctx: &'a Context,\n        store: &mut DocumentStore,\n    ) -> Result<()>";

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(1, caller_file, real_world_sig));
    let process_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(
        2,
        "process",
        store_file,
        Some("DocumentStore"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("real-world signature must still parse + resolve");
    assert_eq!(
        rel.to_id, process_id,
        "pub/async/lifetimes/attrs/multi-line caller signature must not break parameter-type extraction"
    );
}

#[test]
fn instance_call_single_candidate_type_mismatch_resolves_notfound() {
    // Slice 5b: the Found arm of `resolve_one` (single-candidate cache lookup)
    // must also gate by inferred type. Pre-slice-5b, `node.walk()` resolved to
    // the only `walk` method in the index (`FileWalker::walk`) even when the
    // caller's `node` parameter has type `tree_sitter::Node` — because slice 5's
    // filter only fired on Ambiguous results.
    let caller_file = FileId::new(1).unwrap();
    let walker_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn extract(&self, node: &tree_sitter::Node)",
    ));
    // Single `walk` candidate on a wrong-class (FileWalker, not Node).
    cache.insert(make_method_on_class(
        2,
        "walk",
        walker_file,
        Some("FileWalker"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "walk", caller_file, "node")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "single-candidate cache.Found must NOT resolve when the inferred receiver type mismatches the candidate's containing class"
    );
}

#[test]
fn slice_4_6_regression_walk_on_external_node_resolves_notfound() {
    // Slice 4.6 regression: codifies the canonical `extract_calls_recursive`
    // false-positive baseline. Caller signature carries `node: &tree_sitter::Node`
    // (external); single `walk` candidate exists on `FileWalker` (wrong class).
    // Pre-slice-5b this resolved to FileWalker::walk via Found arm; post-slice-5b
    // resolves to NotFound. Pins the `walk` half of the story-4 acceptance
    // criterion (verification block in story body).
    let caller_file = FileId::new(1).unwrap();
    let walker_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn extract_calls_recursive(&self, node: &tree_sitter::Node, code: &str)",
    ));
    cache.insert(make_method_on_class(
        2,
        "walk",
        walker_file,
        Some("FileWalker"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "walk", caller_file, "node")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    assert_eq!(
        batch.len(),
        0,
        "node.walk() with node: &tree_sitter::Node must NOT resolve to FileWalker::walk"
    );
}

#[test]
fn slice_4_6_regression_push_on_external_vec_resolves_notfound() {
    // Slice 4.6 regression: codifies the `push` half. Caller signature carries
    // `calls: &mut Vec<MethodCall>` (external Vec); single `push` candidate
    // exists on `ResolvedBatch` (wrong class). The `Vec<MethodCall>` type
    // reduces to `Vec` via `generic_type → type_identifier` descent (slice 4.2),
    // and `Vec` matches no indexed class → NotFound.
    let caller_file = FileId::new(1).unwrap();
    let batch_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn collect(&self, calls: &mut Vec<MethodCall>)",
    ));
    cache.insert(make_method_on_class(
        2,
        "push",
        batch_file,
        Some("ResolvedBatch"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "push", caller_file, "calls")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    assert_eq!(
        batch.len(),
        0,
        "calls.push(...) with calls: &mut Vec<MethodCall> must NOT resolve to ResolvedBatch::push"
    );
}

#[test]
fn instance_call_external_type_resolves_notfound() {
    // Scenario 1 of [[story-4-parameter-type-inference]]: when the inferred
    // type names an external (unindexed) class, no candidate survives. The
    // resolver MUST return NotFound rather than fall through to a same-name
    // method on an unrelated class. Spec parity with story 2 slice 6's
    // correction that "zero-match returns NotFound, supersedes slice-4's
    // fall-through".
    let caller_file = FileId::new(1).unwrap();
    let symbol_file_a = FileId::new(2).unwrap();
    let symbol_file_b = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    // Caller's parameter is typed as an external/unindexed class `Node`.
    cache.insert(make_caller_with_signature(
        1,
        caller_file,
        "fn caller(node: &Node)",
    ));

    // Two candidates named `kind` on unrelated classes; neither is `Node`.
    cache.insert(make_method_on_class(
        2,
        "kind",
        symbol_file_a,
        Some("RawSymbol"),
    ));
    cache.insert(make_method_on_class(
        3,
        "kind",
        symbol_file_b,
        Some("Symbol"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "kind", caller_file, "node")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "external-type inference with no matching candidate must yield NotFound, not a wrong-class fall-through pick"
    );
}

#[test]
fn python_instance_call_ambiguous_filters_to_inferred_type_class() {
    let lang = LanguageId::new("python");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();
    let store_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "(self, store: DocumentStore) -> None",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "process",
        other_file,
        Some("OtherStore"),
        lang,
    ));
    let store_method_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class_lang(
        3,
        "process",
        store_file,
        Some("DocumentStore"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    let rel = batch.relationships.first().expect("one resolved");
    assert_eq!(
        rel.to_id, store_method_id,
        "Python Ambiguous-arm filter must select DocumentStore.process over OtherStore.process via parameter type"
    );
}

#[test]
fn python_instance_call_wrong_class_single_candidate_collapses_notfound() {
    let lang = LanguageId::new("python");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "(self, store: DocumentStore) -> None",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "process",
        other_file,
        Some("OtherStore"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    assert_eq!(
        batch.len(),
        0,
        "Python store.process() with store: DocumentStore must NOT resolve to OtherStore.process"
    );
}

#[test]
fn typescript_instance_call_ambiguous_filters_to_inferred_type_class() {
    let lang = LanguageId::new("typescript");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();
    let store_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "caller(store: DocumentStore): void",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "process",
        other_file,
        Some("OtherStore"),
        lang,
    ));
    let store_method_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class_lang(
        3,
        "process",
        store_file,
        Some("DocumentStore"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    let rel = batch.relationships.first().expect("one resolved");
    assert_eq!(rel.to_id, store_method_id);
}

#[test]
fn typescript_instance_call_wrong_class_single_candidate_collapses_notfound() {
    let lang = LanguageId::new("typescript");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "async fetch(url: Url): Promise<void>",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "encode",
        other_file,
        Some("Buffer"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "encode", caller_file, "url")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    assert_eq!(batch.len(), 0);
}

#[test]
fn go_instance_call_ambiguous_filters_to_inferred_type_class() {
    let lang = LanguageId::new("go");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();
    let store_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "func Caller(store *DocumentStore)",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "Process",
        other_file,
        Some("OtherStore"),
        lang,
    ));
    let store_method_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class_lang(
        3,
        "Process",
        store_file,
        Some("DocumentStore"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "Process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    let rel = batch.relationships.first().expect("one resolved");
    assert_eq!(rel.to_id, store_method_id);
}

#[test]
fn go_instance_call_wrong_class_single_candidate_collapses_notfound() {
    let lang = LanguageId::new("go");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "func (r *Receiver) Bar(node *tree_sitter.Node) error",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "Walk",
        other_file,
        Some("FileWalker"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "Walk", caller_file, "node")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    assert_eq!(batch.len(), 0);
}

#[test]
fn java_instance_call_ambiguous_filters_to_inferred_type_class() {
    let lang = LanguageId::new("java");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();
    let store_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "public void caller(DocumentStore store)",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "process",
        other_file,
        Some("OtherStore"),
        lang,
    ));
    let store_method_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class_lang(
        3,
        "process",
        store_file,
        Some("DocumentStore"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "process", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    let rel = batch.relationships.first().expect("one resolved");
    assert_eq!(rel.to_id, store_method_id);
}

#[test]
fn java_instance_call_wrong_class_single_candidate_collapses_notfound() {
    let lang = LanguageId::new("java");
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller_with_signature_lang(
        1,
        caller_file,
        "String bar(final DocumentStore store)",
        lang,
    ));
    cache.insert(make_method_on_class_lang(
        2,
        "save",
        other_file,
        Some("FileWriter"),
        lang,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[lang]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![instance_call_unresolved(1, "save", caller_file, "store")],
        variable_bindings: vec![],
    };

    let (batch, _stats) = stage.resolve(&context);
    assert_eq!(batch.len(), 0);
}
