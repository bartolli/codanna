# Documentation Search Architecture

## Overview

This document outlines how we'll integrate documentation extraction and full-text search using Tantivy into the codebase intelligence system.

## Architecture Components

### 1. Documentation Extraction (During Parsing)

```rust
// In src/parsing/rust.rs
fn extract_doc_comment(node: &Node, source: &str) -> Option<String> {
    // Look for preceding comment nodes
    let mut comments = Vec::new();
    let mut current = node.prev_sibling();
    
    while let Some(sibling) = current {
        match sibling.kind() {
            "line_comment" => {
                let text = sibling.utf8_text(source.as_bytes())?;
                if text.starts_with("///") {
                    comments.push(text.trim_start_matches("///").trim());
                } else {
                    break; // Regular comment, not doc
                }
            }
            "block_comment" => {
                let text = sibling.utf8_text(source.as_bytes())?;
                if text.starts_with("/**") && !text.starts_with("/***") {
                    // Parse block comment
                    let cleaned = text.trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .trim();
                    comments.push(cleaned);
                }
            }
            _ => break,
        }
        current = sibling.prev_sibling();
    }
    
    if comments.is_empty() {
        None
    } else {
        // Reverse because we collected from bottom to top
        comments.reverse();
        Some(comments.join("\n"))
    }
}
```

### 2. Tantivy Index Structure

```rust
// src/search/doc_index.rs
use tantivy::{doc, Index, schema::*};

pub struct DocSearchIndex {
    index: Index,
    reader: IndexReader,
}

impl DocSearchIndex {
    pub fn create_schema() -> Schema {
        let mut schema_builder = Schema::builder();
        
        // Symbol identification
        schema_builder.add_u64_field("symbol_id", INDEXED | STORED);
        schema_builder.add_text_field("symbol_name", TEXT | STORED);
        schema_builder.add_text_field("symbol_kind", STRING | STORED);
        
        // Documentation fields
        schema_builder.add_text_field("doc_comment", TEXT | STORED);
        schema_builder.add_text_field("signature", TEXT | STORED);
        
        // File context
        schema_builder.add_u64_field("file_id", INDEXED | STORED);
        schema_builder.add_text_field("file_path", STRING | STORED);
        
        schema_builder.build()
    }
    
    pub fn index_symbol(&mut self, symbol: &Symbol, file_path: &str) {
        let schema = self.index.schema();
        
        let mut doc = Document::new();
        doc.add_u64(schema.get_field("symbol_id").unwrap(), symbol.id.value() as u64);
        doc.add_text(schema.get_field("symbol_name").unwrap(), &symbol.name);
        doc.add_text(schema.get_field("symbol_kind").unwrap(), &format!("{:?}", symbol.kind));
        
        if let Some(ref doc_comment) = symbol.doc_comment {
            doc.add_text(schema.get_field("doc_comment").unwrap(), doc_comment);
        }
        
        if let Some(ref signature) = symbol.signature {
            doc.add_text(schema.get_field("signature").unwrap(), signature);
        }
        
        doc.add_u64(schema.get_field("file_id").unwrap(), symbol.file_id.value() as u64);
        doc.add_text(schema.get_field("file_path").unwrap(), file_path);
        
        self.writer.add_document(doc);
    }
}
```

### 3. Enhanced MCP Tools with Documentation

```rust
// In src/mcp/mod.rs
#[tool(description = "Find symbols by documentation content")]
pub async fn search_docs(
    &self,
    Parameters(SearchDocsRequest { query, limit }): Parameters<SearchDocsRequest>,
) -> Result<CallToolResult, McpError> {
    let searcher = self.doc_index.searcher();
    let results = searcher.search(&query, limit)?;
    
    let mut response = String::new();
    for (score, doc_address) in results {
        let doc = searcher.doc(doc_address)?;
        let symbol_name = doc.get_first(symbol_name_field).unwrap();
        let doc_comment = doc.get_first(doc_comment_field).unwrap_or("No documentation");
        
        response.push_str(&format!(
            "- {} (score: {:.2})\n  {}\n\n",
            symbol_name, score, doc_comment
        ));
    }
    
    Ok(CallToolResult::success(vec![Content::text(response)]))
}
```

### 4. Rich Context for Existing Tools

When returning symbol information in MCP tools, include documentation:

```rust
// Enhanced find_symbol response
let mut result = format!("Found {} symbol(s) named '{}':\n", symbols.len(), name);
for symbol in symbols {
    result.push_str(&format!(
        "- {:?} at line {} in file_id {}\n", 
        symbol.kind, 
        symbol.range.start_line + 1,
        symbol.file_id.value()
    ));
    
    // Add documentation if available
    if let Some(ref doc) = symbol.doc_comment {
        result.push_str(&format!("  Documentation: {}\n", 
            doc.lines().take(3).collect::<Vec<_>>().join(" ")
        ));
    }
    
    // Add signature if available
    if let Some(ref sig) = symbol.signature {
        result.push_str(&format!("  Signature: {}\n", sig));
    }
}
```

## Integration Steps

1. **Update Parser**:
   - Extract doc comments during parsing
   - Store in Symbol struct

2. **Create Tantivy Index**:
   - Build schema for documentation search
   - Index symbols with their docs during indexing
   - Persist Tantivy index alongside symbol store

3. **Add Search Capabilities**:
   - Full-text search across documentation
   - Filter by symbol kind, file, etc.
   - Fuzzy matching for typos

4. **Enhance MCP Tools**:
   - Add `search_docs` tool
   - Include docs in all symbol responses
   - Add context snippets around symbols

## Benefits

1. **For AI Assistants**: 
   - Can search for functionality by description
   - Get rich context about what code does
   - Understand intent, not just structure

2. **For Developers**:
   - Find code by what it does, not just names
   - Discover related functionality
   - Better code understanding

## Example Usage

```bash
# Search for symbols by documentation
cargo run -- mcp search_docs --args '{"query": "parse JSON", "limit": 10}'

# Returns symbols with documentation mentioning JSON parsing
```

## Memory Considerations

- Tantivy indexes are memory-mapped, efficient for large codebases
- Can configure index size vs search speed tradeoffs
- Documentation strings are stored once, referenced by ID

## Language-Specific Documentation Extraction

### Rust
```rust
/// Processes a batch of items efficiently.
/// 
/// # Arguments
/// * `items` - A slice of items to process
/// 
/// # Returns
/// * `Result<(), Error>` - Ok if successful
pub fn process_batch(items: &[Item]) -> Result<(), Error> { ... }
```

### TypeScript/JavaScript (JSDoc)
```typescript
/**
 * Processes a batch of items efficiently.
 * @param {Item[]} items - Array of items to process
 * @returns {Promise<void>} Promise that resolves when complete
 * @throws {ProcessingError} If batch processing fails
 */
async function processBatch(items: Item[]): Promise<void> { ... }
```

### Python (Docstrings)
```python
def process_batch(items: List[Item]) -> None:
    """Process a batch of items efficiently.
    
    Args:
        items: List of items to process
        
    Raises:
        ProcessingError: If batch processing fails
    """
```

### Go
```go
// ProcessBatch processes a batch of items efficiently.
// It returns an error if the processing fails.
func ProcessBatch(items []Item) error { ... }
```

## Documentation Parsing Patterns

Each language parser will look for specific patterns:

1. **Rust**: `///`, `//!`, `/** */`
2. **TypeScript/JS**: `/** */` with JSDoc tags
3. **Python**: Triple quotes after function/class definition
4. **Go**: Comments directly above declarations

## Next Steps

1. Implement doc extraction in Rust parser
2. Create Tantivy index module  
3. Add search_docs MCP tool
4. Update existing tools to include documentation
5. Add tests for documentation extraction and search
6. Extend to other language parsers as they're added