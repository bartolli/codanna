use codanna::parsing::{LanguageParser, php::PhpParser};

#[test]
fn test_php_arrow_call_captures_receiver() {
    let code = r#"<?php
class Demo {
    public function run($obj, $x) {
        $obj->process($x);
    }
}
"#;
    let mut parser = PhpParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "process")
        .expect("$obj->process call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("$obj"));
    assert!(!call.is_static);
}

#[test]
fn test_php_scope_call_is_static_true() {
    let code = r#"<?php
function audit($msg) {
    Logger::info($msg);
}
"#;
    let mut parser = PhpParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "info")
        .expect("Logger::info call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("Logger"));
    assert!(call.is_static);
}

#[test]
fn test_php_top_level_method_call_caller_not_empty() {
    let code = r#"<?php
$service = new Service();
$service->run();
Service::boot();
"#;
    let mut parser = PhpParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let run = calls
        .iter()
        .find(|c| c.method_name == "run")
        .expect("top-level ->run() extracted");
    assert_eq!(
        run.caller, "<module>",
        "top-level member call carries <module>, never empty"
    );
    let boot = calls
        .iter()
        .find(|c| c.method_name == "boot")
        .expect("top-level ::boot() extracted");
    assert_eq!(
        boot.caller, "<module>",
        "top-level scoped call carries <module>, never empty"
    );
}
