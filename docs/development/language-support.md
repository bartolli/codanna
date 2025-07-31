# Language Parser Interface

The entire relationship extraction system is language-agnostic. The architecture is highly modular:

Each language implements the LanguageParser trait:

pub trait LanguageParser: Send + Sync {
fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut u32) -> Vec<Symbol>;
fn find_calls(&mut self, code: &str) -> Vec<(String, String, Range)>;
fn find_implementations(&mut self, code: &str) -> Vec<(String, String, Range)>;
fn find_uses(&mut self, code: &str) -> Vec<(String, String, Range)>;
fn find_defines(&mut self, code: &str) -> Vec<(String, String, Range)>;
fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import>;
}

Adding a New Language

To add Python, TypeScript, or any tree-sitter supported language:

1. Create a parser (e.g., src/parsing/python.rs)
2. Implement the trait methods using tree-sitter queries
3. Register in ParserFactory
4. Add to Language enum

What's Language-Specific vs Generic

Language-Specific (in parsers):

- How to identify functions, classes, methods
- Import statement syntax
- What "implements" means (inheritance, interfaces, traits)
- Method call syntax

Generic (in indexing):

- ResolutionContext (scope-based resolution)
- TraitResolver (tracks type-to-trait mappings)
- Relationship storage and retrieval
- Import resolution logic
- Cross-file relationship resolution

The work we did today - ResolutionContext, TraitResolver, relationship resolution - will benefit ALL
languages. Each language parser just needs to extract the right information, and the generic system
handles the rest.

For example, Python's class Dog(Animal): would create an "implements" relationship, JavaScript's import
statements would feed into the same ImportResolver, and TypeScript's interfaces would use the same
defines relationship system.
