//! csharp parameter types supply receiver types.
//!
//! Stored csharp method signatures exclude the body and are only valid
//! inside a class, so the channel wraps them `class __W__ { <sig> {} }`
//! before parsing (java's wrap precedent). `parameter` carries `type:`
//! and `name:` fields; `ref`/`out`/`in` are sibling modifier nodes and
//! do not displace them. Type reduction: identifier verbatim,
//! `qualified_name` rightmost, `generic_name` base identifier,
//! `nullable_type` unwrapped; `predefined_type` (int, string) stays
//! unbound.

use codanna::parsing::LanguageBehavior;
use codanna::parsing::csharp::CSharpBehavior;

#[test]
fn plain_parameter_binds() {
    let behavior = CSharpBehavior::new();
    let signature = "public void Handle(Bus bus)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "bus"),
        Some("Bus".to_string())
    );
}

#[test]
fn ref_out_in_modifiers_bind() {
    let behavior = CSharpBehavior::new();
    let signature = "void F(ref Cache cache, out Session s, in Widget w)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "cache"),
        Some("Cache".to_string())
    );
    assert_eq!(
        behavior.extract_parameter_type(signature, "s"),
        Some("Session".to_string())
    );
    assert_eq!(
        behavior.extract_parameter_type(signature, "w"),
        Some("Widget".to_string())
    );
}

#[test]
fn generic_takes_base_identifier() {
    let behavior = CSharpBehavior::new();
    let signature = "void F(IList<Item> items)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "items"),
        Some("IList".to_string())
    );
}

#[test]
fn qualified_takes_rightmost() {
    let behavior = CSharpBehavior::new();
    let signature = "void F(Foo.Bar b)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "b"),
        Some("Bar".to_string())
    );
}

#[test]
fn nullable_unwraps() {
    let behavior = CSharpBehavior::new();
    let signature = "void F(Session? s)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "s"),
        Some("Session".to_string())
    );
}

#[test]
fn predefined_type_stays_unbound() {
    let behavior = CSharpBehavior::new();
    let signature = "void F(int count, string name)";
    assert_eq!(behavior.extract_parameter_type(signature, "count"), None);
    assert_eq!(behavior.extract_parameter_type(signature, "name"), None);
}

#[test]
fn default_valued_parameter_binds() {
    let behavior = CSharpBehavior::new();
    let signature = "void F(Session s = null)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "s"),
        Some("Session".to_string())
    );
}
