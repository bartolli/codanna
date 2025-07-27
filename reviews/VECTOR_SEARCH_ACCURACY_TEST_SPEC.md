# Vector Search Accuracy Test Specification

## Test 2: Vector Search Accuracy Implementation

### Objective
Implement Test 2 from the Integration Test Plan to validate semantic search quality with real code examples and measurable accuracy metrics.

### Test File Location
Create: `tests/vector_search_accuracy_test.rs`

### Test Structure

```rust
use codanna::index::DocumentIndex;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

// Import POC components from vector_integration_test.rs
// Reuse VectorSearchEngine and test utilities

#[derive(Debug)]
struct SearchTestCase {
    query: &'static str,
    expected_symbols: Vec<&'static str>,  // Symbol names that MUST appear
    expected_keywords: Vec<&'static str>, // Keywords in relevant code
    min_precision: f32,  // Minimum acceptable precision
}

#[derive(Debug)]
struct SearchMetrics {
    precision: f32,
    recall: f32,
    mean_reciprocal_rank: f32,
    avg_rank_of_relevant: f32,
}
```

### Test Cases to Implement

1. **JSON Parsing Search**
   ```rust
   SearchTestCase {
       query: "parse JSON",
       expected_symbols: vec!["parse_json", "JsonParser", "from_json"],
       expected_keywords: vec!["serde", "json", "deserialize"],
       min_precision: 0.7,
   }
   ```

2. **Error Handling Search**
   ```rust
   SearchTestCase {
       query: "error handling",
       expected_symbols: vec!["handle_error", "ErrorKind", "Result"],
       expected_keywords: vec!["Result<", "Error", "thiserror", "anyhow"],
       min_precision: 0.65,
   }
   ```

3. **Async Function Search**
   ```rust
   SearchTestCase {
       query: "async function",
       expected_symbols: vec!["async_process", "spawn", "await"],
       expected_keywords: vec!["async fn", "tokio", ".await", "spawn"],
       min_precision: 0.75,
   }
   ```

4. **Builder Pattern Search**
   ```rust
   SearchTestCase {
       query: "builder pattern",
       expected_symbols: vec!["Builder", "build", "with_"],
       expected_keywords: vec!["self", "mut self", "build("],
       min_precision: 0.6,
   }
   ```

5. **Test Function Search**
   ```rust
   SearchTestCase {
       query: "unit tests",
       expected_symbols: vec!["test_", "#[test]"],
       expected_keywords: vec!["assert", "assert_eq", "#[cfg(test)]"],
       min_precision: 0.8,
   }
   ```

### Implementation Requirements

1. **Test Data Setup**
   - Use existing test fixtures in `tests/fixtures/`
   - Add new test files if needed for specific patterns
   - Ensure variety of code patterns represented

2. **Search Implementation**
   - Reuse VectorSearchEngine from Test 1
   - Implement semantic_search method if not present
   - Return top-10 results for each query

3. **Accuracy Metrics**
   ```rust
   impl SearchMetrics {
       fn calculate(results: &[SearchResult], test_case: &SearchTestCase) -> Self {
           // Precision: % of returned results that are relevant
           // Recall: % of relevant items that were found
           // MRR: 1/rank of first relevant result
           // Avg rank: average position of all relevant results
       }
   }
   ```

4. **Validation Logic**
   - Check if expected symbols appear in results
   - Verify code snippets contain expected keywords
   - Calculate and assert metrics meet thresholds
   - Print detailed results for debugging

### Code Quality Requirements

Following CLAUDE.md principles:

1. **Function Signatures**
   - Use `&str` for query inputs
   - Use `&[SearchResult]` for result processing
   - Return `Result<SearchMetrics, VectorTestError>`

2. **Error Handling**
   - Use thiserror for custom errors
   - Provide actionable error messages
   - Include search context in errors

3. **Type Safety**
   - Create newtype for `RelevanceScore(f32)`
   - Use `NonZeroU32` for result counts
   - Validate score ranges (0.0..=1.0)

### Test Organization

```rust
mod accuracy_tests {
    use super::*;
    
    #[test]
    fn test_json_parsing_search_accuracy() -> Result<()> {
        // Individual test for JSON parsing case
    }
    
    #[test]
    fn test_error_handling_search_accuracy() -> Result<()> {
        // Individual test for error handling case
    }
    
    // ... other test cases
    
    #[test]
    fn test_overall_search_accuracy() -> Result<()> {
        // Run all test cases and report aggregate metrics
    }
}
```

### Expected Output Example

```
=== Vector Search Accuracy Test Results ===

Query: "parse JSON"
Top 5 results:
1. parse_json (score: 0.92) - src/parser.rs:45
2. JsonParser::parse (score: 0.87) - src/json/parser.rs:12
3. from_json (score: 0.84) - src/utils.rs:78
4. deserialize_json (score: 0.76) - src/serde_utils.rs:23
5. json_to_value (score: 0.71) - src/convert.rs:56

Metrics:
- Precision: 0.80 (4/5 relevant)
- Recall: 0.75 (3/4 expected symbols found)
- MRR: 1.00 (first result relevant)
- Avg Rank: 2.33

✅ Test passed (precision 0.80 >= 0.70)
```

### Integration Notes

1. Import common test utilities from `tests/common/mod.rs`
2. Reuse VectorSearchEngine setup from `vector_integration_test.rs`
3. Consider creating shared accuracy testing utilities for future tests
4. Performance is not critical - focus on accuracy validation

### Success Criteria

- All 5 test cases pass with metrics above thresholds
- Clear output showing search results and metrics
- Code follows all CLAUDE.md principles
- No hardcoded paths or magic numbers
- Proper error handling and recovery