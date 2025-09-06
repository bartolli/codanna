use codanna::parsing::{LanguageParser, CppParser};

#[test]
fn test_cpp_parser_basic() {
    let code = r#"
int main() {
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let symbols = parser.parse(code, codanna::FileId::new(1).unwrap(), &mut codanna::types::SymbolCounter::new());
    
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "main".into());
}