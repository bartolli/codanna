use codanna::parsing::{LanguageParser, CParser};
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind};

#[test]
fn test_c_parser_symbol_extraction() {
    let code = r#"
#include <stdio.h>

#define MAX_SIZE 100

int global_var = 42;

int add(int a, int b) {
    return a + b;
}

struct Point {
    int x;
    int y;
};

enum Color {
    RED,
    GREEN,
    BLUE
};

int main() {
    int result = add(5, 3);
    return 0;
}
"#;

    let mut parser = CParser::new().expect("Failed to create C parser");
    let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());
    
    // Should find 4 symbols: add, Point, Color, main
    assert_eq!(symbols.len(), 4);
    
    // Check that we found the expected symbols
    let mut found_add = false;
    let mut found_point = false;
    let mut found_color = false;
    let mut found_main = false;
    
    for symbol in &symbols {
        if symbol.name == "add".into() && matches!(symbol.kind, SymbolKind::Function) {
            found_add = true;
        } else if symbol.name == "Point".into() && matches!(symbol.kind, SymbolKind::Struct) {
            found_point = true;
        } else if symbol.name == "Color".into() && matches!(symbol.kind, SymbolKind::Enum) {
            found_color = true;
        } else if symbol.name == "main".into() && matches!(symbol.kind, SymbolKind::Function) {
            found_main = true;
        }
    }
    
    assert!(found_add, "Should find add function");
    assert!(found_point, "Should find Point struct");
    assert!(found_color, "Should find Color enum");
    assert!(found_main, "Should find main function");
}

#[test]
fn test_c_parser_find_calls() {
    let code = r#"
int helper() {
    return 42;
}

int process() {
    return helper() * 2;
}

int main() {
    int result = process();
    int value = helper();
    return 0;
}
"#;

    let mut parser = CParser::new().expect("Failed to create C parser");
    let calls = parser.find_calls(code);
    
    // Should find calls to helper and process
    assert!(!calls.is_empty(), "Should find function calls");
    
    let mut found_helper = false;
    let mut found_process = false;
    
    for (_caller, called, _range) in &calls {
        if called == &"helper" {
            found_helper = true;
        } else if called == &"process" {
            found_process = true;
        }
    }
    
    assert!(found_helper, "Should find calls to helper function");
    assert!(found_process, "Should find calls to process function");
}

#[test]
fn test_c_parser_find_imports() {
    let code = r#"
#include <stdio.h>
#include <stdlib.h>
#include "my_header.h"

int main() {
    printf("Hello, World!\n");
    return 0;
}
"#;

    let mut parser = CParser::new().expect("Failed to create C parser");
    let imports = parser.find_imports(code, FileId::new(1).unwrap());
    
    // Should find 3 imports
    assert_eq!(imports.len(), 3);
    
    let mut found_stdio = false;
    let mut found_stdlib = false;
    let mut found_my_header = false;
    
    for import in &imports {
        if import.path == "stdio.h" {
            found_stdio = true;
        } else if import.path == "stdlib.h" {
            found_stdlib = true;
        } else if import.path == "my_header.h" {
            found_my_header = true;
        }
    }
    
    assert!(found_stdio, "Should find stdio.h import");
    assert!(found_stdlib, "Should find stdlib.h import");
    assert!(found_my_header, "Should find my_header.h import");
}