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

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    let behavior: Arc<dyn LanguageBehavior> =
        Arc::from(factory.create_behavior_from_registry(js()));
    map.insert(js(), behavior);
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

fn resolve(
    cache: Arc<SymbolLookupCache>,
    rel: UnresolvedRelationship,
) -> (ResolvedBatch, ResolveStats) {
    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let context = ResolutionContext {
        file_id: rel.file_id,
        language_id: js(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(rel.file_id)),
        unresolved_rels: vec![rel],
        variable_bindings: vec![],
    };
    stage.resolve(&context)
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
