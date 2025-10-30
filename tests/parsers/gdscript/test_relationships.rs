use codanna::parsing::LanguageParser;
use codanna::parsing::gdscript::GdscriptParser;

fn load_fixture() -> &'static str {
    include_str!("../../fixtures/gdscript/player.gd")
}

#[test]
fn test_gdscript_find_calls_captures_signals_and_scene_calls() {
    let code = load_fixture();
    let mut parser = GdscriptParser::new().expect("Failed to create GDScript parser");

    let calls = parser.find_calls(code);

    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "_ready" && *callee == "spawn_enemy"),
        "expected _ready to call spawn_enemy, got {calls:?}"
    );

    let emits_health_changed = calls.iter().any(|(caller, callee, _)| {
        *caller == "apply_damage" && (*callee == "health_changed" || *callee == "emit_signal")
    });
    assert!(
        emits_health_changed,
        "expected apply_damage to emit health_changed (directly or via emit_signal), got {calls:?}"
    );

    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "apply_damage" && *callee == "_reset"),
        "expected apply_damage to invoke _reset, got {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "_reset" && *callee == "add_child"),
        "expected _reset to add_child the effect, got {calls:?}"
    );
}

#[test]
fn test_gdscript_find_uses_detects_extends_and_preloads() {
    let code = load_fixture();
    let mut parser = GdscriptParser::new().expect("Failed to create GDScript parser");

    let uses = parser.find_uses(code);

    assert!(
        uses.iter()
            .any(|(source, target, _)| *source == SCRIPT_SCOPE && *target == "CharacterBody2D"),
        "expected script to extend CharacterBody2D, got {uses:?}"
    );

    assert!(
        uses.iter()
            .any(|(source, target, _)| *source == "EnemyScene"
                && *target == "res://enemies/enemy.gd"),
        "expected EnemyScene constant to preload enemy.gd, got {uses:?}"
    );

    assert!(
        uses.iter()
            .any(|(source, target, _)| *source == "_reset"
                && *target == "res://effects/heal_effect.gd"),
        "expected _reset to preload heal_effect.gd, got {uses:?}"
    );
}

const SCRIPT_SCOPE: &str = "<script>";
