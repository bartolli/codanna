//! Tests for configurable higher-order function-wrapper detection.
//!
//! Some TypeScript codebases define functions through a higher-order wrapper
//! rather than a plain declaration, e.g. `const load = wrap(function* () {})`
//! or `const View = memo(() => {})`. Because the binding's value is a call
//! expression, the parser cannot know it is a function without being told which
//! wrappers to treat that way. The `[languages.typescript].parser_options`
//! `function_wrappers` list supplies those callee names. It is empty by default,
//! so this behavior is fully opt-in and ships knowing no framework.

#[cfg(test)]
mod tests {
    use codanna::parsing::LanguageParser;
    use codanna::parsing::typescript::TypeScriptParser;
    use codanna::types::{FileId, SymbolCounter, SymbolKind};

    fn parse_with_wrappers(code: &str, wrappers: &[&str]) -> Vec<codanna::Symbol> {
        let mut parser = TypeScriptParser::new().expect("Failed to create parser");
        parser.set_function_wrappers(wrappers.iter().map(|w| w.to_string()).collect());
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        parser.parse(code, file_id, &mut counter)
    }

    fn kind_of<'a>(symbols: &'a [codanna::Symbol], name: &str) -> Option<&'a SymbolKind> {
        symbols
            .iter()
            .find(|s| s.name.as_ref() == name)
            .map(|s| &s.kind)
    }

    /// A generator-wrapped binding (`const x = wrap(function* () {})`) is
    /// registered as a Function when its wrapper is configured.
    #[test]
    fn test_generator_wrapper_registers_function() {
        let code = r#"
const loadThing = trackedFn("loadThing")(function* (id: string) {
    return id;
});
"#;
        let symbols = parse_with_wrappers(code, &["trackedFn"]);
        assert_eq!(
            kind_of(&symbols, "loadThing"),
            Some(&SymbolKind::Function),
            "a generator-wrapped binding with a configured wrapper should be a Function"
        );
    }

    /// Control: with no wrappers configured the same binding is NOT a function.
    /// This proves the feature is opt-in and bakes in no framework knowledge.
    #[test]
    fn test_no_wrappers_is_stock_behavior() {
        let code = r#"
const loadThing = trackedFn("loadThing")(function* (id: string) {
    return id;
});
"#;
        let symbols = parse_with_wrappers(code, &[]);
        assert_ne!(
            kind_of(&symbols, "loadThing"),
            Some(&SymbolKind::Function),
            "with no configured wrappers, a wrapped binding must not be treated as a function"
        );
    }

    /// An arrow-bodied wrapper (`const make = wrap(() => { ... })`) is descended,
    /// so functions declared inside its body are indexed too. Arrows are only
    /// descended when the wrapper is configured, which is what keeps ordinary
    /// callbacks such as `arr.map(x => ...)` from being treated as definitions.
    #[test]
    fn test_arrow_wrapper_body_is_descended() {
        let code = r#"
const makeService = provide(() => {
    const inner = trackedFn("inner")(function* () {
        return 1;
    });
    return { inner };
});
"#;
        let symbols = parse_with_wrappers(code, &["provide", "trackedFn"]);
        assert_eq!(
            kind_of(&symbols, "makeService"),
            Some(&SymbolKind::Function),
            "the arrow-bodied wrapper binding should be a Function"
        );
        assert_eq!(
            kind_of(&symbols, "inner"),
            Some(&SymbolKind::Function),
            "a function nested inside the descended wrapper body should be indexed"
        );
    }

    /// The mechanism is framework-neutral: a plain identifier wrapper such as
    /// React's `memo` works the same way.
    #[test]
    fn test_identifier_wrapper_is_generic() {
        let code = r#"
const UserCard = memo(() => {
    return null;
});
"#;
        let symbols = parse_with_wrappers(code, &["memo"]);
        assert_eq!(
            kind_of(&symbols, "UserCard"),
            Some(&SymbolKind::Function),
            "a binding wrapped by a configured identifier wrapper should be a Function"
        );
    }

    /// A call whose callee is NOT in the list is never treated as a definition,
    /// even though it passes an arrow argument. Guards against false positives.
    #[test]
    fn test_unlisted_call_is_not_a_function() {
        let code = r#"
const ids = users.map((u) => u.id);
"#;
        let symbols = parse_with_wrappers(code, &["memo"]);
        assert_ne!(
            kind_of(&symbols, "ids"),
            Some(&SymbolKind::Function),
            "an ordinary call (users.map) must not be treated as a function definition"
        );
    }

    /// Calls made inside a wrapped function are attributed to that function, so
    /// caller/callee tracking works for wrapped definitions.
    #[test]
    fn test_calls_inside_wrapper_are_attributed() {
        let code = r#"
const loadThing = trackedFn("loadThing")(function* (id: string) {
    return id;
});

const useThing = trackedFn("useThing")(function* () {
    const value = yield* loadThing("x");
    return value;
});
"#;
        let mut parser = TypeScriptParser::new().expect("Failed to create parser");
        parser.set_function_wrappers(vec!["trackedFn".to_string()]);
        let calls = parser.find_calls(code);

        let attributed = calls
            .iter()
            .any(|(caller, called, _)| *caller == "useThing" && *called == "loadThing");
        assert!(
            attributed,
            "a call inside a wrapped function should be attributed to it (useThing -> loadThing); got: {calls:?}"
        );
    }
}
