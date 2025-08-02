# MethodCall Enhancement - Sniper-Focused Implementation Plan

## Progress Status
- ✅ **Priority 1**: Parser Interface Enhancement - COMPLETE
  - ✅ 1.1: Added find_method_calls() to trait
  - ✅ 1.2: Implemented from_legacy_format()
- ✅ **Priority 2**: RustParser Enhancement - COMPLETE
  - ✅ 2.1: Override find_method_calls in RustParser
  - ✅ 2.2: Enhanced receiver detection with AST parsing
- ⏳ **Priority 3**: SimpleIndexer Integration - IN PROGRESS
  - ✅ 3.1: Update relationship processing to use find_method_calls()
  - ⏳ 3.2: Enhance method resolution logic
- 🔲 **Priority 4**: Storage Layer - TODO

**Last Updated**: 2025-08-02 - Priority 3.1 complete, enhanced detection integrated and working!

## Workflow
1. **Implementation Point** → Precise code change
2. **Unit Test Reference** → Use existing tests as guide
3. **Implementation** → Write the code
4. **Build** → `cargo build`
5. **Test** → Run tests + CLI verification
6. **Guidelines Check** → Verify Rust best practices
7. **Human Feedback** → Review & approve
8. **Next Task** → Move to next priority

## Priority 1: Parser Interface Enhancement (30 mins) ✅ COMPLETE

### 1.1 Add Method to LanguageParser Trait ✅
**Implementation Point**: `src/parsing/parser.rs:28` - Add new method after `find_calls()`
**Status**: COMPLETE - Added find_method_calls() with default implementation

**Unit Test Reference**: 
```rust
// From method_call.rs:217-287 - test_integration_with_parser_output
// Shows how to convert current tuples to MethodCall
```

**Task**:
```rust
/// Find method calls with rich receiver information
/// 
/// Default implementation converts from find_calls() for backward compatibility
fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
    self.find_calls(code).into_iter()
        .map(|(caller, target, range)| {
            // Parse patterns like existing test shows
            MethodCall::from_legacy_format(&caller, &target, range)
        })
        .collect()
}
```

**Verification**:
- [ ] `cargo check` passes
- [ ] Trait has default implementation (backward compatible)
- [ ] Uses `&str` parameters (zero-cost abstraction)

### 1.2 Implement MethodCall::from_legacy_format ✅
**Implementation Point**: `src/parsing/method_call.rs` - Add method to impl block
**Status**: COMPLETE - Method implemented with comprehensive test coverage

**Unit Test Reference**:
```rust
// From method_call.rs:229-287 - Parsing logic already tested
```

**Task**:
```rust
/// Parse legacy string patterns into MethodCall
/// Handles: "self.method", "Type::method", "receiver@method", "method"
pub fn from_legacy_format(caller: &str, target: &str, range: Range) -> Self {
    // Use logic from test_integration_with_parser_output
}
```

**CLI Test**:
```bash
# After implementation, test with:
cargo test method_call::from_legacy_format
```

## Priority 2: RustParser Enhancement (45 mins) ⏳ NEXT

### 2.1 Override find_method_calls in RustParser
**Implementation Point**: `src/parsing/rust.rs` - Add after `find_calls()`
**Status**: Ready to implement - foundation complete

**Unit Test Reference**:
```rust
// From rust.rs:1096-1118 - test_find_calls pattern
// Shows current call extraction logic
```

**Task**:
1. Extract method calls with receivers
2. Detect static vs instance calls
3. Preserve all context

**Verification**:
- [ ] Test with existing fixtures: `cargo test rust::test_find_calls`
- [ ] Compare output: both APIs should find same calls

### 2.2 Enhanced Receiver Detection
**Implementation Point**: Enhance existing call detection logic

**Unit Test Reference**:
```rust
// From method_call.rs:383-444 - test_priority_1_basic_method_calls
// Shows all patterns we need to detect
```

**Focus Areas**:
- `node.kind() == "field_expression"` → Extract receiver
- `node.kind() == "call_expression"` with `::` → Static call
- Track receiver types from variable_types map

## Priority 3: Integration with SimpleIndexer (1 hour)

### 3.1 Update Relationship Processing ✅
**Implementation Point**: `src/indexing/simple.rs:~800` - `extract_relationships_from_file`
**Status**: COMPLETE - Successfully integrated with debug logging

**Unit Test Reference**:
```rust
// Current: for (caller, target, range) in calls
// New: for method_call in parser.find_method_calls(content)
```

**Task**:
1. Use `find_method_calls()` instead of `find_calls()`
2. Convert back to tuples for now (gradual migration)
3. Add debug logging for receiver tracking

**CLI Verification**:
```bash
# Index a small file and check relationships
./target/release/codanna index tests/fixtures/simple.rs
./target/release/codanna retrieve relationships | grep "Calls"
```

### 3.2 Enhance Method Resolution
**Implementation Point**: `src/indexing/simple.rs:~1200` - `resolve_method_call`

**Current Logic to Enhance**:
```rust
// Check for receiver@method pattern
// Look up receiver's type
// Resolve using TraitResolver
```

**New Logic**:
- Use MethodCall struct directly
- Better static method handling
- Preserve receiver for all calls

## Priority 4: Storage Layer (Optional - Phase 2)

### 4.1 Relationship Storage Enhancement
**Implementation Point**: When ready to persist MethodCall data

**Consideration**: 
- Start with conversion to tuples
- Add new columns/fields later
- Maintain backward compatibility

## Testing Strategy

### Unit Tests First
1. Run existing tests: `cargo test method_call`
2. Run parser tests: `cargo test parsing::`
3. Run integration: `cargo test indexing::`

### CLI Integration Tests
```bash
# Create test file with known patterns
cat > test_method_calls.rs << 'EOF'
struct Data;
impl Data {
    fn process(&self) {}
    fn new() -> Self { Data }
}

fn main() {
    let data = Data::new();  // Static call
    data.process();          // Instance call  
    self.validate();         // Self call
}
EOF

# Index and verify
./target/release/codanna index test_method_calls.rs
./target/release/codanna retrieve calls | grep -E "(new|process|validate)"
```

### Performance Verification
```bash
# Before changes
time ./target/release/codanna index src/

# After changes  
time ./target/release/codanna index src/

# Should be within 5% of original
```

## Guidelines Checklist

Before each commit:
- [ ] Zero-cost abstractions: `&str` in APIs
- [ ] Error handling: Result types, no panics
- [ ] Type-driven: Use newtypes where sensible
- [ ] Functional style: Iterator chains over loops
- [ ] Documentation: Doc comments on public APIs
- [ ] Tests: Unit tests in same file

## Rollout Plan

### Day 1: Foundation (Today)
- [ ] Priority 1.1: Parser trait method
- [ ] Priority 1.2: Legacy format converter
- [ ] Priority 2.1: RustParser override
- [ ] Human Feedback checkpoint

### Day 2: Integration
- [ ] Priority 2.2: Enhanced detection
- [ ] Priority 3.1: SimpleIndexer integration
- [ ] Priority 3.2: Resolution enhancement
- [ ] Human Feedback checkpoint

### Day 3: Polish & Optimize
- [ ] Performance testing
- [ ] Edge case handling
- [ ] Documentation update
- [ ] Final review

## Success Metrics

1. **Functional**: All existing tests pass
2. **Performance**: <5% slowdown
3. **Accuracy**: Instance calls preserve receivers
4. **Compatibility**: Existing CLI commands work unchanged

## Quick Commands Reference

```bash
# Build
cargo build --release

# Test specific module
cargo test method_call --nocapture

# Test everything
cargo test

# Check without building
cargo check

# Run with debug
RUST_LOG=debug ./target/release/codanna index test.rs

# Benchmark
hyperfine './target/release/codanna index src/'
```

## Notes

- Start small: One method at a time
- Test continuously: After each function
- Keep backward compatibility: Critical for gradual migration
- Use debug prints: Verify transformations
- Commit often: Small, focused changes