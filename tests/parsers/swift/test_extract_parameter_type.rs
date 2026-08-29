//! swift parameter types supply receiver types.
//!
//! Stored swift signatures keep the `func` keyword (extract_signature
//! slices from node start) but carry no body, which alone parses as an
//! ERROR node; a `{}` body appended before parsing yields a clean
//! function_declaration (go's wrap precedent). The `parameter` node
//! separates the argument label (field `external_name`) from the
//! internal name and the type, which BOTH sit in field `name` — the
//! body references the internal name, so the label never matches.

use codanna::parsing::LanguageBehavior;
use codanna::parsing::swift::SwiftBehavior;

#[test]
fn plain_parameter_binds() {
    let behavior = SwiftBehavior::new();
    let signature = "func handle(manager: NetworkManager) -> Int";
    assert_eq!(
        behavior.extract_parameter_type(signature, "manager"),
        Some("NetworkManager".to_string())
    );
}

#[test]
fn labeled_parameter_binds_internal_name() {
    let behavior = SwiftBehavior::new();
    let signature = "func handle(with manager: NetworkManager) -> Int";
    assert_eq!(
        behavior.extract_parameter_type(signature, "manager"),
        Some("NetworkManager".to_string())
    );
    // The argument label is not the body-visible name.
    assert_eq!(behavior.extract_parameter_type(signature, "with"), None);
}

#[test]
fn underscore_labeled_parameter_binds() {
    let behavior = SwiftBehavior::new();
    let signature = "func handle(_ session: Session) -> Int";
    assert_eq!(
        behavior.extract_parameter_type(signature, "session"),
        Some("Session".to_string())
    );
}

#[test]
fn optional_parameter_unwraps() {
    let behavior = SwiftBehavior::new();
    let signature = "func handle(retrier: RequestRetrier?) -> Int";
    assert_eq!(
        behavior.extract_parameter_type(signature, "retrier"),
        Some("RequestRetrier".to_string())
    );
}

#[test]
fn inout_parameter_binds() {
    let behavior = SwiftBehavior::new();
    let signature = "func f(x: inout Session)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "x"),
        Some("Session".to_string())
    );
}

#[test]
fn collection_parameter_is_out_of_scope() {
    let behavior = SwiftBehavior::new();
    let signature = "func f(items: [Item])";
    assert_eq!(behavior.extract_parameter_type(signature, "items"), None);
}
