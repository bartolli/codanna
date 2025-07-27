# Vector Test Reference Guide

This guide provides exact line numbers and sed commands for accessing specific tests in the large `tests/tantivy_ivfflat_poc_test.rs` file (~3400 lines).

## Quick Access Commands

### Test 1: Basic K-means Clustering
```bash
# View test function only (lines 252-387)
sed -n '252,387p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_basic_kmeans_clustering
# Start: line 252 (#[test])
# End: line 387 (closing brace)
```

### Test 2: Centroid Serialization
```bash
# View test function only (lines 389-463)
sed -n '389,463p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_centroid_serialization
# Start: line 389 (#[test])
# End: line 463 (closing brace)
```

### Test 3: Memory-Mapped Vector Storage
```bash
# View test function only (lines 466-617)
sed -n '466,617p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_mmap_vector_storage
# Start: line 466 (#[test])
# End: line 617 (closing brace)
```

### Test 4: Cluster State Management
```bash
# View test function only (lines 620-766)
sed -n '620,766p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_tantivy_warmer_state
# Start: line 620 (#[test])
# End: line 766 (closing brace)
```

### Test 5: Custom ANN Query
```bash
# View test function only (lines 769-861)
sed -n '769,861p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_ann_query_basic
# Start: line 769 (#[test])
# End: line 861 (closing brace)
```

### Test 5b: Realistic Scoring and Ranking
```bash
# View test function only (lines 1474-1636)
sed -n '1474,1636p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_realistic_scoring_and_ranking
# Start: line 1474 (#[test])
# End: line 1636 (closing brace)
```

### Test 6: Real Rust Code Vector Search
```bash
# View test function only (lines 1197-1471)
sed -n '1197,1471p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_real_rust_code_search
# Start: line 1197 (#[test])
# End: line 1471 (closing brace)
```

### Test 7: Tantivy Integration with Clusters
```bash
# View test function only (lines 1999-2109)
sed -n '1999,2109p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_tantivy_integration_with_clusters
# Start: line 1999 (#[test])
# End: line 2109 (closing brace)
```

### Test 8: Custom Tantivy Query/Scorer
```bash
# View test function only (lines 1758-1871)
sed -n '1758,1871p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_custom_ann_query_scorer
# Start: line 1758 (#[test])
# End: line 1871 (closing brace)
```

### Test 11: Incremental Clustering Updates (Main Test)
```bash
# View main test function (lines 2214-2414)
sed -n '2214,2414p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_incremental_clustering_updates
# Start: line 2214 (#[test])
# End: line 2414 (closing brace)
```

#### Test 11 Sub-tests:
```bash
# Test 11.1: Add vectors to existing clusters (lines 2236-2274)
sed -n '2236,2274p' tests/tantivy_ivfflat_poc_test.rs

# Test 11.2: Cluster quality monitoring (lines 2277-2316)
sed -n '2277,2316p' tests/tantivy_ivfflat_poc_test.rs

# Test 11.3: Cluster rebalancing (lines 2319-2351)
sed -n '2319,2351p' tests/tantivy_ivfflat_poc_test.rs

# Test 11.4: Cluster cache consistency (lines 2354-2410)
sed -n '2354,2410p' tests/tantivy_ivfflat_poc_test.rs
```

### Test 12: Vector Storage Segment Management (Main Test)
```bash
# View main test function (lines 2802-3077)
sed -n '2802,3077p' tests/tantivy_ivfflat_poc_test.rs

# Full test name: test_vector_storage_segment_management
# Start: line 2802 (#[test])
# End: line 3077 (closing brace)
```

#### Test 12 Sub-tests:
```bash
# Test 12.1: Vector files with segments (lines 2823-2884)
sed -n '2823,2884p' tests/tantivy_ivfflat_poc_test.rs

# Test 12.2: Segment merging with vectors (lines 2887-2950)
sed -n '2887,2950p' tests/tantivy_ivfflat_poc_test.rs

# Test 12.3: Orphaned vector cleanup (lines 2953-3003)
sed -n '2953,3003p' tests/tantivy_ivfflat_poc_test.rs

# Test 12.4: Atomic vector updates (lines 3006-3073)
sed -n '3006,3073p' tests/tantivy_ivfflat_poc_test.rs
```

## Helper Structures and Types

### Core Types (lines 50-250)
```bash
# View error types and core structures
sed -n '50,250p' tests/tantivy_ivfflat_poc_test.rs
```

### Test 11 Specific Types (lines 2416-2600)
```bash
# View IncrementalUpdateManager and related types
sed -n '2416,2600p' tests/tantivy_ivfflat_poc_test.rs
```

### Test 12 Specific Types (lines 3080-3400)
```bash
# View SegmentVectorStorage and related types
sed -n '3080,3400p' tests/tantivy_ivfflat_poc_test.rs
```

## Usage Tips for Agents

1. **View specific test**: Use the sed command for that test only
2. **View test + context**: Expand the range by 20-50 lines on each side
3. **Search within test**: Combine sed with grep:
   ```bash
   sed -n '2214,2414p' tests/tantivy_ivfflat_poc_test.rs | grep -n "quality"
   ```

4. **Extract just the test function signature**:
   ```bash
   sed -n '2214,2220p' tests/tantivy_ivfflat_poc_test.rs
   ```

## Test Dependencies

- Tests 1-10: Independent, can be viewed in isolation
- Test 11: Uses helper structures defined after the test (lines 2416-2600)
- Test 12: Uses helper structures defined after the test (lines 3080-3400)

## File Structure Overview

```
Lines 1-49: Imports and constants
Lines 50-250: Core types (IvfFlatError, ClusterId, etc.)
Lines 252-2109: Tests 1-9 (individual test functions)
Lines 2214-2414: Test 11 main function
Lines 2416-2800: Test 11 helper structures and functions
Lines 2802-3077: Test 12 main function
Lines 3080-3400: Test 12 helper structures and functions
```

This reference allows precise navigation without loading the entire 3400-line file into memory.