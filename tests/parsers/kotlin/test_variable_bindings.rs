//! kotlin supplies receiver types through both evidence channels.
//!
//! `find_variable_types` returned zero bindings for every shape: kotlin's
//! channel stores EXPRESSION TEXT -> type (`"42"` -> Int, `"helper()"` ->
//! return type) for its generic-substitution and extension-call inference,
//! while `binding_type_at_call_site` joins on `binding.name == receiver`. A
//! `val h = HubKt()` therefore never produced an `h` key, and kotlin's
//! behavior overrode no `extract_parameter_type`, so both shapes were dead.
//!
//! Shapes, captured from grammar output:
//!
//! ```text
//! parameter
//!   simple_identifier              "h"
//!   user_type > type_identifier    "HubKt"
//!
//! property_declaration
//!   binding_pattern_kind           "val" / "var"
//!   variable_declaration
//!     simple_identifier            "h"
//!     [user_type > type_identifier]  "HubKt"   only when annotated
//!   call_expression                "HubKt()"
//! ```
//!
//! An un-annotated `val h = HubKt()` is deliberately NOT bound here: kotlin
//! constructors are syntactically identical to function calls, and an
//! unanchored type name falls through to name-keyed receiver matching, so a
//! guessed class name can mis-pick. Under-report beats mis-report.

use codanna::parsing::kotlin::{KotlinBehavior, KotlinParser};
use codanna::parsing::{LanguageBehavior, LanguageParser};

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = KotlinParser::new().expect("kotlin parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn annotated_property_binds_its_declared_type() {
    let code = r#"
package fx

fun seed() {
    val typed: HubKt = HubKt()
    typed.pingKt()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("typed".to_string(), "HubKt".to_string())),
        "an annotated property binds under its variable name, got {got:?}"
    );
}

#[test]
fn annotated_var_property_binds_too() {
    let code = r#"
package fx

fun seed() {
    var mutable: HubKt = HubKt()
    mutable.pingKt()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("mutable".to_string(), "HubKt".to_string())),
        "`var` binds like `val`, got {got:?}"
    );
}

#[test]
fn unannotated_constructor_property_stays_unbound() {
    // `HubKt()` and `helper()` are the same shape in kotlin. Binding the
    // callee name would guess a class, and an unanchored type name is matched
    // name-keyed against candidate members — a guess can mis-pick.
    let code = r#"
package fx

fun seed() {
    val h = HubKt()
    h.pingKt()
}
"#;
    let got = bindings_for(code);
    assert!(
        !got.iter().any(|(name, _)| name == "h"),
        "an un-annotated initializer supplies no class evidence, got {got:?}"
    );
}

#[test]
fn parameter_type_is_extractable_from_the_signature() {
    let behavior = KotlinBehavior::new();
    assert_eq!(
        // The PRODUCTION form: the kotlin parser stores signatures without
        // the `fun` keyword, so a bare `fun ...` string would test input the
        // resolver never supplies.
        behavior.extract_parameter_type("seedParamKt (h: HubKt)", "h"),
        Some("HubKt".to_string()),
        "a kotlin parameter's declared type must be readable from its signature"
    );
}

#[test]
fn parameter_type_picks_the_named_parameter() {
    let behavior = KotlinBehavior::new();
    let signature = "seed (first: HubKt, second: OtherKt)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "second"),
        Some("OtherKt".to_string()),
        "the requested parameter's type is the one returned"
    );
}

#[test]
fn parameter_type_absent_for_unknown_name() {
    let behavior = KotlinBehavior::new();
    assert_eq!(
        behavior.extract_parameter_type("seed (h: HubKt)", "missing"),
        None,
        "a name that is not a parameter has no declared type"
    );
}
