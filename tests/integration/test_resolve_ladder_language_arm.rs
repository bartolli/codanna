//! The priority ladder's same-language arm fails closed on multi-candidate
//! sets; exactly one same-module survivor still resolves.
//!
//! Regression for the ambiguity-ladder first-pick class: a receiver-less
//! call or Extends target whose name matches multiple unimported
//! same-language Public symbols must not resolve to `language_matches[0]`
//! (an arbitrary cross-module copy). Module identity is the one remaining
//! evidence tier: exactly one candidate in the caller's own module wins;
//! zero or many fail closed.

use codanna::config::Settings;
use codanna::indexing::pipeline::types::{
    ResolutionContext, ResolvedBatch, SymbolLookupCache, UnresolvedRelationship,
};
use codanna::indexing::pipeline::{ResolveStage, ResolveStats};
use codanna::parsing::resolution::GenericResolutionContext;
use codanna::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use codanna::symbol::ScopeContext;
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::sync::Arc;

fn js() -> LanguageId {
    LanguageId::new("javascript")
}

fn kotlin() -> LanguageId {
    LanguageId::new("kotlin")
}

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    for lang in [js(), kotlin()] {
        let behavior: Arc<dyn LanguageBehavior> =
            Arc::from(factory.create_behavior_from_registry(lang));
        map.insert(lang, behavior);
    }
    map
}

fn free_fn(id: u32, name: &str, file: u32, module: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Function,
        FileId::new(file).unwrap(),
        Range::new(1, 0, 3, 1),
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym.module_path = Some(module.into());
    sym.scope_context = Some(ScopeContext::Module);
    sym
}

fn class_sym(id: u32, name: &str, file: u32, module: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Class,
        FileId::new(file).unwrap(),
        Range::new(1, 0, 8, 1),
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym.module_path = Some(module.into());
    sym.scope_context = Some(ScopeContext::Module);
    sym
}

fn method_sym(id: u32, name: &str, file: u32, module: &str, class: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        FileId::new(file).unwrap(),
        Range::new(2, 4, 4, 5),
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym.module_path = Some(module.into());
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.to_string().into()),
    });
    sym
}

fn caller(id: u32, kind: SymbolKind, file: u32, module: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        "origin",
        kind,
        FileId::new(file).unwrap(),
        Range::new(10, 0, 16, 1),
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym.module_path = Some(module.into());
    sym
}

fn relation(kind: RelationKind, to_name: &str, caller_file: u32) -> UnresolvedRelationship {
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(1).unwrap()),
        from_name: "origin".into(),
        to_name: to_name.into(),
        file_id: FileId::new(caller_file).unwrap(),
        kind,
        metadata: None,
        to_range: Some(Range::new(12, 1, 12, 20)),
    }
}

fn resolve_with_imports(
    cache: Arc<SymbolLookupCache>,
    rel: UnresolvedRelationship,
    lang: LanguageId,
    imports: Vec<codanna::parsing::Import>,
) -> (ResolvedBatch, ResolveStats) {
    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = ResolutionContext {
        file_id: rel.file_id,
        language_id: lang,
        imports,
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(rel.file_id)),
        unresolved_rels: vec![rel],
        variable_bindings: vec![],
        this_barrier_spans: vec![],
    };
    stage.resolve(&context)
}

fn resolve_in(
    cache: Arc<SymbolLookupCache>,
    rel: UnresolvedRelationship,
    lang: LanguageId,
) -> (ResolvedBatch, ResolveStats) {
    resolve_with_imports(cache, rel, lang, vec![])
}

fn wildcard_import(path: &str, file: u32) -> codanna::parsing::Import {
    codanna::parsing::Import {
        path: path.into(),
        file_id: FileId::new(file).unwrap(),
        alias: None,
        is_glob: true,
        is_type_only: false,
    }
}

fn resolve(
    cache: Arc<SymbolLookupCache>,
    rel: UnresolvedRelationship,
) -> (ResolvedBatch, ResolveStats) {
    resolve_in(cache, rel, js())
}

#[test]
fn cross_module_multi_candidate_call_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    cache.insert(free_fn(2, "helper", 2, "vendor.a"));
    cache.insert(free_fn(3, "helper", 3, "vendor.b"));

    let (batch, stats) = resolve(cache, relation(RelationKind::Calls, "helper", 1));
    assert_eq!(
        batch.len(),
        0,
        "two unimported cross-module candidates must fail closed, not \
         first-pick language_matches[0]"
    );
    assert_eq!(stats.total_processed, 1);
}

#[test]
fn same_module_survivor_resolves() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    cache.insert(free_fn(2, "helper", 2, "vendor.a"));
    cache.insert(free_fn(3, "helper", 3, "app.main.util"));

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "helper", 1));
    assert_eq!(
        batch.len(),
        1,
        "exactly one same-module candidate is evidence"
    );
    assert_eq!(
        batch.relationships[0].to_id,
        SymbolId::new(3).unwrap(),
        "the same-module copy wins over the cross-module one"
    );
}

#[test]
fn cross_module_multi_candidate_extends_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Class, 1, "app.main"));
    cache.insert(class_sym(2, "Logger", 2, "vendor.a"));
    cache.insert(class_sym(3, "Logger", 3, "vendor.b"));

    let (batch, _stats) = resolve(cache, relation(RelationKind::Extends, "Logger", 1));
    assert_eq!(
        batch.len(),
        0,
        "an Extends target with two unimported cross-module candidates \
         must fail closed (the probe class: arbitrary-copy parent)"
    );
}

#[test]
fn same_module_class_member_survivor_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    cache.insert(free_fn(2, "proceed", 2, "vendor.a"));
    cache.insert(method_sym(3, "proceed", 3, "app.main.util", "Wrapper"));

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "proceed", 1));
    assert_eq!(
        batch.len(),
        0,
        "a receiver-less name must not resolve to a same-module class \
         member: the caller holds no instance of the survivor's class \
         (witnessed on ktor: bare proceedWith picked a nested wrapper's \
         method over the true out-of-module receiver type)"
    );
}

#[test]
fn static_class_matched_same_module_member_still_resolves() {
    use codanna::relationship::RelationshipMetadata;

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    cache.insert(method_sym(2, "make", 2, "vendor.a", "Config"));
    cache.insert(method_sym(3, "make", 3, "app.main.util", "Config"));

    let mut rel = relation(RelationKind::Calls, "make", 1);
    rel.metadata = Some(RelationshipMetadata {
        receiver: Some("Config".into()),
        static_call: true,
        ..Default::default()
    });

    let (batch, _stats) = resolve(cache, rel);
    assert_eq!(
        batch.len(),
        1,
        "the static fall-through (Type::name, multiple class-correct \
         copies) keeps its same-module tiebreak: receiver type is class \
         evidence the bare-name gate must not discard"
    );
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(3).unwrap());
}

#[test]
fn instance_receiver_same_module_member_stays_closed() {
    use codanna::relationship::RelationshipMetadata;

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Method, 1, "app.main"));
    cache.insert(method_sym(2, "setSize", 2, "vendor.a", "Renderer"));
    cache.insert(method_sym(3, "setSize", 3, "app.main.util", "Other"));

    let mut rel = relation(RelationKind::Calls, "setSize", 1);
    rel.metadata = Some(RelationshipMetadata {
        receiver: Some("this".into()),
        static_call: false,
        ..Default::default()
    });

    let (batch, _stats) = resolve(cache, rel);
    assert_eq!(
        batch.len(),
        0,
        "a this/instance receiver reaching the ladder never passed class \
         matching; the member gate applies (witnessed: three.js cross-copy \
         this.setSize binds)"
    );
}

#[test]
fn scope_none_method_survivor_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Method, 1, "app.main"));
    cache.insert(free_fn(2, "setup", 2, "vendor.a"));
    let mut member = method_sym(3, "setup", 3, "app.main.util", "Channel");
    member.scope_context = None;
    cache.insert(member);

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "setup", 1));
    assert_eq!(
        batch.len(),
        0,
        "a Method-kind survivor without scope_context is still a member: \
         parsers leave scope None on some members (kotlin suspend \
         members), and those slipped the scope-only gate to wrong \
         cross-file picks (witnessed: ktor private withServerSocket)"
    );
}

#[test]
fn file_scoped_private_member_single_candidate_fails_closed_for_kotlin() {
    use codanna::Visibility;

    let mut c = caller(1, SymbolKind::Method, 1, "io.ktor.client");
    c.language_id = Some(kotlin());
    let mut member = method_sym(
        2,
        "withServerSocket",
        2,
        "io.ktor.client",
        "ConnectionFactoryTest",
    );
    member.language_id = Some(kotlin());
    member.visibility = Visibility::Private;

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(c);
    cache.insert(member);

    let (batch, _stats) = resolve_in(
        cache,
        relation(RelationKind::Calls, "withServerSocket", 1),
        kotlin(),
    );
    assert_eq!(
        batch.len(),
        0,
        "kotlin private members are file-scoped: a unique cross-file \
         Private Method candidate is unreferencable from the caller and \
         must fail closed (witnessed on ktor after package-grained paths)"
    );
}

#[test]
fn private_member_single_candidate_fails_closed_at_found_member_gate() {
    use codanna::Visibility;

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Method, 1, "app.main"));
    let mut member = method_sym(2, "helper", 2, "app.main.util", "Widget");
    member.visibility = Visibility::Private;
    cache.insert(member);

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "helper", 1));
    assert_eq!(
        batch.len(),
        0,
        "a receiver-less cross-file member pick on the same-module rung \
         carries no receiver, import, or inheritance witness: the \
         Found-arm member gate fails closed (module identity plus \
         candidate count is not evidence for a member)"
    );
}

#[test]
fn private_function_single_candidate_resolves_for_module_private_languages() {
    use codanna::Visibility;

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Method, 1, "app.main"));
    let mut target = free_fn(2, "helper", 2, "app.main.util");
    target.visibility = Visibility::Private;
    cache.insert(target);

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "helper", 1));
    assert_eq!(
        batch.len(),
        1,
        "languages without file-scoped privates keep the pre-existing \
         same-module visibility for non-members (rust ancestor-module \
         privates; Private is also the unconfigured Symbol::new \
         default) — neither the kotlin veto nor the member gate covers \
         a Function pick"
    );
}

#[test]
fn same_module_module_scoped_survivor_still_resolves() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    cache.insert(method_sym(2, "helper", 2, "vendor.a", "Other"));
    cache.insert(free_fn(3, "helper", 3, "app.main.util"));

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "helper", 1));
    assert_eq!(
        batch.len(),
        1,
        "the member gate must not touch module-scoped same-module survivors"
    );
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(3).unwrap());
}

#[test]
fn kind_pruned_single_member_survivor_fails_closed() {
    // An Ambiguous set whose kind-compat filter prunes to exactly one
    // member survivor reached the language_matches.len() == 1 early
    // return with no member gate: same-language identity plus candidate
    // count picked a cross-file member (ktor: bare `delay`/`post` calls
    // attached to config members whose runtime referent is external).
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Method, 1, "app.main"));
    let mut field_a = method_sym(2, "delay", 2, "vendor.a", "RetryConfig");
    field_a.kind = SymbolKind::Field;
    let mut field_b = method_sym(3, "delay", 3, "vendor.b", "OtherConfig");
    field_b.kind = SymbolKind::Field;
    cache.insert(field_a);
    cache.insert(field_b);
    cache.insert(method_sym(4, "delay", 4, "vendor.c", "HttpRetry"));

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "delay", 1));
    assert_eq!(
        batch.len(),
        0,
        "a member survivor left alone by kind pruning carries no receiver, \
         import, or inheritance witness - the len==1 language arm must \
         apply the member gate, not resolve on candidate count"
    );
}

#[test]
fn kind_pruned_static_class_matched_member_still_resolves() {
    use codanna::relationship::RelationshipMetadata;

    // Kind pruning leaves one survivor whose class matches the static
    // receiver: class evidence licenses the member pick (served by the
    // pre-bucket receiver filter; the len==1 member gate must not be
    // reachable ahead of it).
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    let mut decoy = method_sym(2, "make", 2, "vendor.a", "Config");
    decoy.kind = SymbolKind::Field;
    cache.insert(decoy);
    cache.insert(method_sym(3, "make", 3, "vendor.b", "Config"));

    let mut rel = relation(RelationKind::Calls, "make", 1);
    rel.metadata = Some(RelationshipMetadata {
        receiver: Some("Config".into()),
        static_call: true,
        ..Default::default()
    });

    let (batch, _stats) = resolve(cache, rel);
    assert_eq!(
        batch.len(),
        1,
        "a class-matched static member pruned to one survivor keeps the \
         static exemption (receiver type is class evidence)"
    );
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(3).unwrap());
}

#[test]
fn kind_pruned_static_class_mismatched_member_fails_closed() {
    use codanna::relationship::RelationshipMetadata;

    // The exemption is class-matched-only: a lone survivor whose class
    // does not match the static receiver dies at the gate.
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    let mut decoy = method_sym(2, "make", 2, "vendor.a", "Config");
    decoy.kind = SymbolKind::Field;
    cache.insert(decoy);
    cache.insert(method_sym(3, "make", 3, "vendor.b", "Unrelated"));

    let mut rel = relation(RelationKind::Calls, "make", 1);
    rel.metadata = Some(RelationshipMetadata {
        receiver: Some("Config".into()),
        static_call: true,
        ..Default::default()
    });

    let (batch, _stats) = resolve(cache, rel);
    assert_eq!(batch.len(), 0);
}

#[test]
fn multi_imported_candidates_fail_closed() {
    // Two wildcard imports admit both same-name candidates into the
    // imported bucket. Tier 2 never fires for wildcards (the `*` segment
    // matches no call name), so the pair arrives Ambiguous, and the
    // imported arm returned imported_matches[0] on multi-match: a
    // first-pick over two import-witnessed copies is not identity.
    let mut c = caller(1, SymbolKind::Function, 1, "app.main");
    c.language_id = Some(kotlin());
    let mut f1 = free_fn(2, "helper", 2, "liba");
    f1.language_id = Some(kotlin());
    let mut f2 = free_fn(3, "helper", 3, "libb");
    f2.language_id = Some(kotlin());

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(c);
    cache.insert(f1);
    cache.insert(f2);

    let imports = vec![wildcard_import("liba.*", 1), wildcard_import("libb.*", 1)];
    let (batch, _stats) = resolve_with_imports(
        cache,
        relation(RelationKind::Calls, "helper", 1),
        kotlin(),
        imports,
    );
    assert_eq!(
        batch.len(),
        0,
        "two import-matched candidates must fail closed, not first-pick \
         imported_matches[0]"
    );
}

#[test]
fn single_imported_candidate_resolves_over_unimported_twin() {
    let mut c = caller(1, SymbolKind::Function, 1, "app.main");
    c.language_id = Some(kotlin());
    let mut f1 = free_fn(2, "helper", 2, "liba");
    f1.language_id = Some(kotlin());
    let mut f2 = free_fn(3, "helper", 3, "libb");
    f2.language_id = Some(kotlin());

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(c);
    cache.insert(f1);
    cache.insert(f2);

    let imports = vec![wildcard_import("liba.*", 1)];
    let (batch, _stats) = resolve_with_imports(
        cache,
        relation(RelationKind::Calls, "helper", 1),
        kotlin(),
        imports,
    );
    assert_eq!(
        batch.len(),
        1,
        "exactly one import-witnessed candidate is identity"
    );
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
}

#[test]
fn unique_cross_module_candidate_still_resolves() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller(1, SymbolKind::Function, 1, "app.main"));
    cache.insert(free_fn(2, "helper", 2, "vendor.a"));

    let (batch, _stats) = resolve(cache, relation(RelationKind::Calls, "helper", 1));
    assert_eq!(
        batch.len(),
        1,
        "a unique candidate is the trusted cross-file recall path; the \
         gate must not touch it"
    );
}
