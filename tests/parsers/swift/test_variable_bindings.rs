//! swift local bindings supply receiver types.
//!
//! swift took both binding-channel trait defaults, so no receiver ever
//! acquired a type from a local declaration and every such member call
//! failed closed.
//!
//! Shapes, captured from grammar output (tree-sitter-swift 0.7.3):
//!
//! ```text
//! property_declaration
//!   (value_binding_pattern)                       `let` | `var`
//!   name: pattern(bound_identifier: simple_identifier)
//!   (type_annotation name: user_type | optional_type(wrapped: user_type))
//!   value: call_expression(simple_identifier | navigation_expression, call_suffix)
//! ```
//!
//! One declaration carries repeated name/annotation/value groups
//! (`let a = A(), b: B = makeB()`), so grouping is sequential per
//! `pattern` child. A called type name is always `.init` in swift (no
//! invoke operator on metatypes), so the initializer callee is capture
//! evidence; a captured factory name matches no ClassMember class and
//! stays inert at resolution (story Decisions, 2026-08-29 ruling).

use codanna::parsing::LanguageParser;
use codanna::parsing::swift::SwiftParser;

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = SwiftParser::new().expect("swift parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn annotation_typed_local_binds() {
    let code = r#"
func caller() {
    let cache: ImageCache
    cache.evict()
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("cache".to_string(), "ImageCache".to_string())]
    );
}

#[test]
fn optional_annotation_unwraps() {
    let code = r#"
func caller() {
    let retrier: RequestRetrier? = nil
    retrier?.retry()
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("retrier".to_string(), "RequestRetrier".to_string())]
    );
}

#[test]
fn initializer_typed_local_binds() {
    let code = r#"
func caller() {
    let session = Session()
    session.request("u")
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("session".to_string(), "Session".to_string())]
    );
}

#[test]
fn navigation_initializer_binds_tail() {
    let code = r#"
func caller() {
    let ns = Alamofire.Session()
    ns.request("u")
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("ns".to_string(), "Session".to_string())]
    );
}

#[test]
fn explicit_init_binds_target() {
    let code = r#"
func caller() {
    let s = Session.init(label: "x")
    s.request("u")
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("s".to_string(), "Session".to_string())]
    );
}

#[test]
fn multi_binding_declaration_groups_per_name() {
    let code = r#"
func caller() {
    let a = Alpha(), b: Beta = makeBeta(), c = makeGamma()
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![
            ("a".to_string(), "Alpha".to_string()),
            ("b".to_string(), "Beta".to_string()),
            ("c".to_string(), "makeGamma".to_string()),
        ]
    );
}

#[test]
fn factory_capture_is_emitted_verbatim() {
    // Deliberate: the capture is inert at resolution unless a class named
    // makeThing exists (story Decisions, 2026-08-29 ruling).
    let code = r#"
func caller() {
    let thing = makeThing()
    thing.run()
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("thing".to_string(), "makeThing".to_string())]
    );
}

#[test]
fn var_binds_like_let() {
    let code = r#"
func caller() {
    var manager: NetworkManager = shared
    manager.retry()
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![("manager".to_string(), "NetworkManager".to_string())]
    );
}
