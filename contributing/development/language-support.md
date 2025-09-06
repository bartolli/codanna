# Adding Language Support

Languages self-register via the modular registry system. Each language lives in its own subdirectory with complete isolation and language-specific resolution capabilities.

**✅ Production Ready:**
- Language registry architecture with self-registration
- Language-specific resolution API with full type tracking
- Complete signature extraction for all symbol types
- Comprehensive scope context tracking with parent relationships

**✅ Supported Languages:**
- **Rust** - Traits, generics, lifetimes, comprehensive type system
- **TypeScript** - Interfaces, type aliases, generics, inheritance tracking
- **Python** - Classes, functions, type hints, inheritance
- **PHP** - Classes, traits, interfaces, namespaces
- **Go** - Structs, interfaces, methods, generics (1.18+), package visibility
- **C** - Functions, structs, enums
- **C++** - Classes, structs, functions, inheritance, templates

**🎯 Ready for new languages** - The architecture is mature and well-tested.

## Architecture

Each language needs:

1. `definition.rs` - implements `LanguageDefinition`
2. `parser.rs` - implements `LanguageParser`
3. `behavior.rs` - implements `LanguageBehavior`
4. `resolution.rs` - language-specific symbol resolution
5. `mod.rs` - module re-exports

## File Structure

Each language requires exactly 5 files in its own subdirectory:

```
src/parsing/{language}/
├── mod.rs        # Module re-exports and public API
├── parser.rs     # Symbol extraction, calls, implementations, imports
├── behavior.rs   # Module paths, visibility, basic language behaviors
├── resolution.rs # Language-specific symbol resolution logic
└── definition.rs # Language ID, extensions, factory methods
```

**Complete example (TypeScript):**
```
src/parsing/typescript/
├── mod.rs        # pub use TypeScriptParser, TypeScriptBehavior, register
├── parser.rs     # Extracts functions, classes, interfaces, types + signatures
├── behavior.rs   # Handles :: vs . separators, basic language behaviors
├── resolution.rs # TypeScript-specific symbol resolution and scoping
└── definition.rs # Language::TypeScript, [".ts", ".tsx"], create_parser()
```

## Key APIs for Language Implementation

### 1. LanguageDefinition (Registry Integration)

```rust
pub trait LanguageDefinition: Send + Sync {
    fn id(&self) -> LanguageId;                    // "rust", "typescript", etc.
    fn name(&self) -> &'static str;                // "Rust", "TypeScript", etc.
    fn extensions(&self) -> &'static [&'static str]; // ["rs"], ["ts", "tsx"], etc.
    fn create_parser(&self, settings: &Settings) -> IndexResult<Box<dyn LanguageParser>>;
    fn create_behavior(&self) -> Box<dyn LanguageBehavior>;
    fn default_enabled(&self) -> bool { true }     // Default config state
}
```

### 2. LanguageParser (Symbol Extraction)

```rust
pub trait LanguageParser: Send + Sync {
    // Core symbol extraction
    fn parse(&mut self, code: &str, file_id: FileId, counter: &mut SymbolCounter) -> Vec<Symbol>;
    
    // Relationship extraction  
    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)>;
    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall>;
    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)>;
    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import>;
    
    // Documentation extraction
    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String>;
    
    // Type erasure for dynamic dispatch
    fn as_any(&self) -> &dyn Any;
}
```

**Signature Extraction (Required:**
All parsers must extract complete signatures for all symbol types:
```rust
// Common pattern: exclude body, include full declaration
fn extract_signature(&self, node: Node, code: &str) -> String {
    let start = node.start_byte();
    let mut end = node.end_byte();
    if let Some(body) = node.child_by_field_name("body") {
        end = body.start_byte();
    }
    code[start..end].trim().to_string()
}
```

### 3. LanguageBehavior (Language-Specific Logic)

```rust
pub trait LanguageBehavior: Send + Sync {
    // Module path formatting
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String;
    fn module_separator(&self) -> &'static str;     // "::" vs "." vs "\\"
    
    // Visibility parsing
    fn parse_visibility(&self, signature: &str) -> Visibility;
    
    // Language capabilities
    fn supports_traits(&self) -> bool { false }
    fn supports_inherent_methods(&self) -> bool { false }
    
    // Symbol resolution
    fn resolve_symbol(&self, name: &str, context: &dyn ResolutionScope, 
                     document_index: &DocumentIndex) -> Option<SymbolId>;
    fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool;
    
    // Module configuration
    fn configure_symbol(&self, symbol: &mut Symbol, module_path: Option<&str>);
}
```

### 4. Key Data Types

```rust
pub struct Import {
    pub path: String,           // "std::collections::HashMap"
    pub alias: Option<String>,  // "as HashMap"
    pub file_id: FileId,
    pub is_glob: bool,          // "use foo::*"
    pub is_type_only: bool,     // TypeScript "import type"
}

pub enum MethodCall {
    Simple { receiver: String, method: String, range: Range },
    Chained { chain: Vec<String>, range: Range },
    Unknown { target: String, range: Range },
}
```

## Step 1: ABI-15 Node Discovery (Required)

Create a comprehensive ABI-15 exploration test before implementing any parser functionality. Tree-sitter node names frequently differ from language keywords (e.g., `abstract_class_declaration` not `abstract class`), making node discovery essential for correct implementation.

### Creating the Node Discovery Test

1. Add a comprehensive test to `tests/abi15_exploration.rs` that covers all language constructs
2. Document findings in `contributing/parsers/{language}/NODE_MAPPING.md`
3. Reference the node mapping document during implementation

### Running the Explorer

```bash
# Run your comprehensive language test
cargo test explore_{language}_abi15_comprehensive -- --nocapture > contributing/parsers/{language}/node_discovery.txt
```

### What to Discover

The explorer reveals:

- Exact node type names for all language constructs
- Field names for data extraction
- Node IDs for validation
- Parent-child relationships
- ABI version compatibility

### Example Discovery Output

```rust
#[test]
fn explore_typescript_abi15_comprehensive() {
    // Discovered that TypeScript uses:
    // - "class_declaration" for regular classes
    // - "abstract_class_declaration" for abstract classes (not "class_declaration" with modifier)
    // - "interface_declaration" for interfaces
    // - "type_alias_declaration" for type aliases
}
```

### Node Categories to Test

Every language test should explore:
- Class/struct declarations (including abstract, sealed, etc.)
- Interface/trait declarations
- Function/method declarations
- Variable/constant declarations
- Type definitions
- Import/export statements
- Module/namespace declarations
- Language-specific constructs (decorators, attributes, etc.)

## Implementation Checklist

1. **ABI-15 Node Discovery** - Run comprehensive tree-sitter exploration and document findings
2. **Create Language Directory** - `src/parsing/{language}/` with five required files:
   - `parser.rs` - Main parsing logic with signature extraction
   - `behavior.rs` - Language-specific behaviors 
   - `resolution.rs` - Language-specific symbol resolution logic
   - `definition.rs` - Registry definition with `register()` function
   - `mod.rs` - Module re-exports
3. **Registry Integration**:
   - Add to `src/parsing/registry.rs`: `super::{language}::register(registry);`
   - Update `src/parsing/mod.rs` with public module and re-exports
4. **Dependencies** - Add to `Cargo.toml`: `tree-sitter-{language} = "0.x"`
5. **Required Features**:
   - ✅ Symbol extraction with scope context
   - ✅ Complete signature extraction for all symbol types
   - ✅ Parent context tracking for nested symbols
   - ✅ Language-specific resolution logic
   - ✅ Comprehensive test coverage

## Performance Requirements

- **Target**: >10,000 symbols/second
- **Memory**: Use `&str` and `&code[node.byte_range()]`
- **IDs**: Use `SymbolCounter`, not raw `u32`

## Example Implementations

- **`src/parsing/rust/`** - Complete implementation with traits, generics, lifetimes, and signature extraction
- **`src/parsing/typescript/`** - Full TypeScript with interfaces, type aliases, inheritance tracking, and complex type resolution
- **`src/parsing/python/`** - Python with class inheritance, type hints, scope tracking, and parent context
- **`src/parsing/php/`** - PHP with namespaces, traits, interfaces, and complete signature support
- **`src/parsing/go/`** - Go with structs, interfaces, generics, methods, package-level visibility, and comprehensive symbol extraction

All parsers follow the same patterns for signature extraction, scope tracking, and resolution API integration.

## mod.rs Template

```rust
//! {Language} language parser implementation

pub mod behavior;
pub mod definition;
pub mod parser;

pub use behavior::{Language}Behavior;
pub use parser::{Language}Parser;
pub use definition::{Language}Language;

// Re-export for registry registration
pub(crate) use definition::register;
```
