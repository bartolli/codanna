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
