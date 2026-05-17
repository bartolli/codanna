//! Static-call disambiguation filter (story 2 slice 4).
//!
//! When an `UnresolvedRelationship` carries `metadata.static_call = true` and
//! `metadata.receiver = Some("Type")`, `disambiguate()` must filter ambiguous
//! candidates to those whose `scope_context.class_name == "Type"` (with
//! `module_path.ends_with("::Type")` as fallback). Single survivor wins
//! without consulting the location-priority fallback. Empty or multiple
//! survivors fall through to existing priority logic.

use codanna::config::Settings;
use codanna::indexing::pipeline::ResolveStage;
use codanna::indexing::pipeline::types::{
    ResolutionContext, SymbolLookupCache, UnresolvedRelationship,
};
use codanna::parsing::ResolutionScope;
use codanna::parsing::resolution::{GenericResolutionContext, ScopeLevel};
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

fn make_caller(id: u32, file_id: FileId) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        "caller",
        SymbolKind::Method,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(rust_lang());
    sym.visibility = Visibility::Public;
    sym
}

fn static_call_unresolved(
    from_id: u32,
    to_name: &str,
    file_id: FileId,
    receiver: &str,
    static_call: bool,
) -> UnresolvedRelationship {
    let meta = RelationshipMetadata::new()
        .at_position(42, 4)
        .with_receiver(receiver)
        .static_call(static_call);
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
fn static_call_filters_ambiguous_by_class_name() {
    let caller_file = FileId::new(1).unwrap();
    let symbol_file = FileId::new(2).unwrap();
    let raw_symbol_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file));
    // Insert the wrong-class candidate first so the existing language-priority
    // fallback would return id=2 (Symbol::new). The static-call filter must
    // override that to select id=3 (RawSymbol::new).
    cache.insert(make_method_on_class(2, "new", symbol_file, Some("Symbol")));
    let raw_symbol_method_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class(
        3,
        "new",
        raw_symbol_file,
        Some("RawSymbol"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![static_call_unresolved(
            1,
            "new",
            caller_file,
            "RawSymbol",
            true,
        )],
    };

    let (batch, stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        1,
        "static-call filter must collapse Ambiguous candidates to the receiver's class"
    );
    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, raw_symbol_method_id,
        "RawSymbol::new must be selected over Symbol::new"
    );
    assert_eq!(stats.calls_resolved, 1);
}

#[test]
fn instance_call_skips_static_filter() {
    let caller_file = FileId::new(1).unwrap();
    let symbol_file = FileId::new(2).unwrap();
    let raw_symbol_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file));
    let symbol_method_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(2, "new", symbol_file, Some("Symbol")));
    cache.insert(make_method_on_class(
        3,
        "new",
        raw_symbol_file,
        Some("RawSymbol"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![static_call_unresolved(
            1,
            "new",
            caller_file,
            "RawSymbol",
            false, // instance call -> static-call filter must be skipped
        )],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, symbol_method_id,
        "instance call must bypass the static-call filter and fall through to priority logic (insertion-order pick)"
    );
}

#[test]
fn static_call_no_match_returns_notfound() {
    // When a qualified static call's receiver matches no candidate's class,
    // the correct result is NotFound — not "fall through and pick a same-name
    // method on an unrelated type." Aligns with plan-out-of-scope:
    // "External-crate method resolution. Correct result is `NotFound`."
    let caller_file = FileId::new(1).unwrap();
    let symbol_file = FileId::new(2).unwrap();
    let raw_symbol_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file));
    cache.insert(make_method_on_class(2, "new", symbol_file, Some("Symbol")));
    cache.insert(make_method_on_class(
        3,
        "new",
        raw_symbol_file,
        Some("RawSymbol"),
    ));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![static_call_unresolved(
            1,
            "new",
            caller_file,
            "NoSuchType",
            true,
        )],
    };

    let (batch, _stats) = stage.resolve(&context);

    assert_eq!(
        batch.len(),
        0,
        "qualified static call with no matching receiver-type must NotFound, not pick a same-name unrelated method"
    );
}

#[test]
fn static_call_matches_via_module_path_fallback() {
    let caller_file = FileId::new(1).unwrap();
    let other_file = FileId::new(2).unwrap();
    let target_file = FileId::new(3).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file));

    // Wrong candidate: class_name set but unrelated; no module_path suffix match.
    let mut other = make_method_on_class(2, "new", other_file, Some("Other"));
    other.module_path = Some("pkg::Other".into());
    cache.insert(other);

    // Target candidate: class_name absent, but module_path ends with "::Target".
    let target_id = SymbolId::new(3).unwrap();
    let mut target = make_method_on_class(3, "new", target_file, None);
    target.module_path = Some("pkg::Target".into());
    cache.insert(target);

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![static_call_unresolved(
            1,
            "new",
            caller_file,
            "Target",
            true,
        )],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, target_id,
        "module_path fallback must select the candidate whose path ends with `::Target`"
    );
}

#[test]
fn static_call_overrides_found_in_file_scope() {
    // A same-name `new` is in the caller's file scope (same-file local impl).
    // Without a receiver-compat gate on the `Found` arm, the resolver returns
    // the local candidate immediately and never consults `disambiguate()`.
    // With the gate, the class-mismatch rejects the local Found result; the
    // pipeline falls through to `cache.resolve` and the static-call filter
    // selects the receiver's `new`.
    let caller_file = FileId::new(1).unwrap();
    let raw_symbol_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, caller_file));
    let local_new_id = SymbolId::new(2).unwrap();
    cache.insert(make_method_on_class(
        2,
        "new",
        caller_file,
        Some("ParseStage"),
    ));
    let raw_symbol_new_id = SymbolId::new(3).unwrap();
    cache.insert(make_method_on_class(
        3,
        "new",
        raw_symbol_file,
        Some("RawSymbol"),
    ));

    let mut scope = GenericResolutionContext::new(caller_file);
    scope.add_symbol("new".to_string(), local_new_id, ScopeLevel::Module);

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: rust_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(scope),
        unresolved_rels: vec![static_call_unresolved(
            1,
            "new",
            caller_file,
            "RawSymbol",
            true,
        )],
    };

    let (batch, _stats) = stage.resolve(&context);

    let rel = batch
        .relationships
        .first()
        .expect("one resolved relationship");
    assert_eq!(
        rel.to_id, raw_symbol_new_id,
        "static call to RawSymbol::new must override the same-name local `new` in file scope"
    );
}
