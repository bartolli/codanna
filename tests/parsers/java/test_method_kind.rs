#[cfg(test)]
mod tests {
    use codanna::SymbolKind;
    use codanna::parsing::LanguageParser;
    use codanna::parsing::java::JavaParser;
    use codanna::symbol::ScopeContext;
    use codanna::types::{FileId, SymbolCounter};

    fn parse(code: &str) -> Vec<codanna::Symbol> {
        let mut parser = JavaParser::new().expect("Failed to create parser");
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        parser.parse(code, file_id, &mut counter)
    }

    #[test]
    fn test_instance_method_emits_method_kind() {
        let code = r#"
public class Owner {
    public String getName() { return this.name; }
}
"#;
        let symbols = parse(code);
        let sym = symbols
            .iter()
            .find(|s| s.name.as_ref() == "getName")
            .expect("getName must extract");
        assert_eq!(
            sym.kind,
            SymbolKind::Method,
            "java instance method must emit Method kind, got {:?}",
            sym.kind
        );
        match &sym.scope_context {
            Some(ScopeContext::ClassMember { class_name }) => {
                assert_eq!(
                    class_name.as_deref(),
                    Some("Owner"),
                    "method must carry named ClassMember scope"
                );
            }
            other => panic!("expected ClassMember scope, got {other:?}"),
        }
    }

    #[test]
    fn test_constructor_emits_method_kind() {
        let code = r#"
public class Owner {
    public Owner(String name) { this.name = name; }
}
"#;
        let symbols = parse(code);
        let sym = symbols
            .iter()
            .find(|s| s.name.as_ref() == "Owner" && s.kind != SymbolKind::Class)
            .expect("constructor must extract");
        assert_eq!(
            sym.kind,
            SymbolKind::Method,
            "java constructor must emit Method kind, got {:?}",
            sym.kind
        );
    }

    #[test]
    fn test_interface_method_emits_method_kind() {
        let code = r#"
public interface Repository {
    Owner findById(int id);
}
"#;
        let symbols = parse(code);
        let sym = symbols
            .iter()
            .find(|s| s.name.as_ref() == "findById")
            .expect("interface method must extract");
        assert_eq!(
            sym.kind,
            SymbolKind::Method,
            "java interface method must emit Method kind, got {:?}",
            sym.kind
        );
    }

    #[test]
    fn test_static_method_emits_method_kind() {
        let code = r#"
public class Owner {
    public static Owner of(String name) { return new Owner(name); }
}
"#;
        let symbols = parse(code);
        let sym = symbols
            .iter()
            .find(|s| s.name.as_ref() == "of")
            .expect("static method must extract");
        assert_eq!(sym.kind, SymbolKind::Method);
    }

    #[test]
    fn test_no_function_kind_symbols_from_class_file() {
        let code = r#"
public class Owner {
    private String name;
    public Owner(String name) { this.name = name; }
    public String getName() { return this.name; }
}
"#;
        let symbols = parse(code);
        let functions: Vec<&str> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .map(|s| s.name.as_ref())
            .collect();
        assert!(
            functions.is_empty(),
            "java has no free functions; Function-kind symbols are mis-kinds: {functions:?}"
        );
    }
}
