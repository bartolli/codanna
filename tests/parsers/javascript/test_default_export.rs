#[cfg(test)]
mod tests {
    use codanna::Visibility;
    use codanna::parsing::LanguageParser;
    use codanna::parsing::javascript::JavaScriptParser;
    use codanna::types::{FileId, SymbolCounter};

    #[test]
    fn test_inline_default_export_function_extracted() {
        let code = r#"
export default function RootLayout(props) { return renderShell(props); }
"#;
        let mut parser = JavaScriptParser::new().expect("Failed to create parser");
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(
            names.contains(&"RootLayout"),
            "inline `export default function` must extract. Got: {names:?}"
        );
        let sym = symbols
            .iter()
            .find(|s| s.name.as_ref() == "RootLayout")
            .unwrap();
        assert_eq!(
            sym.visibility,
            Visibility::Public,
            "default-exported function must be Public"
        );
    }

    #[test]
    fn test_inline_default_export_async_function_extracted() {
        let code = r#"
export default async function HomePage() { return findAllPosts(); }
"#;
        let mut parser = JavaScriptParser::new().expect("Failed to create parser");
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(
            names.contains(&"HomePage"),
            "inline `export default async function` must extract. Got: {names:?}"
        );
    }

    #[test]
    fn test_inline_default_export_class_extracted() {
        let code = r#"
export default class Widget { render() {} }
"#;
        let mut parser = JavaScriptParser::new().expect("Failed to create parser");
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(
            names.contains(&"Widget"),
            "inline `export default class` must extract. Got: {names:?}"
        );
    }

    #[test]
    fn test_split_default_export_unchanged() {
        let code = r#"
function WorksPage() { return sharedHelper(2); }
export default WorksPage;
"#;
        let mut parser = JavaScriptParser::new().expect("Failed to create parser");
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let matches: Vec<_> = symbols
            .iter()
            .filter(|s| s.name.as_ref() == "WorksPage")
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "split default export must produce exactly one symbol"
        );
        assert_eq!(matches[0].visibility, Visibility::Public);
    }

    #[test]
    fn test_anonymous_default_export_produces_no_symbol() {
        let code = r#"
export default function () { return sharedHelper(3); }
"#;
        let mut parser = JavaScriptParser::new().expect("Failed to create parser");
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(
            symbols.is_empty(),
            "anonymous default export has no name evidence; expected no symbols, got: {:?}",
            symbols.iter().map(|s| s.name.as_ref()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_inline_default_export_calls_attributed() {
        let code = r#"
export default function RootLayout(props) { return renderShell(props); }
"#;
        let mut parser = JavaScriptParser::new().expect("Failed to create parser");
        let calls = parser.find_calls(code);
        assert!(
            calls
                .iter()
                .any(|(from, to, _)| *from == "RootLayout" && *to == "renderShell"),
            "call inside inline default export must attribute to it. Got: {calls:?}"
        );
        let layout_calls = calls
            .iter()
            .filter(|(from, to, _)| *from == "RootLayout" && *to == "renderShell")
            .count();
        assert_eq!(
            layout_calls, 1,
            "exactly one call edge expected (no double-walk duplicates). Got: {calls:?}"
        );
    }
}
