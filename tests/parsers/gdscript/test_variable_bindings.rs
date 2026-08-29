//! gdscript local bindings supply receiver types.
//!
//! gdscript took both binding-channel trait defaults, so no receiver
//! ever acquired a type from a declaration and every such member call
//! failed closed.
//!
//! Shapes, captured from grammar output (tree-sitter-gdscript 6.1.0):
//!
//! ```text
//! variable_statement
//!   name: (name)
//!   type: (type (identifier))        `var x: Type`
//!   type: (inferred_type)            `var x := ...`
//!   value: (attribute (identifier) (attribute_call (identifier "new") arguments))
//!                                    `Type.new()` — the explicit
//!                                    constructor idiom names its type
//!   value: (call (identifier))       plain call — a factory's return
//!                                    type lives elsewhere; not evidence
//! ```

use codanna::parsing::LanguageParser;
use codanna::parsing::gdscript::GdscriptParser;

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = GdscriptParser::new().expect("gdscript parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn annotation_typed_var_binds() {
    let code = "func caller():\n    var cache: ImageCache\n    cache.evict()\n";
    assert_eq!(
        bindings_for(code),
        vec![("cache".to_string(), "ImageCache".to_string())]
    );
}

#[test]
fn inferred_new_binds() {
    let code = "func caller():\n    var store := DataStore.new()\n    store.save()\n";
    assert_eq!(
        bindings_for(code),
        vec![("store".to_string(), "DataStore".to_string())]
    );
}

#[test]
fn plain_assign_new_binds() {
    let code = "func caller():\n    var loose = Helper.new()\n    loose.run()\n";
    assert_eq!(
        bindings_for(code),
        vec![("loose".to_string(), "Helper".to_string())]
    );
}

#[test]
fn annotation_outranks_initializer() {
    let code = "func caller():\n    var q: Queue = Pool.new()\n";
    assert_eq!(
        bindings_for(code),
        vec![("q".to_string(), "Queue".to_string())]
    );
}

#[test]
fn factory_call_stays_unbound() {
    // A plain call's return type lives in another signature; `new` on a
    // type name is the only initializer evidence (go/kotlin rule).
    let code = "func caller():\n    var thing = make_thing()\n    thing.run()\n";
    assert_eq!(bindings_for(code), Vec::<(String, String)>::new());
}

#[test]
fn class_level_vars_bind() {
    let code = "extends Node\n\nvar cache: ImageCache\nvar store := DataStore.new()\n";
    assert_eq!(
        bindings_for(code),
        vec![
            ("cache".to_string(), "ImageCache".to_string()),
            ("store".to_string(), "DataStore".to_string()),
        ]
    );
}

#[test]
fn dotted_new_head_stays_unbound() {
    // `a.b.new()` nests attributes; only a bare type head is evidence.
    let code = "func caller():\n    var x = scene.loader.new()\n";
    assert_eq!(bindings_for(code), Vec::<(String, String)>::new());
}
