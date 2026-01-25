use codanna::parsing::LanguageParser;
use codanna::parsing::lua::LuaParser;

#[test]
fn test_lua_basic_function_calls() {
    let code = r#"
function foo()
    bar()
    baz()
end

function main()
    foo()
    print("hello")
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    println!("Found {} calls:", calls.len());
    for (caller, callee, range) in &calls {
        println!("  {} -> {} at line {}", caller, callee, range.start_line);
    }

    // Verify specific calls
    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    assert!(
        call_pairs.contains(&("foo".to_string(), "bar".to_string())),
        "foo should call bar"
    );
    assert!(
        call_pairs.contains(&("foo".to_string(), "baz".to_string())),
        "foo should call baz"
    );
    assert!(
        call_pairs.contains(&("main".to_string(), "foo".to_string())),
        "main should call foo"
    );
    assert!(
        call_pairs.contains(&("main".to_string(), "print".to_string())),
        "main should call print"
    );
}

#[test]
fn test_lua_method_calls_colon_syntax() {
    let code = r#"
function MyClass:new()
    self:init()
    self:setup()
    return self
end

function MyClass:init()
    self.value = 0
end

function MyClass:setup()
    self:reset()
end

function MyClass:reset()
    self.value = 0
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    assert!(
        call_pairs.contains(&("new".to_string(), "init".to_string())),
        "new should call init"
    );
    assert!(
        call_pairs.contains(&("new".to_string(), "setup".to_string())),
        "new should call setup"
    );
    assert!(
        call_pairs.contains(&("setup".to_string(), "reset".to_string())),
        "setup should call reset"
    );
}

#[test]
fn test_lua_module_level_calls() {
    let code = r#"
-- Module-level calls
local json = require("json")
local config = loadConfig()
print("Module loaded")

function initialize()
    print("Initializing...")
    setupDatabase()
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    println!("Module-level calls:");
    for (caller, callee, _) in &calls {
        if caller == &"<module>" {
            println!("  <module> -> {}", callee);
        }
    }

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    // Module-level calls
    assert!(
        call_pairs.contains(&("<module>".to_string(), "require".to_string())),
        "Module should call require"
    );
    assert!(
        call_pairs.contains(&("<module>".to_string(), "loadConfig".to_string())),
        "Module should call loadConfig"
    );
    assert!(
        call_pairs.contains(&("<module>".to_string(), "print".to_string())),
        "Module should call print at module level"
    );

    // Function-level calls
    assert!(
        call_pairs.contains(&("initialize".to_string(), "print".to_string())),
        "initialize should call print"
    );
    assert!(
        call_pairs.contains(&("initialize".to_string(), "setupDatabase".to_string())),
        "initialize should call setupDatabase"
    );
}

#[test]
fn test_lua_dot_notation_calls() {
    let code = r#"
function process()
    table.insert(items, 1)
    table.remove(items, 1)
    math.sqrt(25)
    math.abs(-10)
    string.format("Hello %s", name)
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    // Verify we extract just the function name, not the table prefix
    assert!(
        call_pairs.contains(&("process".to_string(), "insert".to_string())),
        "Should extract 'insert' from table.insert"
    );
    assert!(
        call_pairs.contains(&("process".to_string(), "remove".to_string())),
        "Should extract 'remove' from table.remove"
    );
    assert!(
        call_pairs.contains(&("process".to_string(), "sqrt".to_string())),
        "Should extract 'sqrt' from math.sqrt"
    );
    assert!(
        call_pairs.contains(&("process".to_string(), "abs".to_string())),
        "Should extract 'abs' from math.abs"
    );
    assert!(
        call_pairs.contains(&("process".to_string(), "format".to_string())),
        "Should extract 'format' from string.format"
    );
}

#[test]
fn test_lua_nested_calls_in_arguments() {
    let code = r#"
function outer()
    print(string.upper(getName()))
    result = calculate(getValue(), getMax())
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    // All calls should be detected
    assert!(
        call_pairs.contains(&("outer".to_string(), "print".to_string())),
        "Should detect print call"
    );
    assert!(
        call_pairs.contains(&("outer".to_string(), "upper".to_string())),
        "Should detect string.upper call"
    );
    assert!(
        call_pairs.contains(&("outer".to_string(), "getName".to_string())),
        "Should detect getName call nested in arguments"
    );
    assert!(
        call_pairs.contains(&("outer".to_string(), "calculate".to_string())),
        "Should detect calculate call"
    );
    assert!(
        call_pairs.contains(&("outer".to_string(), "getValue".to_string())),
        "Should detect getValue call in arguments"
    );
    assert!(
        call_pairs.contains(&("outer".to_string(), "getMax".to_string())),
        "Should detect getMax call in arguments"
    );
}

#[test]
fn test_lua_anonymous_function_calls() {
    let code = r#"
function createHandler()
    return function(data)
        process(data)
        validate(data)
    end
end

local callback = function()
    notify()
    cleanup()
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    // Calls within anonymous function in createHandler should use createHandler as caller
    assert!(
        call_pairs.contains(&("createHandler".to_string(), "process".to_string())),
        "Anonymous function in createHandler should call process"
    );
    assert!(
        call_pairs.contains(&("createHandler".to_string(), "validate".to_string())),
        "Anonymous function in createHandler should call validate"
    );

    // Module-level anonymous function calls should use <module> as caller
    assert!(
        call_pairs.contains(&("<module>".to_string(), "notify".to_string())),
        "Module-level anonymous function should call notify"
    );
    assert!(
        call_pairs.contains(&("<module>".to_string(), "cleanup".to_string())),
        "Module-level anonymous function should call cleanup"
    );
}

#[test]
fn test_lua_constructor_patterns() {
    let code = r#"
function Animal.new(name)
    local self = setmetatable({}, Animal)
    self.name = name
    return self
end

function Dog.new(name, breed)
    local self = setmetatable(Animal.new(name), Dog)
    self.breed = breed
    return self
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    // Animal.new should be simplified to just "new"
    assert!(
        call_pairs.contains(&("new".to_string(), "setmetatable".to_string())),
        "Animal.new should call setmetatable"
    );

    // Dog.new should also be simplified to "new" and call both setmetatable and Animal.new
    let dog_new_calls: Vec<_> = calls.iter().filter(|(c, _, _)| *c == "new").collect();
    assert!(
        dog_new_calls.len() >= 2,
        "Dog.new should make at least 2 calls (setmetatable + nested)"
    );
}

#[test]
fn test_lua_method_chaining_detection() {
    let code = r#"
function StringBuilder:append(str)
    table.insert(self.parts, str)
    return self
end

function StringBuilder:build()
    result = self:append("a"):append("b"):toString()
    return result
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    assert!(
        call_pairs.contains(&("append".to_string(), "insert".to_string())),
        "append should call table.insert"
    );

    // Method chaining: each call in the chain is a separate function_call node
    assert!(
        call_pairs.contains(&("build".to_string(), "append".to_string())),
        "build should call append (at least once in chain)"
    );
    assert!(
        call_pairs.contains(&("build".to_string(), "toString".to_string())),
        "build should call toString at end of chain"
    );
}

#[test]
fn test_lua_calls_in_conditionals_and_loops() {
    let code = r#"
function validate(data)
    if isValid(data) then
        process(data)
    else
        handleError(data)
    end

    for i = 1, getCount() do
        doWork(i)
    end

    while hasMore() do
        fetchNext()
    end
end
"#;

    let mut parser = LuaParser::new().expect("Failed to create parser");
    let calls = parser.find_calls(code);

    let call_pairs: Vec<(String, String)> = calls
        .iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect();

    // All calls should be detected regardless of control flow structure
    assert!(
        call_pairs.contains(&("validate".to_string(), "isValid".to_string())),
        "Should detect call in if condition"
    );
    assert!(
        call_pairs.contains(&("validate".to_string(), "process".to_string())),
        "Should detect call in if block"
    );
    assert!(
        call_pairs.contains(&("validate".to_string(), "handleError".to_string())),
        "Should detect call in else block"
    );
    assert!(
        call_pairs.contains(&("validate".to_string(), "getCount".to_string())),
        "Should detect call in for loop condition"
    );
    assert!(
        call_pairs.contains(&("validate".to_string(), "doWork".to_string())),
        "Should detect call in for loop body"
    );
    assert!(
        call_pairs.contains(&("validate".to_string(), "hasMore".to_string())),
        "Should detect call in while condition"
    );
    assert!(
        call_pairs.contains(&("validate".to_string(), "fetchNext".to_string())),
        "Should detect call in while body"
    );
}
