# C/C++ Language Support

This document describes the C/C++ language support in codanna.

## Features

### C Language Support
- Symbol extraction for functions, structs, and enums
- Function call tracking
- Import/include statement parsing
- Variable and macro definition tracking
- Variable usage tracking

### C++ Language Support
- Symbol extraction for functions, classes, structs, and enums
- Function call tracking
- Method implementation tracking (Class::method syntax)
- Inheritance relationship detection
- Import/include statement parsing
- Variable and macro definition tracking
- Variable usage tracking
- Variable type relationships
- Class method discovery

## Implementation Details

The C/C++ parsers are implemented using tree-sitter grammars:
- `tree-sitter-c` for C language parsing
- `tree-sitter-cpp` for C++ language parsing

Both parsers implement the `LanguageParser` trait and provide full integration with the codanna indexing system.

## Test Coverage

Comprehensive tests are included for both parsers:
- Basic functionality tests
- Symbol extraction tests
- Relationship tracking tests
- Import parsing tests