//! Kind-compatibility filter for relationship resolution.
//!
//! Story 1 slice 1: end-to-end rejection of incompatible-kind resolutions
//! covering both the `context.resolve()` and `cache.resolve(Found)` paths.

use codanna::config::Settings;
use codanna::indexing::pipeline::ResolveStage;
use codanna::indexing::pipeline::types::{
    ResolutionContext, SymbolLookupCache, UnresolvedRelationship,
};
use codanna::parsing::ResolutionScope;
use codanna::parsing::resolution::{GenericResolutionContext, ScopeLevel};
use codanna::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::sync::Arc;

fn rust_lang() -> LanguageId {
    LanguageId::new("rust")
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

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    build_behaviors_for(&[rust_lang()])
}

fn make_symbol_lang(
    id: u32,
    name: &str,
    kind: SymbolKind,
    file_id: FileId,
    lang: LanguageId,
) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        kind,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(lang);
    sym.visibility = Visibility::Public;
    sym
}

fn make_symbol(id: u32, name: &str, kind: SymbolKind, file_id: FileId) -> Symbol {
    make_symbol_lang(id, name, kind, file_id, rust_lang())
}

fn make_unresolved_kind(
    from_id: u32,
    from_name: &str,
    to_name: &str,
    file_id: FileId,
    kind: RelationKind,
) -> UnresolvedRelationship {
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(from_id).unwrap()),
        from_name: from_name.into(),
        to_name: to_name.into(),
        file_id,
        kind,
        metadata: None,
        to_range: None,
    }
}

fn make_unresolved(
    from_id: u32,
    from_name: &str,
    to_name: &str,
    file_id: FileId,
) -> UnresolvedRelationship {
    make_unresolved_kind(from_id, from_name, to_name, file_id, RelationKind::Calls)
}

#[test]
fn calls_to_field_rejected_via_cache_resolve_path() {
    let caller_file = FileId::new(1).unwrap();
    let field_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "caller", SymbolKind::Method, caller_file));
    cache.insert(make_symbol(2, "kind", SymbolKind::Field, field_file));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "Calls(Method, Field) is structurally invalid; resolution must reject the Field candidate"
    );
    assert_eq!(stats.calls_resolved, 0);
    assert_eq!(stats.total_processed, 1);
}

#[test]
fn calls_to_field_rejected_via_context_resolve_path() {
    let caller_file = FileId::new(1).unwrap();
    let field_file = FileId::new(2).unwrap();
    let field_id = SymbolId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "caller", SymbolKind::Method, caller_file));
    cache.insert(make_symbol(2, "kind", SymbolKind::Field, field_file));

    let mut scope = GenericResolutionContext::new(caller_file);
    scope.add_symbol("kind".to_string(), field_id, ScopeLevel::Module);

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(scope),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "Calls(Method, Field) must be rejected on the context.resolve() path as well as cache.resolve()"
    );
    assert_eq!(stats.calls_resolved, 0);
    assert_eq!(stats.total_processed, 1);
}

#[test]
fn python_calls_to_field_rejected() {
    let python = LanguageId::new("python");
    let caller_file = FileId::new(1).unwrap();
    let field_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol_lang(
        1,
        "caller",
        SymbolKind::Method,
        caller_file,
        python,
    ));
    cache.insert(make_symbol_lang(
        2,
        "kind",
        SymbolKind::Field,
        field_file,
        python,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[python]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: python,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "Python: Calls(Method, Field) must be rejected by the language's kind-compatibility table"
    );
    assert_eq!(stats.calls_resolved, 0);
}

#[test]
fn typescript_calls_to_field_rejected() {
    let typescript = LanguageId::new("typescript");
    let caller_file = FileId::new(1).unwrap();
    let field_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol_lang(
        1,
        "caller",
        SymbolKind::Method,
        caller_file,
        typescript,
    ));
    cache.insert(make_symbol_lang(
        2,
        "kind",
        SymbolKind::Field,
        field_file,
        typescript,
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors_for(&[typescript]));

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: typescript,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "TypeScript: Calls(Method, Field) must be rejected — TS override at typescript/resolution.rs:415 excludes Field from Calls callees"
    );
    assert_eq!(stats.calls_resolved, 0);
}

#[test]
fn uses_to_field_rejected_unconditional_filter() {
    let caller_file = FileId::new(1).unwrap();
    let field_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "consume", SymbolKind::Function, caller_file));
    cache.insert(make_symbol(2, "kind", SymbolKind::Field, field_file));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved_kind(
            1,
            "consume",
            "kind",
            caller_file,
            RelationKind::Uses,
        )],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "Uses(Function, Field) must be rejected — Field is not in the default Uses callee set. Verifies kind-filter is unconditional across RelationKind, not Calls-only."
    );
    assert_eq!(stats.total_processed, 1);
    assert_eq!(
        stats.calls_resolved, 0,
        "non-Calls relationship; this counter must stay at 0"
    );
}

#[test]
fn function_to_function_calls_passthrough_preserved() {
    let caller_file = FileId::new(1).unwrap();
    let target_file = FileId::new(2).unwrap();
    let target_id = SymbolId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "main", SymbolKind::Function, caller_file));
    cache.insert(make_symbol(2, "helper", SymbolKind::Function, target_file));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "main", "helper", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "Calls(Function, Function) is structurally valid; filter must not over-reject"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(rel.to_id, target_id);
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn ambiguous_all_fields_filtered_to_empty_returns_none() {
    let caller_file = FileId::new(1).unwrap();
    let field1_file = FileId::new(2).unwrap();
    let field2_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "caller", SymbolKind::Method, caller_file));
    cache.insert(make_symbol(2, "kind", SymbolKind::Field, field1_file));
    cache.insert(make_symbol(3, "kind", SymbolKind::Field, field2_file));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "all Ambiguous candidates fail kind-compatibility; survivor set empty; resolution must return None"
    );
    assert_eq!(stats.calls_resolved, 0);
}

#[test]
fn ambiguous_mixed_kind_candidates_filter_to_method_survivor() {
    let caller_file = FileId::new(1).unwrap();
    let field1_file = FileId::new(2).unwrap();
    let field2_file = FileId::new(3).unwrap();
    let method_file = FileId::new(4).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "caller", SymbolKind::Method, caller_file));
    cache.insert(make_symbol(2, "kind", SymbolKind::Field, field1_file));
    cache.insert(make_symbol(3, "kind", SymbolKind::Field, field2_file));
    let method_id = SymbolId::new(4).unwrap();
    cache.insert(make_symbol(4, "kind", SymbolKind::Method, method_file));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "exactly one survivor after kind-filter on Ambiguous candidates"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, method_id,
        "kind-filter must select the Method, rejecting both Fields"
    );
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn calls_to_method_passthrough_preserved_via_cache_resolve_path() {
    let caller_file = FileId::new(1).unwrap();
    let target_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_symbol(1, "caller", SymbolKind::Method, caller_file));
    cache.insert(make_symbol(2, "kind", SymbolKind::Method, target_file));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![make_unresolved(1, "caller", "kind", caller_file)],
        variable_bindings: vec![],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "Calls(Method, Method) is structurally valid; resolution must preserve the edge"
    );
    assert_eq!(stats.calls_resolved, 1);
    assert_eq!(stats.total_processed, 1);
}
