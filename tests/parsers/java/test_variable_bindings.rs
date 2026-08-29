//! java local-variable bindings feed the receiver-type evidence channel.
//!
//! `collect_variable_types` was an empty stub, so `find_variable_types`
//! satisfied the `LanguageParser` contract while returning nothing: java
//! advertised the binding channel and supplied no bindings, and every call
//! through a constructor-bound local failed closed. The param channel
//! (`extract_parameter_type`) carried parameter receivers and hid the gap.
//!
//! Shape, captured from grammar output:
//!
//! ```text
//! local_variable_declaration
//!   type: type_identifier            ("HubJava", or literally "var")
//!   declarator: variable_declarator
//!     name: identifier               ("h")
//!     value: object_creation_expression
//!       type: type_identifier        ("HubJava")
//! ```

use codanna::parsing::LanguageParser;
use codanna::parsing::java::JavaParser;

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = JavaParser::new().expect("java parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn explicit_type_with_constructor_binds() {
    let code = r#"
class Probe {
    void seed() {
        HubJava h = new HubJava();
        h.pingJava();
    }
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("h".to_string(), "HubJava".to_string())),
        "constructor-bound local must bind to its declared type, got {got:?}"
    );
}

#[test]
fn var_takes_its_type_from_the_initializer() {
    let code = r#"
class Probe {
    void seed() {
        var inferred = new HubJava();
        inferred.pingJava();
    }
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("inferred".to_string(), "HubJava".to_string())),
        "`var` carries no type of its own; the initializer supplies it, got {got:?}"
    );
    assert!(
        !got.iter().any(|(_, ty)| ty == "var"),
        "`var` must never be recorded as a type name, got {got:?}"
    );
}

#[test]
fn declaration_without_initializer_uses_the_declared_type() {
    let code = r#"
class Probe {
    void seed() {
        HubJava assigned;
        assigned = new HubJava();
    }
}
"#;
    let got = bindings_for(code);
    assert!(
        got.contains(&("assigned".to_string(), "HubJava".to_string())),
        "an uninitialized declaration still names its type, got {got:?}"
    );
}

#[test]
fn several_declarators_in_one_statement_all_bind() {
    let code = r#"
class Probe {
    void seed() {
        HubJava first = new HubJava(), second = new HubJava();
    }
}
"#;
    let got = bindings_for(code);
    let names: Vec<&String> = got.iter().map(|(n, _)| n).collect();
    assert!(
        names.contains(&&"first".to_string()) && names.contains(&&"second".to_string()),
        "each declarator in the statement binds, got {got:?}"
    );
}

#[test]
fn untyped_var_initializer_fails_closed() {
    // `var x = 5` has no type to recover without full inference. Recording a
    // guess here would attach calls to an arbitrary same-name type; the
    // channel stays empty instead.
    let code = r#"
class Probe {
    void seed() {
        var count = 5;
    }
}
"#;
    let got = bindings_for(code);
    assert!(
        !got.iter().any(|(n, _)| n == "count"),
        "a primitive-initialized `var` supplies no type evidence, got {got:?}"
    );
}
