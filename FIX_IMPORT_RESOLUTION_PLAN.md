# Fix Import-Based Relationship Resolution Plan

## Overview
This document tracks the implementation of accurate import-based symbol resolution. The goal is to fix the ignored test `test_import_based_relationship_resolution` which verifies that cross-file symbol references respect import statements.

## Problem Statement
When multiple symbols have the same name across different files, the system currently cannot disambiguate which one is being referenced based on imports. This causes incorrect relationship tracking.

### Test Case Analysis
The ignored test creates:
- `config.rs`: defines `create_config() -> Config`
- `another.rs`: defines `create_config() -> Another`  
- `main.rs`: imports only `crate::config::create_config` and calls it

Expected: `main()` should link to `config::create_config`
Actual: Resolution fails or links incorrectly

## Root Causes
1. **Module paths not set on symbols** - Symbols are created without module path information
2. **Import resolution ignores module context** - Only matches on symbol name, not full path
3. **Resolution context incomplete** - Imported symbols not properly tracked in scope

## Implementation Plan

### Phase 1: Debug and Understand Current State ✅

#### Task 1.1: Add Debug Output to Import Extraction ✅
**File**: `src/parsing/rust.rs`
**Change**: Add debug prints in `extract_imports` to verify import parsing
**Test**: Run `cargo test test_import_extraction` and verify output
**Success**: All import patterns correctly extracted and printed
**Result**: Import extraction working correctly, debug output controlled by settings.debug

#### Task 1.2: Add Debug Output to Module Path Calculation ✅
**File**: `src/indexing/simple.rs`
**Change**: Add debug prints in `calculate_module_path` and where symbols are created
**Test**: Index a single file and verify module path is calculated
**Success**: Module paths printed for each file
**Result**: Found issue - path was relative, needed to make absolute for ImportResolver

#### Task 1.3: Create Minimal Test for Module Path Assignment ✅
**File**: `src/indexing/simple.rs` (tests section)
**Change**: Add test that verifies symbols get module paths after indexing
**Test**: `cargo test test_symbol_module_paths`
**Success**: Test passes, confirming issue
**Result**: Test created and passing

### Phase 2: Fix Module Path Assignment ✅

#### Task 2.1: Set Module Path on Symbols During Indexing ✅
**File**: `src/indexing/simple.rs`
**Change**: In `configure_symbol`, set module path if not already set
**Test**: Run minimal test from 1.3
**Success**: Symbols have module paths
**Result**: Already working, symbols get file module path

#### Task 2.2: Update Symbol Module Path Format ✅
**File**: `src/indexing/simple.rs`
**Change**: Set full qualified path on symbols (e.g., `crate::config::Config`)
```rust
symbol.module_path = Some(format!("{}::{}", mod_path, symbol.name).into());
```
**Test**: Index test file, check symbol module paths include symbol name
**Success**: Full paths like `crate::config::create_config`
**Result**: Symbols now have full qualified paths

#### Task 2.3: Test Module Path Assignment Works ✅
**File**: `src/indexing/simple.rs` (tests)
**Change**: Created test `test_symbol_module_paths` that verifies full paths
**Test**: `cargo test test_symbol_module_paths`
**Success**: All symbols have correct module paths
**Result**: Test passes with full qualified paths

### Phase 3: Fix Import Resolution Logic

#### Task 3.1: Update resolve_import_path to Use Full Path
**File**: `src/indexing/resolver.rs`
**Change**: In `resolve_import_path`, don't split path - pass full path to lookup
```rust
fn resolve_import_path(&self, path: &str, document_index: &DocumentIndex) -> Option<SymbolId> {
    eprintln!("DEBUG resolve_import_path: Resolving full path '{}'", path);
    
    // Try to find symbol with this exact module path
    let candidates = document_index.find_symbols_by_name(
        path.split("::").last().unwrap_or(path)
    ).ok()?;
    
    // Find one with matching module path
    candidates.into_iter()
        .find(|symbol| {
            symbol.module_path
                .as_ref()
                .map(|m| m.as_ref() == path)
                .unwrap_or(false)
        })
        .map(|s| s.id)
}
```
**Test**: Unit test for resolve_import_path
**Success**: Resolves `crate::config::create_config` correctly

#### Task 3.2: Fix build_resolution_context Import Resolution
**File**: `src/indexing/simple.rs`
**Change**: In `build_resolution_context`, line 1370, pass full import path
```rust
if let Some(symbol_id) = self.import_resolver.resolve_symbol(
    &import.path,  // Full path, not just last segment
    file_id,
    &self.document_index
)
```
**Test**: Debug print in resolution context to verify imports resolved
**Success**: Imported symbols added to context with correct IDs

#### Task 3.3: Update ImportResolver::resolve_symbol
**File**: `src/indexing/resolver.rs`
**Change**: In `resolve_symbol`, use full path for direct imports
```rust
// For direct imports, try full path first
if !import.is_glob {
    if let Some(symbol_id) = self.resolve_import_path(&import.path, document_index) {
        return Some(symbol_id);
    }
}
```
**Test**: Unit test for resolve_symbol with various import types
**Success**: Direct imports resolve correctly

### Phase 4: Fix Symbol Lookup

#### Task 4.1: Update find_symbol_in_module
**File**: `src/indexing/resolver.rs`
**Change**: Match against full module path, not just module prefix
**Test**: Unit test for find_symbol_in_module
**Success**: Finds symbols by full qualified path

#### Task 4.2: Add Integration Test for Import Resolution
**File**: `src/indexing/simple.rs` (tests)
**Change**: Add test similar to ignored one but simpler - just test resolution
**Test**: `cargo test test_simple_import_resolution`
**Success**: Import resolution works for simple case

### Phase 5: Enable and Fix Main Test

#### Task 5.1: Enable the Test with Debug Output
**File**: `src/indexing/simple.rs`
**Change**: Remove `#[ignore]`, add debug prints throughout test
**Test**: `cargo test test_import_based_relationship_resolution -- --nocapture`
**Success**: See where resolution fails

#### Task 5.2: Fix Any Remaining Issues ✅
**File**: Various based on debug output
**Change**: Fixed multiple issues:
- Added visibility detection in Rust parser
- Fixed visibility storage in Tantivy  
- Added batch indexing to avoid premature resolution
- Added import bypass for visibility check
- Fixed test tuple unpacking
**Test**: Run the main test
**Success**: Test passes
**Result**: Import-based relationship resolution now works correctly!

#### Task 5.3: Add More Edge Case Tests
**File**: `src/indexing/simple.rs` (tests)
**Change**: Add tests for:
- Aliased imports
- Glob imports
- Nested module imports
**Test**: Run all import tests
**Success**: All pass

## Success Metrics
1. Module paths are set on all symbols during indexing
2. Import resolution uses full qualified paths
3. The ignored test passes
4. No performance regression (indexing time stays similar)
5. Cross-file relationships correctly respect imports

## Testing Strategy
Each task has a specific test to verify the change works. We'll use:
- Unit tests for individual functions
- Integration tests for end-to-end flow
- Debug output to trace execution
- The existing ignored test as final validation

## Rollback Plan
Each change is small and isolated. If issues arise:
1. Revert the specific commit
2. Debug output helps identify the problem
3. Fix and re-apply with additional tests

## Notes
- Follow Rust coding guidelines (see CLAUDE.md)
- Keep changes minimal and focused
- Add debug output that can be controlled by settings.debug flag
- Ensure changes work conceptually for other languages