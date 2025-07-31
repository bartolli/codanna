# Fix Plan: Relationship Extraction Accuracy

## Problem
The relationship extraction is creating false positive relationships between symbols with the same name across different files/modules.

## Example
When `warm_cluster_cache` is found in a file, it creates relationships with ALL methods that might call something named `warm_cluster_cache`, even if they're in completely different modules.

## Root Cause
```rust
// In add_relationships_by_name
for from_symbol in &from_symbols {
    if from_symbol.file_id == file_id {
        for to_symbol in &to_symbols {
            // This creates relationships with ALL symbols named 'to_name'
            self.add_relationship_internal(from_symbol.id, to_symbol.id, Relationship::new(kind))?;
        }
    }
}
```

## Proposed Fix

### Phase 1: Immediate Improvements
1. **Add module path filtering**:
   ```rust
   // Prefer symbols in the same module
   let same_module_symbols: Vec<_> = to_symbols.iter()
       .filter(|s| s.module_path == from_symbol.module_path)
       .collect();
   
   // If found in same module, use only those
   let target_symbols = if !same_module_symbols.is_empty() {
       same_module_symbols
   } else {
       &to_symbols
   };
   ```

2. **Add proximity scoring**:
   - Same file: highest priority
   - Same module: high priority  
   - Same crate: medium priority
   - External: lowest priority

3. **Filter by symbol kind compatibility**:
   - Methods can only call functions/methods
   - Structs can only implement traits
   - etc.

### Phase 2: Proper Symbol Resolution
1. **Track imports/use statements**:
   ```rust
   struct ImportContext {
       direct_imports: HashMap<String, SymbolId>,
       glob_imports: Vec<String>,
       module_path: String,
   }
   ```

2. **Build resolution context per file**:
   - Parse use statements
   - Track module declarations
   - Build visibility map

3. **Resolve symbols using context**:
   - Check local scope first
   - Check imports
   - Check module hierarchy
   - Fall back to global search

## Implementation Priority
1. Add module path filtering (quick win)
2. Add symbol kind compatibility checks
3. Implement full import tracking (longer term)

## Testing Strategy
1. Create test with same-named functions in different modules
2. Verify only correct relationships are created
3. Test cross-module references with proper imports