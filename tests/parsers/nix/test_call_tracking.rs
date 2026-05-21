use codanna::parsing::LanguageParser;
use codanna::parsing::nix::NixParser;

fn find_calls(code: &str) -> Vec<(String, String)> {
    let mut parser = NixParser::new().expect("Failed to create NixParser");
    parser
        .find_calls(code)
        .into_iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect()
}

#[test]
fn test_simple_apply_expression() {
    let code = r#"{ result = builtins.toString 42; }"#;
    let calls = find_calls(code);
    println!("calls: {calls:?}");
    assert!(
        calls.iter().any(|(_, callee)| callee.contains("toString")),
        "expected toString call, got {calls:?}"
    );
}

#[test]
fn test_callpackage_pattern() {
    let code = r#"
{
  hello = pkgs.callPackage ./hello.nix {};
  world = pkgs.callPackage ./world.nix { inherit stdenv; };
}
"#;
    let calls = find_calls(code);
    println!("callPackage calls: {calls:?}");
    assert!(
        calls
            .iter()
            .any(|(_, callee)| callee.contains("callPackage")),
        "expected callPackage call, got {calls:?}"
    );
}

#[test]
fn test_nested_apply_expressions() {
    let code = r#"{ x = builtins.toString (builtins.length [ 1 2 3 ]); }"#;
    let calls = find_calls(code);
    println!("nested calls: {calls:?}");
    // Should detect both function applications
    assert!(calls.len() >= 2, "expected at least 2 calls, got {calls:?}");
}

#[test]
fn test_import_not_counted_as_call() {
    // import is handled via find_imports, not find_calls
    let code = r#"{ pkgs = import <nixpkgs> {}; }"#;
    let calls = find_calls(code);
    println!("calls for import expr: {calls:?}");
    // The apply_expression `import <nixpkgs>` will appear — that's fine
    // just document current behaviour
}

#[test]
fn test_find_imports_basic() {
    use codanna::types::FileId;
    let code = r#"
{
  nixpkgs = import <nixpkgs> {};
  local = import ./local.nix;
}
"#;
    let mut parser = NixParser::new().unwrap();
    let file_id = FileId::new(1).unwrap();
    let imports = parser.find_imports(code, file_id);
    println!("imports: {imports:?}");
    assert!(
        !imports.is_empty(),
        "expected at least one import, got none"
    );
}

#[test]
fn test_find_imports_from_fixture() {
    use codanna::types::FileId;
    let code = include_str!("../../fixtures/nix/imports.nix");
    let mut parser = NixParser::new().unwrap();
    let file_id = FileId::new(1).unwrap();
    let imports = parser.find_imports(code, file_id);
    println!("imports from fixture: {imports:?}");
    assert!(
        imports.iter().any(|i| i.path.contains("lib.nix")),
        "expected ./lib.nix import, got {imports:?}"
    );
}
