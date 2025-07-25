// Example of how documentation extraction would work

use codanna::{Symbol, SymbolKind, SymbolId, FileId, Range};

fn main() {
    // Example 1: Function with rich documentation
    let process_batch_symbol = Symbol::new(
        SymbolId::new(1).unwrap(),
        "process_batch",
        SymbolKind::Function,
        FileId::new(1).unwrap(),
        Range::new(10, 0, 20, 1),
    )
    .with_signature("pub fn process_batch(items: &[Item]) -> Result<(), Error>")
    .with_doc(r#"Processes a batch of items efficiently.

This function takes a slice of items and processes them in parallel
using a thread pool. It's optimized for large batches.

# Arguments
* `items` - A slice of items to process

# Returns
* `Result<(), Error>` - Ok if successful, Err with details on failure

# Example
```
let items = vec![item1, item2, item3];
process_batch(&items)?;
```"#);

    // Example 2: Struct with documentation
    let config_symbol = Symbol::new(
        SymbolId::new(2).unwrap(),
        "Config",
        SymbolKind::Struct,
        FileId::new(1).unwrap(),
        Range::new(30, 0, 40, 1),
    )
    .with_doc("Configuration for the indexing system.\n\nControls parallelism, memory usage, and persistence options.");

    // Example 3: Method with brief doc
    let new_symbol = Symbol::new(
        SymbolId::new(3).unwrap(),
        "new",
        SymbolKind::Method,
        FileId::new(1).unwrap(),
        Range::new(45, 4, 47, 5),
    )
    .with_signature("pub fn new() -> Self")
    .with_doc("Creates a new Config with default values.");

    // When searching with Tantivy, we could find these by documentation content:
    // - Search: "parallel processing" -> finds process_batch
    // - Search: "configuration" -> finds Config struct
    // - Search: "default values" -> finds new method
    
    println!("Symbol: {}", process_batch_symbol.name);
    if let Some(doc) = &process_batch_symbol.doc_comment {
        println!("Documentation preview: {}", 
            doc.lines().take(2).collect::<Vec<_>>().join(" "));
    }
}

// Example of what MCP tool response would look like:
/*
Query: "find_symbol_with_docs" with args {"name": "process_batch"}

Response:
Found 1 symbol(s) named 'process_batch':
- Function at line 11 in file_id 1
  Signature: pub fn process_batch(items: &[Item]) -> Result<(), Error>
  Documentation: Processes a batch of items efficiently. This function takes a slice of items and processes them in parallel using a thread pool...
  
This rich context helps AI assistants understand:
1. What the function does
2. How to use it
3. What parameters it expects
4. What it returns
5. Example usage
*/