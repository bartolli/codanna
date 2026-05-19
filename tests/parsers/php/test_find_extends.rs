use codanna::parsing::{LanguageParser, php::PhpParser};

#[test]
fn test_find_implementations_emits_each_interface() {
    let code = r#"<?php
interface I {}
interface J {}
class Multi implements I, J {}
"#;
    let mut parser = PhpParser::new().unwrap();
    let extends = parser.find_extends(code);
    let implementations = parser.find_implementations(code);

    let pairs: Vec<(&str, &str)> = implementations.iter().map(|(c, i, _)| (*c, *i)).collect();
    assert_eq!(
        pairs,
        vec![("Multi", "I"), ("Multi", "J")],
        "implements I, J must emit one entry per interface in order"
    );
    assert!(
        extends.is_empty(),
        "interfaces must not leak into find_extends; got {extends:?}"
    );
}

#[test]
fn test_find_both_extends_and_implements() {
    let code = r#"<?php
class Base {}
interface I {}
class Child extends Base implements I {}
"#;
    let mut parser = PhpParser::new().unwrap();
    let extends = parser.find_extends(code);
    let implementations = parser.find_implementations(code);

    let ext_pairs: Vec<(&str, &str)> = extends.iter().map(|(c, b, _)| (*c, *b)).collect();
    let impl_pairs: Vec<(&str, &str)> = implementations.iter().map(|(c, i, _)| (*c, *i)).collect();
    assert_eq!(ext_pairs, vec![("Child", "Base")]);
    assert_eq!(impl_pairs, vec![("Child", "I")]);
}

#[test]
fn test_find_extends_emits_derived_to_base() {
    // `base_clause` in tree-sitter-php is the `extends X` clause (single name).
    // `class_interface_clause` is `implements I, J`. find_extends must emit
    // only the extends edge and find_implementations must not.
    let code = r#"<?php
class Child extends Base {}
"#;
    let mut parser = PhpParser::new().unwrap();
    let extends = parser.find_extends(code);
    let implementations = parser.find_implementations(code);

    assert_eq!(
        extends.len(),
        1,
        "Child extends Base must emit exactly one extends edge, got {extends:?}"
    );
    let (derived, base, _) = extends[0];
    assert_eq!(derived, "Child");
    assert_eq!(base, "Base");
    assert!(
        implementations.is_empty(),
        "extends must not leak into find_implementations; got {implementations:?}"
    );
}
