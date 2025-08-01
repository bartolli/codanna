# Clippy Fix Plan - Progress Tracking

This document tracks the systematic fixing of clippy warnings in the codebase. Each fix should be tested before proceeding to the next.

## Testing Requirements

After **EACH** fix:
1. Build: `cargo build --lib -p codanna`
2. Test: `cargo test` (or module-specific tests)
3. Verify: `cargo clippy --lib -p codanna -- -W clippy::all`
4. Commit: Make a small commit for the fix

## Progress Tracking

### Phase 1A: Automated Low-Risk Fixes (clippy --fix)

Run once for all: `cargo clippy --fix --lib -p codanna --allow-dirty`

- [x] **Unnecessary cast** - `src/storage/persistence.rs:33`
  - Remove: `indexer.file_count() as u32` → `indexer.file_count()`
  
- [x] **Clone on Copy type** - `src/storage/tantivy.rs:683`
  - Remove: `value.clone()` → `value`
  
- [x] **Needless borrows** - `src/storage/tantivy.rs:754, 1447`
  - Remove: `&format!("{:?}", kind)` → `format!("{:?}", kind)`
  
- [x] **Redundant closure** - `src/parsing/factory.rs:38`
  - Simplify: `|e| IndexError::General(e)` → `IndexError::General`
  
- [x] **Use or_default()** - `src/indexing/resolver.rs:60`
  - Replace: `or_insert_with(Vec::new)` → `or_default()`
  
- [x] **Use first()** - `src/indexing/simple.rs:1388`
  - Replace: `candidates.get(0)` → `candidates.first()`
  
- [x] **Use is_some_and()** - `src/indexing/walker.rs:68`
  - Replace: `map_or(false, |ft| ft.is_file())` → `is_some_and(|ft| ft.is_file())`
  
- [x] **Format in format args** - `src/mcp/mod.rs:394`
  - Combine nested format! calls (manually fixed)
  
- [x] **Unused enumerate index** - `src/vector/clustering.rs:202`
  - Remove enumerate when index isn't used

**Test after Phase 1A**: `cargo test` - Note: 2 test failures exist (unrelated to clippy fixes)

### Phase 1B: Manual Default Implementations

Fix one at a time, test after each:

- [x] **Add Default for ImportResolver** - `src/indexing/resolver.rs:40`
  - Added by cargo clippy --fix
  
- [x] **Add Default for TantivyTransaction** - `src/indexing/simple.rs:24`
  - Added by cargo clippy --fix
  
- [x] **Add Default for FileTransaction** - `src/indexing/transaction.rs:62`
  - Added by cargo clippy --fix
  
- [x] **Derive Default for RelationshipMetadata** - `src/relationship/mod.rs:107`
  - Changed to derive by cargo clippy --fix

### Phase 1C: Manual String Operations

- [x] **Strip suffix fixes** - `src/indexing/resolver.rs:179, 187`
  - Line 179: Use `path_str.strip_suffix(".rs")` ✓
  - Line 187: Use `path_without_ext.strip_suffix("/mod")` ✓
  - Test: `cargo test -- indexing::resolver`

### Phase 2: Medium Risk - Careful Review Required

- [ ] **FromStr trait** - `src/types/mod.rs:123`
  - Implement proper `FromStr` trait instead of method
  - Update callers if needed
  - Test: `cargo test -- types`

- [ ] **PartialOrd fix** - `src/vector/types.rs:213`
  - Change to: `Some(self.cmp(other))`
  - Verify Score ordering behavior first
  - Test: `cargo test -- vector::types`

- [ ] **Identical if blocks** - `src/indexing/resolver.rs:196-201`
  - Extract helper function: `is_crate_root(module_path: &str) -> bool`
  - Simplify if-else chain
  - Test: `cargo test -- indexing::resolver`

### Phase 3: High Risk - Architectural Changes

- [ ] **Box large errors** - `src/config.rs:182, 275`
  - Change to: `Result<Self, Box<figment::Error>>`
  - Update all callers
  - Test: Full suite `cargo test`
  - Check: `cargo test -- config`

- [ ] **Recursive parameter** - `src/storage/graph.rs:170`
  - Analyze if false positive
  - If needed, add: `#[allow(clippy::only_used_in_recursion)]`
  - Add justification comment
  - Test: `cargo test -- storage::graph`

- [ ] **Recursive parameter** - `src/parsing/rust.rs:579`
  - Analyze if false positive
  - If needed, add: `#[allow(clippy::only_used_in_recursion)]`
  - Add justification comment
  - Test: `cargo test -- parsing::rust`

## Module-Specific Test Commands

- Storage: `cargo test -- storage::`
- Vector: `cargo test -- vector::`
- Indexing: `cargo test -- indexing::`
- Parsing: `cargo test -- parsing::`
- Config: `cargo test -- config::`
- MCP: `cargo test -- mcp::`

## Rollback Strategy

If any test fails:
1. Immediately revert the change
2. Investigate why the fix broke functionality
3. Document in this file why the warning can't be fixed
4. Add `#[allow(clippy::...)]` with explanation

## Guidelines Compliance

Per `docs/development/guidelines.md`:
- Guideline 7: "**MUST** fix all warnings from `cargo clippy -- -W clippy::all`"
- All warnings must be addressed before merging
- Use `#[allow(...)]` only with clear justification

## Notes

- Total warnings: 24 (excluding the one already fixed)
- Estimated time: 2-3 hours with thorough testing
- Commit after each logical group of fixes
- Keep this document updated as fixes are completed