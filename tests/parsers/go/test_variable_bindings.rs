//! go local bindings supply receiver types.
//!
//! go carried `extract_parameter_type` but no `find_variable_types` at all,
//! so a receiver bound to a local had no evidence and every such call failed
//! closed.
//!
//! Shapes, captured from grammar output:
//!
//! ```text
//! short_var_declaration
//!   left:  expression_list(identifier)
//!   right: expression_list(composite_literal type: type_identifier)
//!   right: expression_list(unary_expression operand: composite_literal)   `&T{}`
//!
//! var_declaration > var_spec
//!   name: identifier
//!   type: type_identifier | pointer_type(type_identifier)
//!                         | qualified_type(package, name: type_identifier)
//! ```
//!
//! A composite literal names its type outright, so it is evidence. A
//! constructor-style `h := NewHubGo()` is not: the return type lives in
//! another function's signature, and guessing from the `New` naming
//! convention would be a convention, not evidence.

use codanna::parsing::LanguageParser;
use codanna::parsing::go::GoParser;

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = GoParser::new().expect("go parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn pointer_composite_literal_binds() {
    let code = r#"package fx

func seed() {
	a := &HubGo{}
	a.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("a".to_string(), "HubGo".to_string())),
        "`&T{{}}` names its type outright, got {got:?}"
    );
}

#[test]
fn value_composite_literal_binds() {
    let code = r#"package fx

func seed() {
	b := HubGo{}
	b.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("b".to_string(), "HubGo".to_string())),
        "`T{{}}` binds like `&T{{}}`, got {got:?}"
    );
}

#[test]
fn qualified_composite_literal_binds_the_type_tail() {
    let code = r#"package fx

func seed() {
	f := pkg.HubGo{}
	f.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("f".to_string(), "HubGo".to_string())),
        "a package-qualified literal binds its type name, got {got:?}"
    );
}

#[test]
fn var_declaration_without_value_binds_its_type() {
    let code = r#"package fx

func seed() {
	var c HubGo
	c.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("c".to_string(), "HubGo".to_string())),
        "a declared type binds without any initializer, got {got:?}"
    );
}

#[test]
fn pointer_var_declaration_binds_the_pointee() {
    let code = r#"package fx

func seed() {
	var d *HubGo = &HubGo{}
	d.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("d".to_string(), "HubGo".to_string())),
        "`*T` binds T; the pointer is not part of the class name, got {got:?}"
    );
}

#[test]
fn multiple_names_in_one_var_spec_all_bind() {
    let code = r#"package fx

func seed() {
	var a, b HubGo
	a.Ping()
}
"#;
    let got = bindings_for(code);
    let names: Vec<&String> = got.iter().map(|(n, _)| n).collect();
    assert!(
        names.contains(&&"a".to_string()) && names.contains(&&"b".to_string()),
        "each name in the spec takes the declared type, got {got:?}"
    );
}

#[test]
fn constructor_call_stays_unbound() {
    // `NewHubGo()` returns whatever its signature says, which lives in another
    // function. The `New` prefix is a convention, not evidence.
    let code = r#"package fx

func seed() {
	e := NewHubGo()
	e.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        !got.iter().any(|(name, _)| name == "e"),
        "a constructor-style call supplies no local type evidence, got {got:?}"
    );
}

#[test]
fn mismatched_multi_assign_stays_unbound() {
    // `a, b := f()` pairs one right-hand expression against two names; there
    // is no positional type to take.
    let code = r#"package fx

func seed() {
	a, b := twoResults()
	a.Ping()
}
"#;
    let got = bindings_for(code);
    assert!(
        !got.iter().any(|(name, _)| name == "a" || name == "b"),
        "an unpairable multi-assign supplies nothing, got {got:?}"
    );
}
