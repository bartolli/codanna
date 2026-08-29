//! php binds constructor-assigned locals, and reduces parameter types to a
//! name that can match a class.
//!
//! `extract_variable_types_from_node` matched `simple_parameter` only, so the
//! most direct receiver-typing idiom in php — `$x = new Foo(); $x->method();`
//! — supplied no evidence. The parameter arm additionally matched only a
//! direct `named_type` child, so the wrapped forms never bound:
//!
//! ```text
//! simple_parameter type: named_type    > name          "Foo"     bound
//! simple_parameter type: optional_type > named_type    "?Foo"    wrapped
//! simple_parameter type: union_type    > named_type... "A|B"     wrapped
//!
//! assignment_expression
//!   left:  variable_name                               "$x"
//!   right: object_creation_expression
//!            name | qualified_name                     "Foo" | "\App\Foo"
//! ```
//!
//! Union types stay unbound by decision: the channel carries one type per
//! name and the join takes the latest binding, so emitting both members would
//! silently pick one. Fail closed instead.

use codanna::parsing::LanguageParser;
use codanna::parsing::php::PhpParser;

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = PhpParser::new().expect("php parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn constructor_assignment_binds() {
    let code = r#"<?php
function seed(): void
{
    $local = new HubPhp();
    $local->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("$local".to_string(), "HubPhp".to_string())),
        "a constructor-assigned local must bind, got {got:?}"
    );
}

#[test]
fn qualified_constructor_binds_the_class_tail() {
    let code = r#"<?php
function seed(): void
{
    $qualified = new \App\HubPhp();
    $qualified->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("$qualified".to_string(), "HubPhp".to_string())),
        "a namespace-qualified constructor binds its class tail, got {got:?}"
    );
}

#[test]
fn nullable_parameter_type_binds_the_inner_class() {
    let code = r#"<?php
function seed(?HubPhp $nullable): void
{
    $nullable->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("$nullable".to_string(), "HubPhp".to_string())),
        "`?Foo` is Foo-or-null; a call on it targets Foo, got {got:?}"
    );
}

#[test]
fn union_parameter_type_fails_closed() {
    let code = r#"<?php
function seed(HubPhp|OtherPhp $union): void
{
    $union->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        !got.iter().any(|(name, _)| name == "$union"),
        "a union receiver names no single type; binding one member would be a guess, got {got:?}"
    );
}

#[test]
fn plain_parameter_type_still_binds() {
    let code = r#"<?php
function seed(HubPhp $plain): void
{
    $plain->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("$plain".to_string(), "HubPhp".to_string())),
        "the parameter arm must keep working, got {got:?}"
    );
}

#[test]
fn non_constructor_assignment_supplies_nothing() {
    // `$x = helper();` has no type without full inference. A guess here would
    // attach calls to an arbitrary same-name class.
    let code = r#"<?php
function seed(): void
{
    $fromCall = helper();
    $fromCall->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        !got.iter().any(|(name, _)| name == "$fromCall"),
        "a non-constructor initializer supplies no type evidence, got {got:?}"
    );
}

#[test]
fn qualified_parameter_type_binds_the_class_tail() {
    // The assignment arm already reduces `new \App\Foo()` to `Foo`. A
    // parameter annotated `\App\Foo` must reduce the same way: the binding
    // type is matched against a ClassMember class name, which carries no
    // namespace prefix.
    let code = r#"<?php
namespace App;

function seed(\App\HubPhp $qualified): void
{
    $qualified->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("$qualified".to_string(), "HubPhp".to_string())),
        "a namespace-qualified parameter type binds its class tail, got {got:?}"
    );
}

#[test]
fn nullable_qualified_parameter_type_binds_the_class_tail() {
    let code = r#"<?php
namespace App;

function seed(?\App\HubPhp $both): void
{
    $both->ping();
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("$both".to_string(), "HubPhp".to_string())),
        "the nullable and qualified reductions compose, got {got:?}"
    );
}
