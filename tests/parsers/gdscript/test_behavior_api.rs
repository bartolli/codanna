//! GDScript behavior API tests - TDD approach
//!
//! These tests define the API contract that GdscriptBehavior must implement
//! to achieve parity with PythonBehavior (the closest language analog).
//!
//! Test Organization:
//! 1. Basic API (Tier 1) - Core behavior methods
//! 2. Stateful API (Tier 3) - Import tracking and state management
//! 3. Resolution API (Tier 2) - Symbol resolution helpers
//! 4. GDScript-specific - res:// paths, extends, class_name

use codanna::parsing::gdscript::{GdscriptBehavior, GdscriptResolutionContext};
use codanna::parsing::{Import, LanguageBehavior};
use codanna::{FileId, Visibility};
use std::path::Path;

// =============================================================================
// Tier 1: Basic API Tests (Should already pass)
// =============================================================================

#[test]
fn test_format_module_path() {
    let behavior = GdscriptBehavior::new();

    // GDScript uses file paths as module paths
    assert_eq!(
        behavior.format_module_path("scripts/player", "Player"),
        "scripts/player"
    );
}

#[test]
fn test_parse_visibility() {
    let behavior = GdscriptBehavior::new();

    // Public functions
    assert_eq!(
        behavior.parse_visibility("func move():"),
        Visibility::Public
    );
    assert_eq!(
        behavior.parse_visibility("class Player:"),
        Visibility::Public
    );

    // Private (underscore prefix - same as Python)
    assert_eq!(
        behavior.parse_visibility("func _private():"),
        Visibility::Private
    );
    assert_eq!(
        behavior.parse_visibility("var _internal = 0"),
        Visibility::Private
    );

    // Special methods like _ready, _process should be Public (Godot lifecycle)
    assert_eq!(
        behavior.parse_visibility("func _ready():"),
        Visibility::Private // Note: GDScript treats _ prefix as private convention
    );
}

#[test]
fn test_module_separator() {
    let behavior = GdscriptBehavior::new();
    // GDScript uses filesystem paths, separator is /
    assert_eq!(behavior.module_separator(), "/");
}

#[test]
fn test_supports_features() {
    let behavior = GdscriptBehavior::new();

    // GDScript doesn't have traits (interfaces exist but different)
    assert!(!behavior.supports_traits());

    // GDScript doesn't have inherent methods (everything is on classes)
    assert!(!behavior.supports_inherent_methods());
}

#[test]
fn test_validate_node_kinds() {
    let behavior = GdscriptBehavior::new();

    // Valid GDScript node kinds (from GRAMMAR_ANALYSIS.md - handled nodes)
    assert!(behavior.validate_node_kind("class_definition"));
    assert!(behavior.validate_node_kind("function_definition"));
    assert!(behavior.validate_node_kind("variable_statement"));

    // Invalid node kind (Rust-specific)
    assert!(!behavior.validate_node_kind("struct_item"));
}

#[test]
fn test_module_path_from_file() {
    let behavior = GdscriptBehavior::new();
    let root = Path::new("/project");

    let extensions = &["gd"];

    // Test regular script
    let script_path = Path::new("/project/scripts/player.gd");
    assert_eq!(
        behavior.module_path_from_file(script_path, root, extensions),
        Some("res://scripts/player".to_string())
    );

    // Test nested script
    let nested_path = Path::new("/project/scenes/levels/level1.gd");
    assert_eq!(
        behavior.module_path_from_file(nested_path, root, extensions),
        Some("res://scenes/levels/level1".to_string())
    );

    // Test root script
    let root_script = Path::new("/project/main.gd");
    assert_eq!(
        behavior.module_path_from_file(root_script, root, extensions),
        Some("res://main".to_string())
    );
}

// =============================================================================
// Tier 3: Stateful API Tests (Currently will fail - need implementation)
// =============================================================================

#[test]
fn test_has_behavior_state() {
    let behavior = GdscriptBehavior::new();

    // Behavior should have import tracking methods
    // (testing state() directly requires trait import, we test via methods instead)
    let file_id = FileId::new(1).expect("valid file id");
    let imports = behavior.get_imports_for_file(file_id);
    assert!(imports.is_empty()); // Should work without panic
}

#[test]
fn test_register_file() {
    use std::path::PathBuf;

    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");
    let path = PathBuf::from("/project/scripts/player.gd");
    let module_path = "res://scripts/player".to_string();

    // Should register file and associate with module path
    behavior.register_file(path.clone(), file_id, module_path.clone());

    // Should be able to retrieve module path
    assert_eq!(
        behavior.get_module_path_for_file(file_id),
        Some(module_path)
    );
}

#[test]
fn test_add_import() {
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    let import = Import {
        file_id,
        path: "res://scripts/enemy.gd".to_string(),
        alias: None,
        is_glob: false,
        is_type_only: false,
    };

    // Should track import
    behavior.add_import(import.clone());

    // Should retrieve imports for file
    let imports = behavior.get_imports_for_file(file_id);
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, "res://scripts/enemy.gd");
}

#[test]
fn test_get_imports_for_file_empty() {
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    // Should return empty vec for file with no imports
    let imports = behavior.get_imports_for_file(file_id);
    assert!(imports.is_empty());
}

#[test]
fn test_multiple_imports_same_file() {
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    // Add multiple imports for same file
    let import1 = Import {
        file_id,
        path: "res://scripts/enemy.gd".to_string(),
        alias: None,
        is_glob: false,
        is_type_only: false,
    };
    let import2 = Import {
        file_id,
        path: "res://scripts/weapon.gd".to_string(),
        alias: Some("Gun".to_string()),
        is_glob: false,
        is_type_only: false,
    };

    behavior.add_import(import1);
    behavior.add_import(import2);

    // Should retrieve all imports
    let imports = behavior.get_imports_for_file(file_id);
    assert_eq!(imports.len(), 2);
}

#[test]
fn test_imports_isolated_by_file() {
    let behavior = GdscriptBehavior::new();
    let file1 = FileId::new(1).expect("valid file id");
    let file2 = FileId::new(2).expect("valid file id");

    let import1 = Import {
        file_id: file1,
        path: "res://scripts/enemy.gd".to_string(),
        alias: None,
        is_glob: false,
        is_type_only: false,
    };
    let import2 = Import {
        file_id: file2,
        path: "res://scripts/weapon.gd".to_string(),
        alias: None,
        is_glob: false,
        is_type_only: false,
    };

    behavior.add_import(import1);
    behavior.add_import(import2);

    // Each file should only see its own imports
    assert_eq!(behavior.get_imports_for_file(file1).len(), 1);
    assert_eq!(behavior.get_imports_for_file(file2).len(), 1);
    assert_eq!(
        behavior.get_imports_for_file(file1)[0].path,
        "res://scripts/enemy.gd"
    );
    assert_eq!(
        behavior.get_imports_for_file(file2)[0].path,
        "res://scripts/weapon.gd"
    );
}

// =============================================================================
// Tier 2: Resolution API Tests (Currently will fail - need implementation)
// =============================================================================

#[test]
fn test_import_matches_symbol_exact() {
    let behavior = GdscriptBehavior::new();

    // Exact match
    assert!(behavior.import_matches_symbol("res://scripts/player", "res://scripts/player", None));
}

#[test]
fn test_import_matches_symbol_with_extension() {
    let behavior = GdscriptBehavior::new();

    // Import path has .gd extension, symbol path doesn't
    assert!(behavior.import_matches_symbol(
        "res://scripts/player.gd",
        "res://scripts/player",
        None
    ));

    // Both have extension
    assert!(behavior.import_matches_symbol(
        "res://scripts/player.gd",
        "res://scripts/player.gd",
        None
    ));
}

#[test]
fn test_import_matches_symbol_without_res_prefix() {
    let behavior = GdscriptBehavior::new();

    // Import without res://, symbol with res://
    assert!(behavior.import_matches_symbol("scripts/player.gd", "res://scripts/player", None));

    // Both without res://
    assert!(behavior.import_matches_symbol("scripts/player", "scripts/player", None));
}

#[test]
fn test_import_matches_symbol_relative_paths() {
    let behavior = GdscriptBehavior::new();

    // Relative import from same directory
    assert!(behavior.import_matches_symbol(
        "./enemy.gd",
        "res://scripts/enemy",
        Some("res://scripts/player")
    ));

    // Relative import from parent directory
    assert!(behavior.import_matches_symbol(
        "../utils/math.gd",
        "res://utils/math",
        Some("res://scripts/player")
    ));
}

#[test]
fn test_is_resolvable_symbol() {
    use codanna::symbol::ScopeContext;
    use codanna::{Range, Symbol, SymbolKind};

    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");
    let symbol_id = codanna::SymbolId::new(1).expect("valid symbol id");

    // Resolvable: Class
    let class_symbol = Symbol::new(
        symbol_id,
        "Player".to_string(),
        SymbolKind::Class,
        file_id,
        Range::new(0, 0, 10, 0),
    )
    .with_scope(ScopeContext::Module);
    assert!(behavior.is_resolvable_symbol(&class_symbol));

    // Resolvable: Function
    let func_symbol = Symbol::new(
        symbol_id,
        "move".to_string(),
        SymbolKind::Function,
        file_id,
        Range::new(0, 0, 5, 0),
    )
    .with_scope(ScopeContext::Module);
    assert!(behavior.is_resolvable_symbol(&func_symbol));

    // Not resolvable: Local parameter
    let param_symbol = Symbol::new(
        symbol_id,
        "x".to_string(),
        SymbolKind::Variable,
        file_id,
        Range::new(0, 0, 1, 0),
    )
    .with_scope(ScopeContext::Parameter);
    assert!(!behavior.is_resolvable_symbol(&param_symbol));
}

#[test]
fn test_is_symbol_visible_from_file() {
    use codanna::{Range, Symbol, SymbolKind};

    let behavior = GdscriptBehavior::new();
    let file1 = FileId::new(1).expect("valid file id");
    let file2 = FileId::new(2).expect("valid file id");
    let symbol_id = codanna::SymbolId::new(1).expect("valid symbol id");

    // Same file: always visible
    let symbol = Symbol::new(
        symbol_id,
        "_private_func".to_string(),
        SymbolKind::Function,
        file1,
        Range::new(0, 0, 5, 0),
    );
    assert!(behavior.is_symbol_visible_from_file(&symbol, file1));

    // Different file, public symbol: visible
    let public_symbol = Symbol::new(
        symbol_id,
        "public_func".to_string(),
        SymbolKind::Function,
        file1,
        Range::new(0, 0, 5, 0),
    );
    assert!(behavior.is_symbol_visible_from_file(&public_symbol, file2));

    // Different file, private symbol (underscore): not visible
    let private_symbol = Symbol::new(
        symbol_id,
        "_private_func".to_string(),
        SymbolKind::Function,
        file1,
        Range::new(0, 0, 5, 0),
    );
    assert!(!behavior.is_symbol_visible_from_file(&private_symbol, file2));
}

// =============================================================================
// GDScript-Specific Tests
// =============================================================================

#[test]
fn test_gdscript_class_name_import() {
    // class_name makes a class globally visible
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    let import = Import {
        file_id,
        path: "Player".to_string(), // class_name Player
        alias: None,
        is_glob: true, // Global visibility
        is_type_only: false,
    };

    behavior.add_import(import);

    let imports = behavior.get_imports_for_file(file_id);
    assert_eq!(imports.len(), 1);
    assert!(imports[0].is_glob);
}

#[test]
fn test_gdscript_extends_import() {
    // extends Parent should be tracked as import
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    let import = Import {
        file_id,
        path: "res://scripts/parent.gd".to_string(),
        alias: None,
        is_glob: false,
        is_type_only: false,
    };

    behavior.add_import(import);

    let imports = behavior.get_imports_for_file(file_id);
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, "res://scripts/parent.gd");
}

#[test]
fn test_gdscript_preload_import() {
    // preload("res://...") should be tracked
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    let import = Import {
        file_id,
        path: "res://scenes/enemy.tscn".to_string(),
        alias: Some("EnemyScene".to_string()),
        is_glob: false,
        is_type_only: false,
    };

    behavior.add_import(import);

    let imports = behavior.get_imports_for_file(file_id);
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].alias, Some("EnemyScene".to_string()));
}

#[test]
fn test_create_resolution_context() {
    let behavior = GdscriptBehavior::new();
    let file_id = FileId::new(1).expect("valid file id");

    // Should create GdscriptResolutionContext
    let mut context = behavior.create_resolution_context(file_id);

    // Should downcast to GdscriptResolutionContext
    let _gdscript_context = context
        .as_any_mut()
        .downcast_mut::<GdscriptResolutionContext>()
        .expect("Should be GdscriptResolutionContext");
}

#[test]
fn test_create_inheritance_resolver() {
    let behavior = GdscriptBehavior::new();

    // Should create GdscriptInheritanceResolver
    let _resolver = behavior.create_inheritance_resolver();

    // Should not panic
}

// =============================================================================
// Integration Tests - Behavior + Parser
// =============================================================================

#[test]
#[ignore] // Will pass after parser integration
fn test_parser_tracks_extends() {
    // This will test that when parser sees 'extends Parent',
    // it calls behavior.add_import() with the parent class
    todo!("Implement after parser integration")
}

#[test]
#[ignore] // Will pass after parser integration
fn test_parser_tracks_class_name() {
    // This will test that when parser sees 'class_name MyClass',
    // it calls behavior.add_import() to register global visibility
    todo!("Implement after parser integration")
}

#[test]
#[ignore] // Will pass after resolution implementation
fn test_cross_file_extends_resolution() {
    // This will test end-to-end:
    // 1. File A has class Parent
    // 2. File B has 'extends Parent'
    // 3. Resolution finds Parent class from File A
    todo!("Implement after build_resolution_context")
}
