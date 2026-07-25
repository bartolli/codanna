//! Lexical-this resolution boundary: a self-alias-receiver call whose
//! caller carries no ClassMember scope resolves through the innermost
//! this-barrier (non-arrow callable span) containing the call site, or
//! fails closed. Arrows contribute no barrier, so arrow-nested `this.X`
//! reaches the enclosing method; nested `function` callables are their
//! own barrier and reject. Languages whose behavior does not vouch that
//! alias receivers are explicit keep the historical fall-through.

use codanna::config::Settings;
use codanna::indexing::pipeline::types::{
    ResolutionContext, ResolvedBatch, SymbolLookupCache, UnresolvedRelationship,
};
use codanna::indexing::pipeline::{ResolveStage, ResolveStats};
use codanna::parsing::resolution::GenericResolutionContext;
use codanna::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use codanna::relationship::RelationshipMetadata;
use codanna::symbol::ScopeContext;
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::sync::Arc;

fn js() -> LanguageId {
    LanguageId::new("javascript")
}

fn cpp() -> LanguageId {
    LanguageId::new("cpp")
}

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    for lang in [js(), cpp()] {
        let behavior: Arc<dyn LanguageBehavior> =
            Arc::from(factory.create_behavior_from_registry(lang));
        map.insert(lang, behavior);
    }
    map
}

fn method_sym(id: u32, name: &str, range: Range, class: &str, lang: LanguageId) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        FileId::new(1).unwrap(),
        range,
    );
    sym.language_id = Some(lang);
    sym.visibility = Visibility::Public;
    sym.module_path = Some("app.widget".into());
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.into()),
    });
    sym
}

fn nested_fn_sym(id: u32, name: &str, range: Range, lang: LanguageId) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Function,
        FileId::new(1).unwrap(),
        range,
    );
    sym.language_id = Some(lang);
    sym.visibility = Visibility::Public;
    sym.module_path = Some("app.widget".into());
    sym.scope_context = Some(ScopeContext::Local {
        hoisted: false,
        parent_name: None,
        parent_kind: None,
    });
    sym
}

fn this_call(from_id: u32, to_name: &str, call_site: Range) -> UnresolvedRelationship {
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(from_id).unwrap()),
        from_name: "caller".into(),
        to_name: to_name.into(),
        file_id: FileId::new(1).unwrap(),
        kind: RelationKind::Calls,
        metadata: Some(
            RelationshipMetadata::new()
                .at_position(call_site.start_line, call_site.start_column)
                .static_call(false)
                .with_receiver("this"),
        ),
        to_range: Some(call_site),
    }
}

fn resolve_with_barriers(
    cache: Arc<SymbolLookupCache>,
    rel: UnresolvedRelationship,
    barriers: Vec<Range>,
    lang: LanguageId,
) -> (ResolvedBatch, ResolveStats) {
    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = ResolutionContext {
        file_id: rel.file_id,
        language_id: lang,
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(rel.file_id)),
        unresolved_rels: vec![rel],
        variable_bindings: vec![],
        this_barrier_spans: barriers,
    };
    stage.resolve(&context)
}

const METHOD_SPAN: Range = Range {
    start_line: 1,
    start_column: 2,
    end_line: 5,
    end_column: 3,
};

// class Widget { render() { const render = () => this.render(); } }
// Caller is the arrow (Local scope, same name as the method). The
// method's span is the innermost barrier: the callee is the METHOD,
// never the shadowing arrow.
#[test]
fn arrow_this_call_resolves_to_lexical_method() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(method_sym(2, "render", METHOD_SPAN, "Widget", js()));
    cache.insert(nested_fn_sym(3, "render", Range::new(2, 10, 2, 38), js()));

    let (batch, stats) = resolve_with_barriers(
        cache,
        this_call(3, "render", Range::new(2, 25, 2, 38)),
        vec![METHOD_SPAN],
        js(),
    );
    assert_eq!(stats.resolved, 1, "lexical method must resolve");
    let rel = &batch.relationships[0];
    assert_eq!(
        rel.to_id,
        SymbolId::new(2).unwrap(),
        "callee must be the ClassMember method, not the shadowing arrow"
    );
}

// Same shape without barrier evidence: fail closed, never the arrow.
#[test]
fn arrow_this_call_without_barrier_evidence_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(method_sym(2, "render", METHOD_SPAN, "Widget", js()));
    cache.insert(nested_fn_sym(3, "render", Range::new(2, 10, 2, 38), js()));

    let (batch, stats) = resolve_with_barriers(
        cache,
        this_call(3, "render", Range::new(2, 25, 2, 38)),
        vec![],
        js(),
    );
    assert_eq!(
        stats.resolved, 0,
        "no barrier evidence must fail closed: {:?}",
        batch.relationships
    );
}

// method { function inner() { this.render(); } } — the nested function
// declaration is its own innermost barrier; `this` is dynamic there.
#[test]
fn nested_function_declaration_this_call_fails_closed() {
    let inner_span = Range::new(2, 4, 4, 5);
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(method_sym(2, "render", METHOD_SPAN, "Widget", js()));
    cache.insert(nested_fn_sym(3, "inner", inner_span, js()));

    let (batch, stats) = resolve_with_barriers(
        cache,
        this_call(3, "render", Range::new(3, 8, 3, 20)),
        vec![METHOD_SPAN, inner_span],
        js(),
    );
    assert_eq!(
        stats.resolved, 0,
        "innermost barrier is the nested function, not a class member: {:?}",
        batch.relationships
    );
}

// Arrow nested in an arrow nested in the method: arrows contribute no
// barrier, so the innermost barrier is still the method.
#[test]
fn arrow_in_arrow_resolves_through_to_method() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(method_sym(2, "render", METHOD_SPAN, "Widget", js()));
    cache.insert(nested_fn_sym(3, "outer", Range::new(2, 10, 4, 11), js()));
    cache.insert(nested_fn_sym(4, "render", Range::new(3, 12, 3, 44), js()));

    let (batch, stats) = resolve_with_barriers(
        cache,
        this_call(4, "render", Range::new(3, 30, 3, 43)),
        vec![METHOD_SPAN],
        js(),
    );
    assert_eq!(stats.resolved, 1, "arrow chain must reach the method");
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
}

// Module-level arrow: no barrier contains the call site.
#[test]
fn module_level_arrow_this_call_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(method_sym(2, "render", METHOD_SPAN, "Widget", js()));
    cache.insert(nested_fn_sym(3, "render", Range::new(10, 0, 10, 30), js()));

    let (batch, stats) = resolve_with_barriers(
        cache,
        this_call(3, "render", Range::new(10, 15, 10, 29)),
        vec![METHOD_SPAN],
        js(),
    );
    assert_eq!(
        stats.resolved, 0,
        "no containing barrier must fail closed: {:?}",
        batch.relationships
    );
}

// cpp lists `this` in self_receiver_aliases and emits it on bare
// calls inside member functions, but its behavior does not vouch
// alias-implies-explicit: the self-form miss keeps the historical
// fall-through and a same-module free function still resolves.
#[test]
fn non_vouching_language_keeps_fall_through() {
    let cache = Arc::new(SymbolLookupCache::new());
    let mut callee = nested_fn_sym(2, "helper", Range::new(8, 0, 9, 1), cpp());
    callee.scope_context = Some(ScopeContext::Module);
    cache.insert(callee);
    cache.insert(nested_fn_sym(3, "caller_fn", Range::new(2, 4, 4, 5), cpp()));

    let (batch, stats) = resolve_with_barriers(
        cache,
        this_call(3, "helper", Range::new(3, 8, 3, 20)),
        vec![],
        cpp(),
    );
    assert_eq!(
        stats.resolved, 1,
        "non-vouching language must keep the fall-through: {:?}",
        batch.relationships
    );
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
}
