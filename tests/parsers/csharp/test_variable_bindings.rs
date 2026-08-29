//! csharp local bindings: both capture arms agree on the token form.
//!
//! The initializer arm (`new List<T>()`) reduces to the base identifier;
//! the annotation arm must reduce identically — a raw `List<T>` or
//! `Session?` on one side of the name-equality join with ClassMember
//! class_name is a silent recall loss (the php sigil precedent).

use codanna::parsing::LanguageParser;
use codanna::parsing::csharp::CSharpParser;

fn bindings_for(code: &str) -> Vec<(String, String)> {
    let mut parser = CSharpParser::new().expect("csharp parser");
    parser
        .find_variable_types(code)
        .into_iter()
        .map(|(name, ty, _)| (name.to_string(), ty.to_string()))
        .collect()
}

#[test]
fn annotation_arm_reduces_like_initializer_arm() {
    let code = r#"
class W {
    void F() {
        List<Item> items = factory.Make();
        Session? maybe = GetSession();
        Foo.Bar b = GetBar();
        Helper h = new Helper();
    }
}
"#;
    assert_eq!(
        bindings_for(code),
        vec![
            ("items".to_string(), "List".to_string()),
            ("maybe".to_string(), "Session".to_string()),
            ("b".to_string(), "Bar".to_string()),
            ("h".to_string(), "Helper".to_string()),
        ]
    );
}
