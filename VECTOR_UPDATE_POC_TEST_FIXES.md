# Vector Update POC Test Fixes

## Issue Summary

The `vector_update_poc_test.rs` file has type mismatch errors where `usize` values are being passed to `SymbolId::new()` which expects `u32`.

## Errors to Fix

### 1. Type Mismatches (10 occurrences)

**Problem**: `SymbolId::new()` expects `u32` but receiving `usize` from iterator indices.

**Locations**:
- Lines 593-594: `symbols.len() + 1` and `symbols.len() + 2`
- Lines 600-601: `old_symbols.len() + 1` and `old_symbols.len() + 2`
- Lines 606-607: `old_symbols.len() + 1` and `old_symbols.len() + 2`
- Lines 663, 665, 670, 672: `existing_count + 1` through `existing_count + 4`

**Fix**: Convert `usize` to `u32` using `try_into().unwrap()` or cast if safe:

```rust
// Before:
SymbolId::new(symbols.len() + 1)

// After:
SymbolId::new((symbols.len() + 1) as u32)
// OR (safer):
SymbolId::new((symbols.len() + 1).try_into().unwrap())
```

### 2. Unused Imports (2 warnings)

**Problem**: Unused imports that should be removed.

**Locations**:
- Line 13: `HashSet` is imported but not used
- Line 16: `Path` is imported but not used

**Fix**: Remove these imports:

```rust
// Line 13 - Remove HashSet from the import
use std::collections::{HashMap}; // Remove HashSet

// Line 16 - Remove entire line or just Path
use std::path::PathBuf; // Remove Path
```

## Recommended Fix Pattern

Since these are test files with controlled data sizes, using `as u32` is safe for the conversions. However, for production code, use `.try_into().unwrap()` for better error handling.

### Quick Fix Commands

You can use sed to fix all occurrences:

```bash
# Fix the type conversions (example for one pattern)
sed -i '' 's/SymbolId::new(symbols\.len() + \([0-9]\+\))/SymbolId::new((symbols.len() + \1) as u32)/g' tests/vector_update_poc_test.rs

# Remove unused imports
sed -i '' '13s/, HashSet//' tests/vector_update_poc_test.rs
sed -i '' '16s/, Path//' tests/vector_update_poc_test.rs
```

## Manual Fix Instructions

1. Open `tests/vector_update_poc_test.rs`
2. For each error on lines 593, 594, 600, 601, 606, 607, 663, 665, 670, 672:
   - Wrap the arithmetic expression in parentheses
   - Add `as u32` after the closing parenthesis
3. Remove `HashSet` from line 13's import
4. Remove `Path` from line 16's import
5. Run `cargo test --test vector_update_poc_test` to verify fixes

## Root Cause

The issue arises because:
- Collection `.len()` returns `usize` (architecture-dependent size)
- `SymbolId::new()` expects `u32` (fixed 32-bit size)
- Rust doesn't automatically convert between numeric types for safety

This is good type safety - it prevents potential overflow issues on 16-bit systems or when collections grow beyond u32::MAX.