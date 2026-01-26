use codanna::parsing::LanguageParser;
use codanna::parsing::lua::LuaParser;

fn load_oop_fixture() -> &'static str {
    include_str!("../../fixtures/lua/oop.lua")
}

fn load_methods_fixture() -> &'static str {
    include_str!("../../fixtures/lua/methods.lua")
}

fn load_modules_fixture() -> &'static str {
    include_str!("../../fixtures/lua/modules.lua")
}

#[test]
fn test_lua_oop_fixture_inheritance_calls() {
    let code = load_oop_fixture();
    let mut parser = LuaParser::new().expect("Failed to create Lua parser");

    let calls = parser.find_calls(code);

    println!("OOP fixture calls:");
    for (caller, callee, range) in &calls {
        println!("  {} -> {} at line {}", caller, callee, range.start_line);
    }

    // Animal.new calls setmetatable
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "new" && *callee == "setmetatable"),
        "Animal.new should call setmetatable, got {calls:?}"
    );

    // Dog.new calls Animal.new (inheritance pattern)
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "new" && *callee == "new"),
        "Dog.new should call Animal.new for inheritance, got {calls:?}"
    );

    // Logger:getInstance calls print
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "getInstance" && *callee == "print"),
        "Logger:getInstance should call print, got {calls:?}"
    );

    // Module-level setmetatable calls for inheritance setup
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "<module>" && *callee == "setmetatable"),
        "Module level should have setmetatable calls for inheritance, got {calls:?}"
    );
}

#[test]
fn test_lua_methods_fixture_table_operations() {
    let code = load_methods_fixture();
    let mut parser = LuaParser::new().expect("Failed to create Lua parser");

    let calls = parser.find_calls(code);

    println!("Methods fixture calls:");
    for (caller, callee, range) in &calls {
        println!("  {} -> {} at line {}", caller, callee, range.start_line);
    }

    // Counter.new calls setmetatable
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "new" && *callee == "setmetatable"),
        "Counter.new should call setmetatable, got {calls:?}"
    );

    // Counter.create calls Counter.new
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "create" && *callee == "new"),
        "Counter.create should call Counter.new, got {calls:?}"
    );

    // StringBuilder:append calls table.insert
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "append" && *callee == "insert"),
        "StringBuilder:append should call table.insert, got {calls:?}"
    );

    // StringBuilder:appendLine calls table.insert (twice)
    let append_line_inserts: Vec<_> = calls
        .iter()
        .filter(|(caller, callee, _)| *caller == "appendLine" && *callee == "insert")
        .collect();
    assert!(
        append_line_inserts.len() >= 2,
        "StringBuilder:appendLine should call table.insert at least twice, got {calls:?}"
    );

    // StringBuilder:toString calls table.concat
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "toString" && *callee == "concat"),
        "StringBuilder:toString should call table.concat, got {calls:?}"
    );

    // Vector:length calls math.sqrt
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "length" && *callee == "sqrt"),
        "Vector:length should call math.sqrt, got {calls:?}"
    );

    // Vector:normalize calls self:length (method call)
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "normalize" && *callee == "length"),
        "Vector:normalize should call self:length, got {calls:?}"
    );

    // Vector.__add calls Vector.new
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "__add" && *callee == "new"),
        "Vector.__add should call Vector.new, got {calls:?}"
    );

    // Vector.__tostring calls string.format
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "__tostring" && *callee == "format"),
        "Vector.__tostring should call string.format, got {calls:?}"
    );
}

#[test]
fn test_lua_modules_fixture_function_calls() {
    let code = load_modules_fixture();
    let mut parser = LuaParser::new().expect("Failed to create Lua parser");

    let calls = parser.find_calls(code);

    println!("Modules fixture calls:");
    for (caller, callee, range) in &calls {
        println!("  {} -> {} at line {}", caller, callee, range.start_line);
    }

    // M.process calls _privateHelper
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "process" && *callee == "_privateHelper"),
        "M.process should call _privateHelper, got {calls:?}"
    );

    // M.formatOutput calls string.format and tostring
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "formatOutput" && *callee == "format"),
        "M.formatOutput should call string.format, got {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "formatOutput" && *callee == "tostring"),
        "M.formatOutput should call tostring, got {calls:?}"
    );

    // M.utils.split calls table.insert
    assert!(
        calls
            .iter()
            .any(|(caller, callee, _)| *caller == "split" && *callee == "insert"),
        "M.utils.split should call table.insert, got {calls:?}"
    );
}

#[test]
fn test_lua_call_count_in_fixtures() {
    let mut parser = LuaParser::new().expect("Failed to create Lua parser");

    // Test that we extract a reasonable number of calls from each fixture
    let oop_calls = parser.find_calls(load_oop_fixture());
    assert!(
        oop_calls.len() >= 5,
        "OOP fixture should have at least 5 calls, found {}",
        oop_calls.len()
    );

    let methods_calls = parser.find_calls(load_methods_fixture());
    assert!(
        methods_calls.len() >= 10,
        "Methods fixture should have at least 10 calls, found {}",
        methods_calls.len()
    );

    let modules_calls = parser.find_calls(load_modules_fixture());
    assert!(
        modules_calls.len() >= 3,
        "Modules fixture should have at least 3 calls, found {}",
        modules_calls.len()
    );
}

#[test]
fn test_lua_no_duplicate_calls() {
    let code = r#"
function test()
    print("hello")
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create Lua parser");
    let calls = parser.find_calls(code);

    // Should only have one call: test -> print
    assert_eq!(calls.len(), 1, "Should have exactly one call");
    assert_eq!(calls[0].0, "test");
    assert_eq!(calls[0].1, "print");
}

#[test]
fn test_lua_calls_preserve_line_numbers() {
    let code = r#"
function foo()
    bar()
end

function baz()
    qux()
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create Lua parser");
    let calls = parser.find_calls(code);

    // bar() is on line 2 (0-indexed: line 2)
    let bar_call = calls.iter().find(|(_, callee, _)| *callee == "bar");
    assert!(bar_call.is_some());
    assert_eq!(bar_call.unwrap().2.start_line, 2);

    // qux() is on line 6 (0-indexed: line 6)
    let qux_call = calls.iter().find(|(_, callee, _)| *callee == "qux");
    assert!(qux_call.is_some());
    assert_eq!(qux_call.unwrap().2.start_line, 6);
}
