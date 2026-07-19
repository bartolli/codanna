use codanna::FileId;
use codanna::parsing::{LanguageParser, lua::LuaParser};
use codanna::symbol::ScopeContext;
use codanna::types::SymbolCounter;

#[test]
fn test_lua_dot_call_captures_receiver() {
    let code = r#"
local tbl = {}
function tbl.fn() end

local function caller()
    tbl.fn()
end
"#;
    let mut parser = LuaParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "fn")
        .expect("tbl.fn call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("tbl"));
    assert!(!call.is_static);
}

#[test]
fn test_lua_method_symbol_carries_classmember_scope() {
    let code = r#"
local tbl = {}
function tbl:method() end
"#;
    let mut parser = LuaParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    let symbols = parser.parse(code, FileId(1), &mut counter);
    let method = symbols
        .iter()
        .find(|s| s.name.as_ref() == "method")
        .expect("method symbol should be extracted");
    match &method.scope_context {
        Some(ScopeContext::ClassMember { class_name }) => {
            assert_eq!(class_name.as_deref(), Some("tbl"));
        }
        other => panic!(
            "expected ScopeContext::ClassMember{{ class_name: Some(\"tbl\") }}, got {other:?}"
        ),
    }
}

#[test]
fn test_lua_method_call_carries_enclosing_caller() {
    let code = r#"
local Vector = {}
function Vector.new() end

local function createObject()
    local v = Vector:new()
    return v
end

local top = Vector:new()
"#;
    let mut parser = LuaParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let inner = calls
        .iter()
        .find(|c| c.range.start_line == 5)
        .expect("call inside createObject extracted");
    assert_eq!(
        inner.caller, "createObject",
        "method call must carry the enclosing function name (plain-call walker identity)"
    );
    let module_level = calls
        .iter()
        .find(|c| c.range.start_line == 9)
        .expect("module-level call extracted");
    assert_eq!(
        module_level.caller, "<module>",
        "module-level method call matches the plain-call walker's <module> identity"
    );
}
