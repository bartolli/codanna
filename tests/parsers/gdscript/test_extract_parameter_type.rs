//! gdscript parameter types supply receiver types.
//!
//! Stored gdscript signatures are rebuilt as `func name(params) -> ret`
//! (no body, no trailing colon) and parse clean as a
//! `function_definition` — no wrap needed, unlike kotlin (`fun`
//! restored) and swift (`{}` appended). `typed_parameter` carries
//! `(identifier) type: (type (identifier))`; an untyped parameter is a
//! bare `identifier` and stays unbound.

use codanna::parsing::LanguageBehavior;
use codanna::parsing::gdscript::GdscriptBehavior;

#[test]
fn typed_parameter_binds() {
    let behavior = GdscriptBehavior::new();
    let signature = "func handle(req: Request, count: int) -> int";
    assert_eq!(
        behavior.extract_parameter_type(signature, "req"),
        Some("Request".to_string())
    );
}

#[test]
fn untyped_parameter_stays_unbound() {
    let behavior = GdscriptBehavior::new();
    let signature = "func handle(req, count: int)";
    assert_eq!(behavior.extract_parameter_type(signature, "req"), None);
}

#[test]
fn default_valued_typed_parameter_binds() {
    let behavior = GdscriptBehavior::new();
    let signature = "func handle(req: Request = Request.new())";
    assert_eq!(
        behavior.extract_parameter_type(signature, "req"),
        Some("Request".to_string())
    );
}
