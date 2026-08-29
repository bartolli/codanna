//! A php variable receiver resolves through its binding.
//!
//! The parser lane lock (`tests/parsers/php/test_variable_binding_sigil.rs`)
//! pins the binding name to the source token. This lane pins the consequence:
//! `binding_type_at_call_site` joins binding to call by `binding.name ==
//! receiver`, so a php binding recorded without its `$` never reaches
//! `is_instance_type_compatible` and the row fails closed.

use codanna::config::Settings;
use codanna::indexing::pipeline::ResolveStage;
use codanna::indexing::pipeline::types::{
    ResolutionContext, SymbolLookupCache, UnresolvedRelationship, VariableBinding,
};
use codanna::parsing::resolution::GenericResolutionContext;
use codanna::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use codanna::relationship::RelationshipMetadata;
use codanna::symbol::ScopeContext;
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::sync::Arc;

fn php_lang() -> LanguageId {
    LanguageId::new("php")
}

fn build_behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    let mut map = HashMap::new();
    let behavior: Arc<dyn LanguageBehavior> =
        Arc::from(factory.create_behavior_from_registry(php_lang()));
    map.insert(php_lang(), behavior);
    map
}

fn make_method(id: u32, name: &str, file_id: FileId, class: &str) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Method,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(php_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.into()),
    });
    sym
}

fn make_class(id: u32, name: &str, file_id: FileId) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Class,
        file_id,
        Range::new(id, 0, id + 1, 0),
    );
    sym.language_id = Some(php_lang());
    sym.visibility = Visibility::Public;
    sym.scope_context = Some(ScopeContext::Module);
    sym
}

fn make_caller(id: u32, name: &str, file_id: FileId) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Function,
        file_id,
        // Wide span so the binding sits inside the caller.
        Range::new(0, 0, 100, 0),
    );
    sym.language_id = Some(php_lang());
    sym.visibility = Visibility::Public;
    sym.signature = Some("function seed(HubPhp $h)".into());
    sym
}

/// The receiver token as the php parser captures it: raw source, sigil kept.
fn php_instance_call(from_id: u32, to_name: &str, file_id: FileId) -> UnresolvedRelationship {
    let meta = RelationshipMetadata::new()
        .at_position(42, 4)
        .with_receiver("$h")
        .static_call(false);
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(from_id).unwrap()),
        from_name: "seed".into(),
        to_name: to_name.into(),
        file_id,
        kind: RelationKind::Calls,
        metadata: Some(meta),
        to_range: Some(Range::new(42, 4, 42, 20)),
    }
}

fn resolve_with_binding_name(binding_name: &str) -> usize {
    let caller_file = FileId::new(1).unwrap();
    let hub_file = FileId::new(2).unwrap();

    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(make_caller(1, "seed", caller_file));
    cache.insert(make_class(2, "HubPhp", hub_file));
    cache.insert(make_method(3, "ping", hub_file, "HubPhp"));

    let stage = ResolveStage::new(Arc::clone(&cache), build_behaviors());

    let context = ResolutionContext {
        file_id: caller_file,
        language_id: php_lang(),
        imports: vec![],
        local_symbols: vec![],
        scope: Box::new(GenericResolutionContext::new(caller_file)),
        unresolved_rels: vec![php_instance_call(1, "ping", caller_file)],
        variable_bindings: vec![VariableBinding {
            name: binding_name.to_string(),
            type_name: "HubPhp".to_string(),
            range: Range::new(1, 0, 1, 10),
        }],
        this_barrier_spans: vec![],
    };

    let (batch, _) = stage.resolve(&context);
    batch.len()
}

#[test]
fn php_variable_receiver_resolves_through_its_binding() {
    assert_eq!(
        resolve_with_binding_name("$h"),
        1,
        "a binding under the source token must join the call row's receiver"
    );
}

#[test]
fn php_binding_without_the_sigil_does_not_join() {
    // Pins the mechanism, not a wish: a sigil-stripped binding cannot match the
    // receiver token, which is exactly why the parser lane must keep the `$`.
    assert_eq!(
        resolve_with_binding_name("h"),
        0,
        "a sigil-stripped binding names a variable that no php call row carries"
    );
}
