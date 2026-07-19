//! Multi-survivor instance-receiver disambiguation fails closed;
//! an identity anchor recovers the pick.
//!
//! Regression for the three.js mis-pick class: when the inferred receiver
//! type matches multiple same-name members (duplicate class copies in the
//! corpus), resolution must not fall through to the name-keyed priority
//! ladder, whose terminal arms are first-pick and proximity — not identity
//! evidence. Exactly one survivor resolves; zero or many fail closed —
//! unless the receiver type resolves to a specific Class in the caller's
//! file scope (import binding) or span (function-local class): then the
//! anchored copy's own member wins and the other copies are rejected.

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

fn class_sym(id: u32, name: &str, file: u32, range: Range) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Class,
        FileId::new(file).unwrap(),
        range,
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym
}

fn method_in_class(id: u32, name: &str, class: &str, file: u32, range: Range) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        FileId::new(file).unwrap(),
        range,
    );
    sym.language_id = Some(js());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.into()),
    });
    sym
}

/// Duplicate copies exist, but the caller's file scope binds the type name
/// to one specific Class (an import binding): that copy's member wins.
#[test]
fn import_anchored_duplicate_copies_recover() {
    use codanna::parsing::ResolutionScope;
    use codanna::parsing::resolution::ScopeLevel;

    let caller_file = 1;
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(caller_fn(1, caller_file));
    cache.insert(method_of_class(2, "set", "Vector2", 2));
    cache.insert(method_in_class(
        3,
        "set",
        "Vector3",
        3,
        Range::new(1, 1, 3, 2),
    ));
    cache.insert(method_in_class(
        4,
        "set",
        "Vector3",
        4,
        Range::new(1, 1, 3, 2),
    ));
    cache.insert(class_sym(5, "Vector3", 3, Range::new(0, 0, 4, 1)));
    cache.insert(class_sym(6, "Vector3", 4, Range::new(0, 0, 4, 1)));

    let mut scope = GenericResolutionContext::new(FileId::new(caller_file).unwrap());
    scope.add_symbol(
        "Vector3".to_string(),
        SymbolId::new(5).unwrap(),
        ScopeLevel::Module,
    );

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());
    let (call, bindings) = typed_receiver_call(caller_file);

    let context = ResolutionContext {
        file_id: FileId::new(caller_file).unwrap(),
        language_id: js(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(scope),
        unresolved_rels: vec![call],
        variable_bindings: bindings,
    };
    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "the anchored copy's member must win over the duplicate in file 4"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id,
        SymbolId::new(3).unwrap(),
        "survivor is the member of the Class the scope binding names"
    );
    assert_eq!(stats.calls_resolved, 1);
}

/// Two same-name function-local classes in ONE file (the pydantic
/// validate_call twin-test shape): the class inside the caller's own span
/// anchors, and only its member resolves.
#[test]
fn span_local_class_anchors_same_file_duplicates() {
    let caller_file = 1;
    let cache = Arc::new(SymbolLookupCache::new());
    // Caller spans lines 2..20; its local class X spans 3..8 with foo at
    // 4..6. A sibling test function's X spans 30..38 with foo at 31..33.
    let mut caller = caller_fn(1, caller_file);
    caller.range = Range::new(2, 0, 20, 1);
    cache.insert(caller);
    cache.insert(method_in_class(
        2,
        "foo",
        "X",
        caller_file,
        Range::new(4, 1, 6, 2),
    ));
    cache.insert(method_in_class(
        3,
        "foo",
        "X",
        caller_file,
        Range::new(31, 1, 33, 2),
    ));
    cache.insert(class_sym(5, "X", caller_file, Range::new(3, 0, 8, 1)));
    cache.insert(class_sym(6, "X", caller_file, Range::new(30, 0, 38, 1)));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let call = UnresolvedRelationship {
        from_id: Some(SymbolId::new(1).unwrap()),
        from_name: "run".into(),
        to_name: "foo".into(),
        file_id: FileId::new(caller_file).unwrap(),
        kind: RelationKind::Calls,
        metadata: Some(RelationshipMetadata {
            line: Some(10),
            column: Some(1),
            context: None,
            receiver: Some("x".into()),
            static_call: false,
        }),
        to_range: Some(Range::new(10, 1, 10, 20)),
    };
    let bindings = vec![VariableBinding {
        name: "x".to_string(),
        type_name: "X".to_string(),
        range: Range::new(9, 1, 9, 20),
    }];

    let context = ResolutionContext {
        file_id: FileId::new(caller_file).unwrap(),
        language_id: js(),
        imports: vec![],
        local_symbols: vec![
            SymbolId::new(5).unwrap(),
            SymbolId::new(6).unwrap(),
            SymbolId::new(2).unwrap(),
            SymbolId::new(3).unwrap(),
        ],
        scope: Box::new(GenericResolutionContext::new(
            FileId::new(caller_file).unwrap(),
        )),
        unresolved_rels: vec![call],
        variable_bindings: bindings,
    };
    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "the X inside the caller's span anchors; its foo must resolve"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id,
        SymbolId::new(2).unwrap(),
        "survivor is the caller-span X's foo, not the sibling function's"
    );
    assert_eq!(stats.calls_resolved, 1);
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
