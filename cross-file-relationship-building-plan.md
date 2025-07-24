# Cross-File Relationship Building Implementation Plan

Problem Analysis

Currently, the system only creates relationships between symbols within the same file due to this
constraint:
if caller.file_id == file_id {
// Only creates relationship if both symbols are in the same file
}

This limitation prevents us from understanding how code components interact across file boundaries,
which is critical for real-world codebases.

Implementation Plan

1. Add Module Path Tracking to Symbols

Files to modify: src/symbol/mod.rs, src/parsing/rust.rs

- Add module_path field to Symbol struct to store the full module path (e.g., crate::storage::memory)
- Update RustParser to capture module declarations and track current module context
- Extract and store import information (use statements)

2. Create Import Resolution System

New file: src/indexing/resolver.rs

- Create a ImportResolver that tracks:
  - File-to-module mappings
  - Import statements per file
  - Public/private visibility
- Implement module path resolution logic:
  - Handle relative paths (super::, self::)
  - Handle absolute paths (crate::, external crates)
  - Support glob imports (use foo::\*)

3. Update Relationship Building Logic

File to modify: src/indexing/simple.rs

- Remove the caller.file_id == file_id constraint
- After indexing all files, run a resolution pass:
  a. For each unresolved symbol reference
  b. Check imports in the current file
  c. Resolve to actual symbol using module paths
  d. Create cross-file relationships

4. Enhance Parser to Capture Use Statements

File to modify: src/parsing/rust.rs

- Add parsing for use_declaration nodes
- Track imported symbols and their paths
- Store import information for later resolution

5. Create Test Infrastructure

New files: Test fixtures with multi-file scenarios

- tests/fixtures/multi_file/mod.rs
- tests/fixtures/multi_file/sub_module.rs
- Tests for various import patterns

Detailed Steps

1. Update Symbol Structure
   - Add module_path: Option<String> field
   - Add visibility: Visibility enum (Public, Private, Crate)
   - Update serialization/deserialization
2. Enhance Rust Parser
   - Track current module context while parsing
   - Extract module declarations (mod foo;, mod foo { ... })
   - Parse use statements and store them separately
   - Build module hierarchy during parsing
3. Implement Resolution Logic
   - Create two-pass indexing:
     - Pass 1: Index all files, collect symbols and imports
   - Pass 2: Resolve imports and create cross-file relationships
   - Handle Rust's module resolution rules
4. Update SimpleIndexer
   - Add resolve_cross_file_relationships() method
   - Call after all files are indexed
   - Match unresolved references to actual symbols
5. Testing Strategy
   - Unit tests for module path parsing
   - Integration tests with multi-file projects
   - Test common patterns: workspace members, external crates

Benefits

- Complete understanding of codebase dependencies
- Accurate impact analysis across files
- Better support for refactoring
- More useful for AI assistants understanding project structure

Risks and Mitigations

- Performance: Resolution pass adds overhead
  - Mitigation: Use parallel processing, cache resolutions
- Complexity: Module resolution rules are complex
  - Mitigation: Start with basic cases, iterate
- Memory: Storing more data per symbol
  - Mitigation: Use compact representations

This implementation will enable the system to understand how code components interact across file
boundaries, making it much more useful for real-world codebases.
