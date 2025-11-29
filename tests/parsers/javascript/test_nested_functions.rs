#[cfg(test)]
mod tests {
    use codanna::parsing::LanguageParser;
    use codanna::parsing::javascript::JavaScriptParser;
    use codanna::types::{FileId, SymbolCounter};

    #[test]
    fn test_nested_function_extraction() {
        // Test that nested functions are properly extracted as symbols
        // Mirrors TypeScript test for React component support
        let code = r#"
// React component pattern with nested functions
const Component = () => {
    const handleClick = () => {
        console.log('clicked');
        toggleTheme();
    };

    const toggleTheme = () => {
        console.log('theme');
    };

    return { handleClick, toggleTheme };
};

// Regular function with nested function
function outer() {
    function inner() {
        console.log('inner');
    }
    inner();
}
"#;

        let mut parser = JavaScriptParser::new().expect("Failed to create parser");

        // Check symbol extraction
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        // Verify nested functions are extracted
        let symbol_names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();

        assert_eq!(
            symbols.len(),
            5,
            "Expected 5 symbols: Component, handleClick, toggleTheme, outer, inner. Got: {symbol_names:?}"
        );
        assert!(symbol_names.contains(&"Component"), "Missing Component");
        assert!(
            symbol_names.contains(&"handleClick"),
            "Missing nested handleClick"
        );
        assert!(
            symbol_names.contains(&"toggleTheme"),
            "Missing nested toggleTheme"
        );
        assert!(symbol_names.contains(&"outer"), "Missing outer");
        assert!(symbol_names.contains(&"inner"), "Missing nested inner");
    }

    #[test]
    fn test_nested_function_relationships() {
        // Test that relationships between nested functions are tracked
        let code = r#"
const App = () => {
    const doWork = () => {
        helperFunction();
    };

    const helperFunction = () => {
        console.log('helping');
    };

    doWork();
};
"#;

        let mut parser = JavaScriptParser::new().expect("Failed to create parser");

        // Check call tracking
        let calls = parser.find_calls(code);

        // Find the doWork -> helperFunction call
        let has_nested_call = calls
            .iter()
            .any(|(caller, callee, _)| *caller == "doWork" && *callee == "helperFunction");

        assert!(
            has_nested_call,
            "Should track doWork -> helperFunction call. Found calls: {calls:?}"
        );

        // Also check App calls doWork
        let has_parent_call = calls
            .iter()
            .any(|(caller, callee, _)| *caller == "App" && *callee == "doWork");

        assert!(
            has_parent_call,
            "Should track App -> doWork call. Found calls: {calls:?}"
        );
    }

    #[test]
    fn test_curried_arrow_functions() {
        // Test curried functions like: const multiply = (a) => (b) => a * b;
        let code = r#"
const multiply = (a) => (b) => a * b;
const add = (a) => (b) => a + b;
"#;

        let mut parser = JavaScriptParser::new().expect("Failed to create parser");

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let symbol_names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();

        // At minimum, we should extract the top-level const declarations
        assert!(
            symbol_names.contains(&"multiply"),
            "Missing multiply. Got: {symbol_names:?}"
        );
        assert!(
            symbol_names.contains(&"add"),
            "Missing add. Got: {symbol_names:?}"
        );
    }

    #[test]
    fn test_class_method_nested_functions() {
        // Test nested functions inside class methods
        let code = r#"
class UserService {
    async fetchUser(id) {
        const processResponse = (data) => {
            return data.user;
        };

        const response = await fetch(`/users/${id}`);
        return processResponse(await response.json());
    }

    createHandler() {
        const innerHandler = (event) => {
            console.log(event);
        };
        return innerHandler;
    }
}
"#;

        let mut parser = JavaScriptParser::new().expect("Failed to create parser");

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let symbol_names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();

        // Verify class, methods, and nested functions
        assert!(
            symbol_names.contains(&"UserService"),
            "Missing UserService class"
        );
        assert!(
            symbol_names.contains(&"fetchUser"),
            "Missing fetchUser method"
        );
        assert!(
            symbol_names.contains(&"createHandler"),
            "Missing createHandler method"
        );
        assert!(
            symbol_names.contains(&"processResponse"),
            "Missing nested processResponse. Got: {symbol_names:?}"
        );
        assert!(
            symbol_names.contains(&"innerHandler"),
            "Missing nested innerHandler. Got: {symbol_names:?}"
        );
    }
}
