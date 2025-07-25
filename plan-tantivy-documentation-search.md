# Plan: Integrate Tantivy for Documentation Search with MCP Tools

Based on my analysis, here's the enhanced plan to integrate Tantivy for documentation search with MCP
tool support:

Current State

- We already extract documentation comments (doc_comment field in Symbol)
- Tantivy is already in our dependencies (version 0.24.2)
- We have MCP server infrastructure in place
- We have a well-structured storage layer

Implementation Plan

1. Create Tantivy Storage Module (src/storage/tantivy.rs)

- Define rich schema with fields:
  - symbol_id: Stored field for reference
  - name: Text field with tokenizer for symbol names
  - doc_comment: Text field for full-text search
  - signature: Text field for function signatures
  - module_path: Facet field for filtering by module
  - kind: Facet field for filtering by symbol type
  - file_path: Stored field for file location
  - line_number: Stored field for exact location
  - context: Text field for surrounding code context

2. Update Storage Module

- Add the new tantivy module to src/storage/mod.rs
- Create DocumentIndex struct to manage Tantivy operations
- Support both exact and fuzzy search modes

3. Integrate with SimpleIndexer

- Add DocumentIndex to SimpleIndexer
- Update index_file to add rich documents to Tantivy
- Include code context (few lines before/after) for better understanding
- Update remove_file_symbols to maintain index consistency

4. Add MCP Tools for AI Search

// New MCP tools in src/mcp/mod.rs #[tool(description = "Search documentation and code comments for understanding APIs and functionality")]
pub async fn search_documentation(
query: String,
filters: Option<SearchFilters>,
) -> SearchResults {
// Returns rich results with:
// - Symbol name and type
// - Full file path and line number (e.g., "src/parser/rust.rs:125")
// - Documentation text with highlighted matches
// - Function signature if applicable
// - Module context
// - Related symbols (e.g., trait implementations)
}

#[tool(description = "Find code by semantic meaning, not just text matching")]
pub async fn semantic_code_search(
query: String,
context_lines: Option<usize>,
) -> SemanticSearchResults {
// Returns:
// - Matching code snippets with context
// - Relevance scores
// - File locations for navigation
// - Symbol relationships
}

#[tool(description = "Get detailed context around a specific symbol or location")]
pub async fn get_code_context(
file_path: String,
line: usize,
radius: Option<usize>,
) -> CodeContext {
// Returns:
// - Code snippet with configurable radius
// - All symbols defined/used in the context
// - Documentation for those symbols
// - Import statements affecting the context
}

5. Rich Search Result Structure

pub struct SearchResult {
pub symbol_id: SymbolId,
pub name: String,
pub kind: SymbolKind,
pub file_path: String,
pub line: u32,
pub column: u16,
pub doc_comment: Option<String>,
pub signature: Option<String>,
pub module_path: String,
pub score: f32,
pub highlights: Vec<TextHighlight>,
pub context: CodeContext,
}

pub struct CodeContext {
pub before: Vec<String>, // Lines before
pub target: String, // The matching line
pub after: Vec<String>, // Lines after
pub imports: Vec<String>, // Relevant imports
pub related_symbols: Vec<SymbolReference>,
}

6. Search Features

- Fuzzy matching for typo tolerance
- Semantic ranking based on:
  - Documentation relevance
  - Symbol importance (public APIs ranked higher)
  - Usage frequency
  - Recency
- Context-aware search that understands:
  - "find JSON parsing" → finds serde, json modules
  - "error handling" → finds Result, Error types
  - "async file operations" → finds tokio::fs functions

7. CLI Integration

# Search documentation

codebase-intelligence search docs "parse json"

# Semantic code search

codebase-intelligence search code "error handling in async context"

# Get context around a location

codebase-intelligence context src/main.rs:150 --radius 10

Benefits for AI Assistants

1. Rich metadata - Every result includes file path, line numbers, and context
2. Semantic understanding - Search by meaning, not just keywords
3. Navigation support - Results include file_path:line format for easy reference
4. Context awareness - Understand surrounding code and relationships
5. Efficient exploration - Find relevant code without reading entire files

Testing Plan

- Unit tests for schema and indexing
- Integration tests with MCP tools
- Test search quality with real queries
- Benchmark search performance
- Test context extraction accuracy

This approach will make the codebase truly searchable and understandable for AI assistants, providing
rich context and precise locations for every piece of information.
