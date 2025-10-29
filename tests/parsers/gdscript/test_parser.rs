use codanna::Visibility;
use codanna::parsing::LanguageParser;
use codanna::parsing::gdscript::GdscriptParser;
use codanna::types::{FileId, SymbolCounter, SymbolKind};

fn build_parser() -> (GdscriptParser, FileId, SymbolCounter) {
    let parser = GdscriptParser::new().expect("Failed to create GDScript parser");
    let file_id = FileId::new(1).expect("Invalid file id");
    let counter = SymbolCounter::new();
    (parser, file_id, counter)
}

#[test]
fn test_gdscript_parser_extracts_core_symbols() {
    let code = r#"
## Main player script
class_name Player

## Player character implementation
class Player extends CharacterBody2D:
    ## Emitted when health changes
    signal health_changed(new_value)

    ## Movement speed in pixels per second
    var speed := 400

    ## Maximum allowed health
    const MAX_HEALTH := 100

    ## Creates a new player instance
    func _init():
        self.health = MAX_HEALTH

    ## Moves the player using input
    func move(delta):
        return speed * delta

## Utility helper available to the script
func helper():
    return "helper"
"#;

    let (mut parser, file_id, mut counter) = build_parser();
    let symbols = parser.parse(code, file_id, &mut counter);

    // Module symbol is synthesized at parser level
    let module_symbol = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Module)
        .expect("Script should generate a module symbol");
    assert_eq!(module_symbol.name.as_ref(), "<script>");

    // Class definition with documentation
    let player_class = symbols
        .iter()
        .find(|s| s.signature.as_deref() == Some("class Player"))
        .expect("Player class should be extracted");
    let class_doc = player_class
        .doc_comment
        .as_deref()
        .expect("Player class should include documentation comment");
    assert!(
        class_doc.contains("Player character implementation"),
        "Unexpected class documentation: {class_doc}"
    );

    // Instance variable inside the class
    let speed_field = symbols
        .iter()
        .find(|s| s.name.as_ref() == "speed")
        .expect("Instance variable should be extracted");
    assert_eq!(speed_field.kind, SymbolKind::Field);

    // Constructor should be detected as a method and remain private due to leading underscore
    let init_method = symbols
        .iter()
        .find(|s| s.name.as_ref() == "_init")
        .expect("Constructor should be extracted");
    assert_eq!(init_method.kind, SymbolKind::Method);
    assert_eq!(init_method.visibility, Visibility::Private);
    let init_doc = init_method
        .doc_comment
        .as_deref()
        .expect("Constructor should capture documentation");
    assert!(
        init_doc.contains("Creates a new player instance"),
        "Unexpected constructor doc: {init_doc}"
    );

    // Free-standing function at script scope
    let helper_fn = symbols
        .iter()
        .find(|s| s.name.as_ref() == "helper")
        .expect("Free function should be extracted");
    assert_eq!(helper_fn.kind, SymbolKind::Function);
    assert!(
        helper_fn
            .doc_comment
            .as_deref()
            .unwrap_or_default()
            .contains("Utility helper"),
        "Helper function should retain documentation"
    );

    // Ensure inheritance relationships are surfaced
    let extends = parser.find_extends(code);
    assert!(
        extends
            .iter()
            .any(|(derived, base, _)| *derived == "Player" && *base == "CharacterBody2D"),
        "Player should extend CharacterBody2D, got {extends:?}"
    );
}
