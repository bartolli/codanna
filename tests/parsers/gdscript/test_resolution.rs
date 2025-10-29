use codanna::parsing::gdscript::{
    GdscriptBehavior, GdscriptInheritanceResolver, GdscriptResolutionContext,
};
use codanna::parsing::{
    InheritanceResolver, LanguageBehavior, ResolutionScope, ScopeLevel, ScopeType,
};
use codanna::{FileId, SymbolId};

#[test]
fn test_gdscript_resolution_context_basic() {
    let file_id = FileId::new(1).unwrap();
    let mut context = GdscriptResolutionContext::new(file_id);

    let player_id = SymbolId::new(11).unwrap();
    context.add_symbol("Player".into(), player_id, ScopeLevel::Module);
    assert_eq!(context.resolve("Player"), Some(player_id));

    context.enter_scope(ScopeType::Class);
    let move_id = SymbolId::new(12).unwrap();
    context.add_symbol("move".into(), move_id, ScopeLevel::Module);
    assert_eq!(context.resolve("move"), Some(move_id));

    context.enter_scope(ScopeType::function());
    let tmp_id = SymbolId::new(13).unwrap();
    context.add_symbol("temp".into(), tmp_id, ScopeLevel::Local);
    assert_eq!(context.resolve("temp"), Some(tmp_id));

    context.exit_scope(); // function
    assert!(context.resolve("temp").is_none());

    context.exit_scope(); // class
    assert_eq!(context.resolve("Player"), Some(player_id));
    assert_eq!(context.resolve("move"), Some(move_id));
}

#[test]
fn test_gdscript_behavior_produces_context() {
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(2).unwrap();
    let mut context = behavior.create_resolution_context(file_id);

    let helper_id = SymbolId::new(21).unwrap();
    context.add_symbol("helper".into(), helper_id, ScopeLevel::Module);
    assert_eq!(context.resolve("helper"), Some(helper_id));
}

#[test]
fn test_gdscript_inheritance_resolver() {
    let mut resolver = GdscriptInheritanceResolver::new();
    resolver.add_inheritance("Player".into(), "CharacterBody2D".into(), "extends");
    resolver.add_inheritance("CharacterBody2D".into(), "Node2D".into(), "extends");
    resolver.add_type_methods(
        "CharacterBody2D".into(),
        vec!["physics_process".into(), "move_and_slide".into()],
    );
    resolver.add_type_methods("Player".into(), vec!["jump".into()]);

    assert!(resolver.is_subtype("Player", "Node2D"));
    assert_eq!(
        resolver.resolve_method("Player", "move_and_slide"),
        Some("CharacterBody2D".into())
    );

    let mut methods = resolver.get_all_methods("Player");
    methods.sort();
    assert_eq!(
        methods,
        vec![
            "jump".to_string(),
            "move_and_slide".to_string(),
            "physics_process".to_string()
        ]
    );
}
