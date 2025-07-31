# Language Parser Interface

The entire relationship extraction system is language-agnostic. The architecture is highly modular, allowing easy addition of new languages while leveraging the sophisticated analysis infrastructure.

## Core Trait Definition

Each language implements the `LanguageParser` trait:

```rust
pub trait LanguageParser: Send + Sync {
    // Extract symbols (functions, methods, classes, etc.)
    fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut u32) -> Vec<Symbol>;
    
    // Extract documentation comments for symbols
    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String>;
    
    // Relationship extraction methods
    fn find_calls(&mut self, code: &str) -> Vec<(String, String, Range)>;
    fn find_implementations(&mut self, code: &str) -> Vec<(String, String, Range)>;
    fn find_uses(&mut self, code: &str) -> Vec<(String, String, Range)>;
    fn find_defines(&mut self, code: &str) -> Vec<(String, String, Range)>;
    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import>;
    
    // Language identification
    fn language(&self) -> Language;
    
    // Advanced features (with default implementations)
    fn find_variable_types(&mut self, code: &str) -> Vec<(String, String, Range)> {
        Vec::new() // Languages can override for type tracking
    }
    
    // Enable downcasting for language-specific features
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

## Adding a New Language

To add Python, TypeScript, or any tree-sitter supported language:

### 1. Create Parser Module
Create `src/parsing/python.rs` (or appropriate language):

```rust
use tree_sitter::{Parser, Node};
use crate::parsing::{LanguageParser, Language};

pub struct PythonParser {
    parser: Parser,
}

impl PythonParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|e| format!("Failed to set Python language: {}", e))?;
        Ok(Self { parser })
    }
}
```

### 2. Implement Core Methods

#### Symbol Extraction
```rust
fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut u32) -> Vec<Symbol> {
    // Extract functions, classes, methods
    // Python: def, class, async def
    // TypeScript: function, class, interface, type
    // Go: func, type, interface
}
```

#### Relationship Detection
```rust
fn find_calls(&mut self, code: &str) -> Vec<(String, String, Range)> {
    // Python: function_call, method_call
    // TypeScript: call_expression, new_expression
    // For method calls, use receiver@method format for type resolution
}

fn find_implementations(&mut self, code: &str) -> Vec<(String, String, Range)> {
    // Python: class Dog(Animal) → ("Dog", "Animal", range)
    // TypeScript: class Dog implements Animal → ("Dog", "Animal", range)
    // Go: type Dog struct { Animal } → ("Dog", "Animal", range)
}
```

### 3. Register in ParserFactory
Update `src/parsing/mod.rs`:

```rust
match language {
    Language::Rust => Box::new(RustParser::new()?),
    Language::Python => Box::new(PythonParser::new()?),
    // Add new language here
}
```

### 4. Add to Language Enum
Update `src/types/language.rs`:

```rust
pub enum Language {
    Rust,
    Python,
    TypeScript,
    // Add new language
}
```

## What's Language-Specific vs Generic

### Language-Specific (in parsers)

#### Symbol Identification
- **Python**: `def`, `class`, `async def`, decorators
- **TypeScript**: `function`, `class`, `interface`, `type`, generics
- **Go**: `func`, `type`, `interface`, methods on types
- **Ruby**: `def`, `class`, `module`, singleton methods

#### Import Syntax
- **Python**: `import foo`, `from foo import bar`, `import foo as baz`
- **TypeScript**: `import {foo} from 'bar'`, `import * as foo`, dynamic imports
- **Go**: `import "foo"`, `import . "foo"`, aliased imports
- **Rust**: `use foo::bar`, `use foo as bar`, `use foo::*`

#### Inheritance/Implementation
- **Python**: `class Dog(Animal):` → inheritance
- **TypeScript**: `class Dog extends Animal implements Trainable`
- **Go**: Implicit interface implementation
- **Java**: `class Dog extends Animal implements Trainable`

#### Method Call Syntax
- **Python**: `obj.method()`, `super().method()`
- **TypeScript**: `obj.method()`, `obj?.method()`, `super.method()`
- **Go**: `obj.Method()`, method expressions `Type.Method`
- **Rust**: `obj.method()`, `Type::method()`, `<Type as Trait>::method()`

### Generic Infrastructure (in indexing)

#### ResolutionContext
- Scope-based symbol resolution
- Works identically for all languages
- Handles local variables, imports, module scope, global scope

#### TraitResolver (works for all inheritance systems)
- **Rust**: Tracks trait implementations
- **Python**: Tracks class inheritance
- **TypeScript**: Tracks interface implementations and class extensions
- **Go**: Tracks implicit interface satisfaction

#### Relationship Storage
- Language-agnostic relationship types (Calls, Implements, Uses, Defines)
- Tantivy storage works identically for all languages
- Cross-file relationship resolution is universal

#### Import Resolution
- ImportResolver handles all import styles
- Module path resolution adapts to language conventions
- Symbol visibility rules can be customized per language

#### Method Resolution
- Variable type tracking (if language parser provides it)
- Receiver tracking (`obj@method` format)
- Inherent vs trait/inherited method resolution

## Examples of Language Adaptation

### Python
```python
class Dog(Animal):           # → Implements relationship: Dog implements Animal
    def bark(self):          # → Defines relationship: Dog defines bark
        self.make_sound()    # → Calls relationship: bark calls make_sound
        super().eat()        # → Calls relationship: bark calls eat
```

### TypeScript
```typescript
class Dog extends Animal implements Trainable {  // → Two relationships
    bark(): void {                              // → Defines relationship
        this.makeSound();                       // → Calls with type tracking
    }
}
```

### Go
```go
type Dog struct {
    Animal                   // → Embedding creates "implements" relationship
}

func (d *Dog) Bark() {      // → Defines relationship: Dog defines Bark
    d.MakeSound()           // → Calls relationship with receiver tracking
}
```

## Key Insight

The sophisticated analysis infrastructure (ResolutionContext, TraitResolver, ImportResolver, relationship resolution) works for ALL languages. Each language parser only needs to:

1. Extract symbols with proper kinds
2. Identify relationships using language-specific syntax
3. Format method calls as `receiver@method` for type resolution
4. Extract imports in a common format

The generic system handles all the complex resolution, storage, and cross-file analysis, providing consistent, high-quality code intelligence across all supported languages.
