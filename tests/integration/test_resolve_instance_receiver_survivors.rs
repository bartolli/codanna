//! Multi-survivor instance-receiver disambiguation fails closed.
//!
//! Regression for the three.js mis-pick class: when the inferred receiver
//! type matches multiple same-name members (duplicate class copies in the
//! corpus), resolution must not fall through to the name-keyed priority
//! ladder, whose terminal arms are first-pick and proximity — not identity
//! evidence. Exactly one survivor resolves; zero or many fail closed.

use codanna::config::Settings;
use codanna::indexing::pipeline::types::{
    ResolutionContext, ResolvedBatch, SymbolLookupCache, UnresolvedRelationship, VariableBinding,
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

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    let behavior: Arc<dyn LanguageBehavior> =
        Arc::from(factory.create_behavior_from_registry(js()));
    map.insert(js(), behavior);
    map
}

fn method_of_class(id: u32, name: &str, class: &str, file: u32) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        FileId::new(file).unwrap(),
        Range::new(1, 1, 3, 2),
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.into()),
    });
    sym
}

fn caller_fn(id: u32, file: u32) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        "run",
        SymbolKind::Function,
        FileId::new(file).unwrap(),
        Range::new(2, 0, 6, 1),
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym
}

/// `run` holds `v = new Vector3()` (a binding inside its span) and calls
/// `v.set(...)` after the binding site.
fn typed_receiver_call(caller_file: u32) -> (UnresolvedRelationship, Vec<VariableBinding>) {
    let call = UnresolvedRelationship {
        from_id: Some(SymbolId::new(1).unwrap()),
        from_name: "run".into(),
        to_name: "set".into(),
        file_id: FileId::new(caller_file).unwrap(),
        kind: RelationKind::Calls,
        metadata: Some(RelationshipMetadata {
            line: Some(5),
            column: Some(1),
            context: None,
            receiver: Some("v".into()),
            static_call: false,
        }),
        to_range: Some(Range::new(5, 1, 5, 20)),
    };
    let bindings = vec![VariableBinding {
        name: "v".to_string(),
        type_name: "Vector3".to_string(),
        range: Range::new(4, 1, 4, 30),
    }];
    (call, bindings)
}

fn resolve_with_candidates(duplicate_copy: bool) -> (ResolvedBatch, ResolveStats) {
    let caller_file = 1;
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller_fn(1, caller_file));
    // Lowest id: a same-name member of a SIBLING class. The pre-fix
    // fall-through picked this via language_matches[0].
    cache.insert(method_of_class(2, "set", "Vector2", 2));
    cache.insert(method_of_class(3, "set", "Vector3", 3));
    if duplicate_copy {
        // Byte-copy of the receiver's class in another file (bundle copy).
        cache.insert(method_of_class(4, "set", "Vector3", 4));
    }

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let (call, bindings) = typed_receiver_call(caller_file);

    let context = ResolutionContext {
        file_id: FileId::new(caller_file).unwrap(),
        language_id: js(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(
            FileId::new(caller_file).unwrap(),
        )),
        unresolved_rels: vec![call],
        variable_bindings: bindings,
    };
    stage.resolve(&context)
}

#[test]
fn duplicate_class_copies_fail_closed() {
    let (batch, stats) = resolve_with_candidates(true);
    assert_eq!(
        batch.len(),
        0,
        "two Vector3.set copies tie at distance 0: multiple survivors must \
         fail closed, not fall through to the ladder (which would first-pick \
         the lowest-id sibling Vector2.set)"
    );
    assert_eq!(stats.calls_resolved, 0);
    assert_eq!(stats.total_processed, 1);
}

#[test]
fn single_class_copy_resolves_correct_member() {
    let (batch, stats) = resolve_with_candidates(false);
    assert_eq!(
        batch.len(),
        1,
        "exactly one distance-0 survivor must resolve"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id,
        SymbolId::new(3).unwrap(),
        "the survivor is Vector3.set — not the lower-id sibling Vector2.set"
    );
    assert_eq!(stats.calls_resolved, 1);
}
