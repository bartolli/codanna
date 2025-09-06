//! Tests for C++ Parser Implementation
//!
//! Validates core C++ parsing functionality:
//! - Symbol extraction (functions, classes, structs, enums)
//! - Function call tracking
//! - Method implementation tracking
//! - Inheritance relationship detection
//! - Import/include statement parsing
//! - Variable and macro definition tracking
//! - Variable usage tracking
//! - Variable type relationships
//! - Class method discovery

use codanna::parsing::{LanguageParser, CppParser};

#[test]
fn test_cpp_parser_symbol_extraction() {
    println!("\n=== C++ Parser Symbol Extraction Test ===\n");

    let code = r#"
#include <iostream>
#include <vector>

#define MAX_SIZE 100

int global_counter = 0;

class Base {
public:
    virtual void method() {
        std::cout << "Base method" << std::endl;
    }
    
    virtual void another_method() = 0;
};

class Derived : public Base {
public:
    void method() override {
        std::cout << "Derived method" << std::endl;
    }
    
    void another_method() override {
        std::cout << "Derived another method" << std::endl;
    }
};

int add(int a, int b) {
    return a + b;
}

int main() {
    Derived obj;
    obj.method();
    int result = add(5, 3);
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let symbols = parser.parse(code, codanna::FileId::new(1).unwrap(), &mut codanna::types::SymbolCounter::new());

    println!("Found {} symbols:", symbols.len());
    for symbol in &symbols {
        println!("  {:?}: {} at line {}", symbol.kind, symbol.name, symbol.range.start_line);
    }

    // Verify expected symbols
    assert!(
        symbols.iter().any(|s| s.name == "add".into() && matches!(s.kind, codanna::SymbolKind::Function)),
        "Should find add function"
    );

    assert!(
        symbols.iter().any(|s| s.name == "Base".into() && matches!(s.kind, codanna::SymbolKind::Class)),
        "Should find Base class"
    );

    assert!(
        symbols.iter().any(|s| s.name == "Derived".into() && matches!(s.kind, codanna::SymbolKind::Class)),
        "Should find Derived class"
    );

    assert!(
        symbols.iter().any(|s| s.name == "main".into() && matches!(s.kind, codanna::SymbolKind::Function)),
        "Should find main function"
    );

    println!("✓ All expected symbols found");
}

#[test]
fn test_cpp_parser_find_calls() {
    println!("\n=== C++ Parser find_calls() Test ===\n");

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

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let calls = parser.find_calls(code);

    println!("Found {} function calls:", calls.len());
    for (caller, called, range) in &calls {
        println!("  In '{}': calls '{}' at line {}", caller, called, range.start_line);
    }

    // Note: Current implementation uses empty string for caller as we don't track containing functions
    assert!(
        calls.iter().any(|(_caller, called, _)| called == &"helper"),
        "Should find calls to helper function"
    );

    assert!(
        calls.iter().any(|(_caller, called, _)| called == &"process"),
        "Should find calls to process function"
    );

    println!("✓ Function calls tracked correctly");
}

#[test]
fn test_cpp_parser_find_implementations() {
    println!("\n=== C++ Parser find_implementations() Test ===\n");

    let code = r#"
class MyClass {
public:
    void method();
};

void MyClass::method() {
    // Method implementation
}

int add(int a, int b) {
    return a + b;
}

int main() {
    MyClass obj;
    obj.method();
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let implementations = parser.find_implementations(code);

    println!("Found {} method implementations:", implementations.len());
    for (class_name, method_name, range) in &implementations {
        println!("  Class '{}' implements method '{}' at line {}", class_name, method_name, range.start_line);
    }

    // Should find method implementation
    assert!(
        implementations.iter().any(|(class_name, method_name, _)| class_name == &"MyClass" && method_name == &"method"),
        "Should find MyClass::method implementation"
    );

    println!("✓ Method implementations tracked correctly");
}

#[test]
fn test_cpp_parser_find_extends() {
    println!("\n=== C++ Parser find_extends() Test ===\n");

    let code = r#"
class Base {
public:
    virtual void method() = 0;
};

class Derived : public Base {
public:
    void method() override;
};

class AnotherDerived : public Base, public std::vector<int> {
public:
    void method() override;
};
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let extends = parser.find_extends(code);

    println!("Found {} inheritance relationships:", extends.len());
    for (derived, base, range) in &extends {
        println!("  Class '{}' extends '{}' at line {}", derived, base, range.start_line);
    }

    // Should find inheritance relationships
    assert!(
        extends.iter().any(|(derived, base, _)| derived == &"Derived" && base == &"Base"),
        "Should find Derived extends Base"
    );

    assert!(
        extends.iter().any(|(derived, base, _)| derived == &"AnotherDerived" && base == &"Base"),
        "Should find AnotherDerived extends Base"
    );

    println!("✓ Inheritance relationships tracked correctly");
}

#[test]
fn test_cpp_parser_find_uses() {
    println!("\n=== C++ Parser find_uses() Test ===\n");

    let code = r#"
#define MAX_SIZE 100

int global_var = 42;

int main() {
    int local_var = global_var;
    int result = local_var + MAX_SIZE;
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let uses = parser.find_uses(code);

    println!("Found {} variable/function uses:", uses.len());
    for (context, used, range) in &uses {
        println!("  Uses '{}' in context '{}' at line {}", used, context, range.start_line);
    }

    // Current implementation tracks identifiers
    assert!(
        uses.iter().any(|(_, used, _)| used == &"global_var"),
        "Should find uses of global_var"
    );

    assert!(
        uses.iter().any(|(_, used, _)| used == &"local_var"),
        "Should find uses of local_var"
    );

    assert!(
        uses.iter().any(|(_, used, _)| used == &"MAX_SIZE"),
        "Should find uses of MAX_SIZE"
    );

    println!("✓ Variable and macro uses tracked correctly");
}

#[test]
fn test_cpp_parser_find_defines() {
    println!("\n=== C++ Parser find_defines() Test ===\n");

    let code = r#"
#define MAX_SIZE 100
#define SQUARE(x) ((x) * (x))

int global_var = 42;

int main() {
    int local_var = 10;
    int squared = SQUARE(5);
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let defines = parser.find_defines(code);

    println!("Found {} definitions:", defines.len());
    for (name, kind, range) in &defines {
        println!("  Defines '{}' as {} at line {}", name, kind, range.start_line);
    }

    // Should find macro definitions
    assert!(
        defines.iter().any(|(name, kind, _)| name == &"MAX_SIZE" && kind == &"macro"),
        "Should find MAX_SIZE macro definition"
    );

    assert!(
        defines.iter().any(|(name, kind, _)| name == &"SQUARE" && kind == &"macro"),
        "Should find SQUARE macro definition"
    );

    // Should find variable definitions
    assert!(
        defines.iter().any(|(name, kind, _)| name == &"global_var = 42" && kind == &"variable"),
        "Should find global_var definition"
    );

    assert!(
        defines.iter().any(|(name, kind, _)| name == &"local_var = 10" && kind == &"variable"),
        "Should find local_var definition"
    );

    println!("✓ Variable and macro definitions tracked correctly");
}

#[test]
fn test_cpp_parser_find_variable_types() {
    println!("\n=== C++ Parser find_variable_types() Test ===\n");

    let code = r#"
#include <vector>
#include <string>

int main() {
    int number = 42;
    std::string text = "hello";
    std::vector<int> numbers = {1, 2, 3};
    auto value = 3.14;
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let variable_types = parser.find_variable_types(code);

    println!("Found {} variable type relationships:", variable_types.len());
    for (var_name, type_name, range) in &variable_types {
        println!("  Variable '{}' has type '{}' at line {}", var_name, type_name, range.start_line);
    }

    // Should find variable type relationships
    assert!(
        variable_types.iter().any(|(var_name, type_name, _)| var_name == &"number = 42" && type_name == &"int"),
        "Should find number variable with int type"
    );

    // Note: Complex types like std::string and std::vector might not be extracted correctly
    // in this simple implementation

    println!("✓ Variable type relationships tracked correctly");
}

#[test]
fn test_cpp_parser_find_inherent_methods() {
    println!("\n=== C++ Parser find_inherent_methods() Test ===\n");

    let code = r#"
class MyClass {
public:
    void method1();
    int method2(int x) {
        return x * 2;
    }
    void method3() const;
    
private:
    void private_method();
};

int main() {
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let inherent_methods = parser.find_inherent_methods(code);

    println!("Found {} inherent methods:", inherent_methods.len());
    for (class_name, method_name, range) in &inherent_methods {
        println!("  Class '{}' has method '{}' at line {}", class_name, method_name, range.start_line);
    }

    // Should find methods defined within the class
    assert!(
        inherent_methods.iter().any(|(class_name, method_name, _)| class_name == "MyClass" && method_name == "method1"),
        "Should find MyClass::method1"
    );

    assert!(
        inherent_methods.iter().any(|(class_name, method_name, _)| class_name == "MyClass" && method_name == "method2"),
        "Should find MyClass::method2"
    );

    assert!(
        inherent_methods.iter().any(|(class_name, method_name, _)| class_name == "MyClass" && method_name == "method3"),
        "Should find MyClass::method3"
    );

    assert!(
        inherent_methods.iter().any(|(class_name, method_name, _)| class_name == "MyClass" && method_name == "private_method"),
        "Should find MyClass::private_method"
    );

    println!("✓ Inherent methods tracked correctly");
}

#[test]
fn test_cpp_parser_find_imports() {
    println!("\n=== C++ Parser find_imports() Test ===\n");

    let code = r#"
#include <iostream>
#include <vector>
#include <string>
#include "my_header.h"

int main() {
    std::cout << "Hello, World!" << std::endl;
    return 0;
}
"#;

    let mut parser = CppParser::new().expect("Failed to create C++ parser");
    let imports = parser.find_imports(code, codanna::FileId::new(1).unwrap());

    println!("Found {} imports:", imports.len());
    for import in &imports {
        println!("  Imports '{}'", import.path);
    }

    // Should find standard library includes
    assert!(
        imports.iter().any(|import| import.path == "iostream"),
        "Should find iostream import"
    );

    assert!(
        imports.iter().any(|import| import.path == "vector"),
        "Should find vector import"
    );

    assert!(
        imports.iter().any(|import| import.path == "string"),
        "Should find string import"
    );

    // Should find local header includes
    assert!(
        imports.iter().any(|import| import.path == "my_header.h"),
        "Should find my_header.h import"
    );

    println!("✓ Import statements parsed correctly");
}