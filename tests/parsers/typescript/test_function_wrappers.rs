//! Configurable function-wrapper detection (issue #115).
//!
//! `const X = wrapper(fn)` registers X as a Function and attributes the
//! wrapped body's calls to X when `wrapper` is declared via
//! `[languages.typescript.parser_options] function_wrappers`. Empty list
//! (the default) leaves behavior unchanged.

#[cfg(test)]
mod tests {
    use codanna::SymbolKind;
    use codanna::parsing::LanguageParser;
    use codanna::parsing::typescript::TypeScriptParser;
    use codanna::types::{FileId, SymbolCounter};

    const CODE: &str = r#"
function helper(x: number): number {
  return x + 1;
}

const plain = () => helper(1);

const View = memo(() => {
  const inner = () => helper(4);
  return helper(2);
});

const Input = React.forwardRef((props, ref) => helper(3));

const load = wrap("load")(function* () {
  yield helper(5);
});

const store = create((set) => set({}));
"#;

    fn wrappers() -> Vec<String> {
        vec![
            "memo".to_string(),
            "forwardRef".to_string(),
            "wrap".to_string(),
        ]
    }

    fn parse_kinds(parser: &mut TypeScriptParser) -> Vec<(String, SymbolKind)> {
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();
        parser
            .parse(CODE, file_id, &mut counter)
            .into_iter()
            .map(|s| (s.name.to_string(), s.kind))
            .collect()
    }

    fn kind_of(kinds: &[(String, SymbolKind)], name: &str) -> SymbolKind {
        kinds
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("symbol '{name}' not found"))
            .1
    }

    #[test]
    fn wrapper_bindings_register_as_functions() {
        let mut parser = TypeScriptParser::new()
            .expect("parser")
            .with_function_wrappers(wrappers());
        let kinds = parse_kinds(&mut parser);

        assert_eq!(kind_of(&kinds, "plain"), SymbolKind::Function);
        assert_eq!(kind_of(&kinds, "View"), SymbolKind::Function);
        // member-expression callee: React.forwardRef matches declared forwardRef
        assert_eq!(kind_of(&kinds, "Input"), SymbolKind::Function);
        // curried chain: wrap("load")(fn)
        assert_eq!(kind_of(&kinds, "load"), SymbolKind::Function);
        // create is NOT declared: stays a constant
        assert_eq!(kind_of(&kinds, "store"), SymbolKind::Constant);
    }

    #[test]
    fn wrapped_body_symbols_are_descended() {
        let mut parser = TypeScriptParser::new()
            .expect("parser")
            .with_function_wrappers(wrappers());
        let kinds = parse_kinds(&mut parser);

        // `inner` lives inside the memo-wrapped arrow body
        assert_eq!(kind_of(&kinds, "inner"), SymbolKind::Function);
    }

    #[test]
    fn empty_wrapper_list_leaves_behavior_unchanged() {
        let mut parser = TypeScriptParser::new().expect("parser");
        let kinds = parse_kinds(&mut parser);

        assert_eq!(kind_of(&kinds, "plain"), SymbolKind::Function);
        assert_eq!(kind_of(&kinds, "View"), SymbolKind::Constant);
        assert_eq!(kind_of(&kinds, "Input"), SymbolKind::Constant);
        assert_eq!(kind_of(&kinds, "load"), SymbolKind::Constant);
        assert_eq!(kind_of(&kinds, "store"), SymbolKind::Constant);
    }

    #[test]
    fn wrapped_body_calls_attribute_to_the_binding() {
        let mut parser = TypeScriptParser::new()
            .expect("parser")
            .with_function_wrappers(wrappers());
        let calls = parser.find_calls(CODE);

        let callers_of_helper: Vec<&str> = calls
            .iter()
            .filter(|(_, callee, _)| *callee == "helper")
            .map(|(caller, _, _)| *caller)
            .collect();

        assert!(
            callers_of_helper.contains(&"plain"),
            "direct arrow baseline"
        );
        assert!(
            callers_of_helper.contains(&"View"),
            "memo-wrapped arrow body call attributes to View; got {callers_of_helper:?}"
        );
        assert!(
            callers_of_helper.contains(&"Input"),
            "React.forwardRef-wrapped body call attributes to Input; got {callers_of_helper:?}"
        );
        assert!(
            callers_of_helper.contains(&"load"),
            "curried-wrapped generator body call attributes to load; got {callers_of_helper:?}"
        );
        // `inner` is a named arrow inside the wrapped body; its call
        // attributes to inner, not View
        assert!(callers_of_helper.contains(&"inner"));
    }

    #[test]
    fn dotted_wrapper_names_match_full_member_text() {
        let code = r#"
function helper(x: number): number { return x; }
const program = Effect.gen(function* () {
  yield helper(1);
});
const strict = Effect.fn("strict")(function* () {
  yield helper(2);
});
"#;
        let mut parser = TypeScriptParser::new()
            .expect("parser")
            .with_function_wrappers(vec!["Effect.gen".to_string(), "Effect.fn".to_string()]);
        let mut counter = SymbolCounter::new();
        let kinds: Vec<(String, SymbolKind)> = parser
            .parse(code, FileId::new(1).unwrap(), &mut counter)
            .into_iter()
            .map(|s| (s.name.to_string(), s.kind))
            .collect();

        assert_eq!(kind_of(&kinds, "program"), SymbolKind::Function);
        assert_eq!(kind_of(&kinds, "strict"), SymbolKind::Function);

        let calls = parser.find_calls(code);
        let callers: Vec<&str> = calls
            .iter()
            .filter(|(_, callee, _)| *callee == "helper")
            .map(|(caller, _, _)| *caller)
            .collect();
        assert!(
            callers.contains(&"program") && callers.contains(&"strict"),
            "dotted-wrapper bodies attribute to their bindings; got {callers:?}"
        );
    }

    #[test]
    fn dotted_wrapper_does_not_match_bare_property_calls() {
        let code = r#"
const loose = gen(function* () { return 1; });
"#;
        let mut parser = TypeScriptParser::new()
            .expect("parser")
            .with_function_wrappers(vec!["Effect.gen".to_string()]);
        let mut counter = SymbolCounter::new();
        let kinds: Vec<(String, SymbolKind)> = parser
            .parse(code, FileId::new(1).unwrap(), &mut counter)
            .into_iter()
            .map(|s| (s.name.to_string(), s.kind))
            .collect();

        assert_eq!(
            kind_of(&kinds, "loose"),
            SymbolKind::Constant,
            "declared Effect.gen must not match a bare gen(...) call"
        );
    }

    #[test]
    fn unconfigured_parser_leaves_wrapped_calls_unattributed() {
        let mut parser = TypeScriptParser::new().expect("parser");
        let calls = parser.find_calls(CODE);

        let callers_of_helper: Vec<&str> = calls
            .iter()
            .filter(|(_, callee, _)| *callee == "helper")
            .map(|(caller, _, _)| *caller)
            .collect();

        assert!(callers_of_helper.contains(&"plain"));
        assert!(
            !callers_of_helper.contains(&"View"),
            "default-off: wrapped calls must not attribute; got {callers_of_helper:?}"
        );
    }
}
