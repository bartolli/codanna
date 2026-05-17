use codanna::parsing::{LanguageParser, csharp::CSharpParser};

#[test]
fn test_csharp_static_call_uppercase_receiver_is_static_true() {
    let code = r#"
public class Builder {
    public static string Build() {
        return Logger.Create();
    }
}
"#;
    let mut parser = CSharpParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "Create")
        .expect("Logger.Create call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("Logger"));
    assert!(
        call.is_static,
        "uppercase-leading receiver should mark is_static=true"
    );
}

#[test]
fn test_csharp_instance_call_lowercase_receiver_is_static_false() {
    let code = r#"
public class Processor {
    public void Run(Calculator calc) {
        calc.Add(1, 2);
    }
}
"#;
    let mut parser = CSharpParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "Add")
        .expect("calc.Add call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("calc"));
    assert!(
        !call.is_static,
        "lowercase-leading receiver should mark is_static=false"
    );
}
