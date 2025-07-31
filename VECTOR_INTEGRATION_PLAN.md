# Vector Search Integration Plan - Production Migration

## Overview
This document provides a detailed, incremental plan for completing the vector search integration from POC to production. Each task is designed to be small (1-2 hours), testable, and focused on a single file or component.

## Current State Summary
- ✅ Basic vector modules implemented (`src/vector/*`)
- ✅ SimpleIndexer has optional vector support
- ✅ POC tests passing (11 tests in `tantivy_ivfflat_poc_test.rs`)
- ✅ **DocumentIndex integration started** (Tasks 1.1-1.4 complete)
- ❌ No incremental clustering
- ❌ No CLI support
- ❌ No query parser integration

## Progress Update (2025-07-31)
- ✅ **Task 1.1**: Vector fields added to Tantivy schema
- ✅ **Task 1.2**: VectorMetadata structure implemented
- ✅ **Task 1.3**: Vector storage path added to DocumentIndex
- ✅ **Task 1.4**: Cluster cache implemented (650ns lookups!)
- ✅ **Task 1.5**: Vector commit hook implemented with embedding support

## Implementation Phases

### Phase 1: DocumentIndex Integration (5-7 days)

#### Task 1.1: Add Vector Fields to Tantivy Schema
**Files**: `src/storage/tantivy.rs`
**Duration**: 2 hours
**Description**: Add optional vector-related fields to the Tantivy schema
```rust
// Add fields:
// - cluster_id: FAST u64 field
// - vector_id: FAST u64 field (maps to SymbolId)
// - has_vector: FAST bool field
```
**Test**: Create new test `test_schema_with_vector_fields` in `tantivy.rs`
**Validation**: 
- Schema creation succeeds with new fields
- Existing tests still pass
- Can store/retrieve documents with vector fields

#### Task 1.2: Create VectorMetadata Structure
**Files**: `src/storage/tantivy.rs`
**Duration**: 1 hour
**Description**: Add struct to track vector-related metadata per document
```rust
struct VectorMetadata {
    vector_id: Option<VectorId>,
    cluster_id: Option<ClusterId>,
    embedding_version: u32,
}
```
**Test**: Unit tests for serialization/deserialization
**Validation**: Can round-trip metadata through Tantivy

#### Task 1.3: Add Vector Storage Path to DocumentIndex
**Files**: `src/storage/tantivy.rs`
**Duration**: 1 hour
**Description**: Add vector storage management to DocumentIndex
```rust
pub struct DocumentIndex {
    // existing fields...
    vector_storage_path: Option<PathBuf>,
    vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
}
```
**Test**: Modify existing DocumentIndex tests to verify optional vector support
**Validation**: 
- DocumentIndex works with and without vector support
- Path handling is correct

#### Task 1.4: Implement Cluster Cache Structure
**Files**: `src/storage/tantivy.rs`
**Duration**: 2 hours
**Description**: Add cluster cache based on POC design (Test 4)
```rust
struct ClusterCache {
    generation: u64,
    segment_mappings: HashMap<SegmentOrdinal, HashMap<ClusterId, Vec<DocId>>>,
}
```
**Test**: Create `test_cluster_cache_operations`
**Reference**: `tantivy_ivfflat_poc_test.rs::test_04_cluster_state_management`
**Validation**: 
- Cache builds correctly from segments
- Lookup performance <10μs

#### Task 1.5: Add Vector Commit Hook ✅
**Files**: `src/storage/tantivy.rs`
**Duration**: 2 hours  
**Description**: Modify `commit_batch` to trigger vector operations
```rust
impl DocumentIndex {
    pub fn commit_batch(&self) -> StorageResult<()> {
        // existing commit...
        if let Some(ref engine) = self.vector_engine {
            self.post_commit_vector_processing()?;
        }
    }
}
```
**Test**: Create `test_vector_commit_integration` ✅
**Validation**: 
- ✅ Commits trigger vector processing
- ✅ Failures don't corrupt text index
**Implementation Notes**:
- Added `pending_embeddings` field to track symbols awaiting vector processing
- Added `embedding_generator` field for generating embeddings
- Added `with_embedding_generator()` builder method
- Modified `add_document()` to track symbols for embedding
- Implemented `post_commit_vector_processing()` that generates embeddings and indexes vectors
- Created comprehensive test coverage in `document_index_vector_commit_test.rs`

#### Task 1.6: Implement Cache Warming
**Files**: `src/storage/tantivy.rs`
**Duration**: 3 hours
**Description**: Add cluster cache warming after reader reload
```rust
fn warm_cluster_cache(&self) -> StorageResult<()> {
    // Implementation from POC Test 4
}
```
**Test**: Create `test_cache_warming_performance`
**Reference**: `tantivy_ivfflat_poc_test.rs::test_04_cluster_state_management`
**Validation**: 
- Cache rebuilds on generation change
- Performance meets targets

#### Task 1.7: Integration Test for DocumentIndex + Vectors
**Files**: `tests/document_index_vector_integration_test.rs`
**Duration**: 2 hours
**Description**: Comprehensive test of DocumentIndex with vector support
**Test Coverage**:
- Index documents with vectors
- Commit and reload
- Verify cluster cache
- Test hybrid queries
**Validation**: All operations work end-to-end

### Phase 2: Incremental Clustering (3-4 days)

#### Task 2.1: Define VectorClusterer Trait
**Files**: `src/vector/clustering.rs`
**Duration**: 1 hour
**Description**: Create trait for pluggable clustering algorithms
```rust
pub trait VectorClusterer: Send + Sync {
    fn initial_clustering(&self, vectors: &[Vec<f32>]) -> Result<KMeansResult, ClusteringError>;
    fn assign_to_cluster(&self, vector: &[f32], centroids: &[Vec<f32>]) -> ClusterId;
    fn should_rebalance(&self, stats: &ClusterStats) -> bool;
}
```
**Test**: Create mock implementation for testing
**Validation**: Trait is ergonomic and complete

#### Task 2.2: Implement ClusterStats Tracking
**Files**: `src/vector/clustering.rs`
**Duration**: 2 hours
**Description**: Add statistics for cluster quality monitoring
```rust
pub struct ClusterStats {
    cluster_sizes: HashMap<ClusterId, usize>,
    total_vectors: usize,
    average_distance: f32,
    max_cluster_size: usize,
    min_cluster_size: usize,
}
```
**Test**: Create `test_cluster_stats_calculation`
**Reference**: `tantivy_ivfflat_poc_test.rs::test_11_incremental_clustering`
**Validation**: Stats accurately reflect cluster state

#### Task 2.3: Implement Incremental Assignment
**Files**: `src/vector/engine.rs`
**Duration**: 2 hours
**Description**: Add method to assign new vectors without full re-clustering
```rust
impl VectorSearchEngine {
    pub fn add_vectors_incremental(&mut self, vectors: &[(VectorId, Vec<f32>)]) -> Result<(), VectorError> {
        // Assign to nearest existing clusters
        // Update statistics
        // Check if rebalancing needed
    }
}
```
**Test**: Create `test_incremental_vector_addition`
**Validation**: 
- New vectors assigned correctly
- Performance <1ms per vector
- Stats updated

#### Task 2.4: Implement Rebalancing Logic
**Files**: `src/vector/clustering.rs`
**Duration**: 3 hours
**Description**: Add cluster rebalancing when quality degrades
```rust
impl VectorClusterer for KMeansClusterer {
    fn should_rebalance(&self, stats: &ClusterStats) -> bool {
        // Check cluster size variance
        // Check average distances
        // Return true if rebalancing needed
    }
}
```
**Test**: Create `test_rebalancing_triggers`
**Reference**: `tantivy_ivfflat_poc_test.rs::test_11_incremental_clustering`
**Validation**: 
- Triggers at appropriate thresholds
- Rebalancing improves quality

#### Task 2.5: Integration Test for Incremental Updates
**Files**: `tests/incremental_clustering_test.rs`
**Duration**: 2 hours
**Description**: Test incremental clustering workflow
**Test Coverage**:
- Add vectors incrementally
- Verify assignments
- Trigger rebalancing
- Measure performance
**Validation**: Incremental updates meet performance targets

### Phase 3: Symbol Change Detection (3-4 days)

#### Task 3.1: Add Symbol Hashing to Parser Trait
**Files**: `src/parsing/mod.rs`
**Duration**: 2 hours
**Description**: Extend Parser trait with content hashing
```rust
pub trait Parser: Send + Sync {
    // existing methods...
    fn calculate_symbol_hash(&self, symbol: &Symbol, content: &str) -> String;
}
```
**Test**: Add tests for each parser implementation
**Validation**: Hashes are stable and deterministic

#### Task 3.2: Implement Symbol Hash Storage
**Files**: `src/storage/tantivy.rs`
**Duration**: 2 hours
**Description**: Add symbol_hash field to Tantivy schema
```rust
// Add field: symbol_hash: TEXT STORED
```
**Test**: Create `test_symbol_hash_storage`
**Validation**: Can store and retrieve symbol hashes

#### Task 3.3: Create SymbolChangeDetector
**Files**: `src/vector/change_detection.rs` (new file)
**Duration**: 3 hours
**Description**: Implement change detection logic from Test 10
```rust
pub struct SymbolChangeDetector {
    pub fn detect_changes(&self, old_symbols: &[Symbol], new_symbols: &[Symbol]) 
        -> SymbolChanges {
        // Implementation from vector_update_test.rs
    }
}
```
**Test**: Port tests from `vector_update_test.rs`
**Reference**: `vector_update_test.rs::test_10_*`
**Validation**: All Test 10 scenarios pass

#### Task 3.4: Integrate Change Detection with SimpleIndexer
**Files**: `src/indexing/simple.rs`
**Duration**: 2 hours
**Description**: Use change detector during file updates
```rust
impl SimpleIndexer {
    fn index_file_with_change_detection(&mut self, path: &Path) -> IndexResult<()> {
        // Load old symbols
        // Parse new symbols
        // Detect changes
        // Update only changed symbols
    }
}
```
**Test**: Create `test_incremental_symbol_updates`
**Validation**: Only changed symbols trigger re-embedding

#### Task 3.5: Implement VectorUpdateTransaction
**Files**: `src/vector/update.rs` (new file)
**Duration**: 3 hours
**Description**: Atomic vector updates with rollback
```rust
pub struct VectorUpdateTransaction {
    // Implementation from Test 10.4
}
```
**Test**: Create comprehensive transaction tests
**Reference**: `vector_update_test.rs::test_10_update_transaction`
**Validation**: 
- Atomic updates
- Rollback on failure
- Concurrent safety

### Phase 4: Query Integration (3-4 days)

#### Task 4.1: Create Vector Query Types
**Files**: `src/storage/query.rs` (new file)
**Duration**: 2 hours
**Description**: Define vector query structures
```rust
pub struct VectorQuery {
    pub embedding: Vec<f32>,
    pub k: usize,
    pub clusters_to_probe: f32, // percentage
}
```
**Test**: Unit tests for query validation
**Validation**: Query structures are complete

#### Task 4.2: Implement Custom Tantivy Query
**Files**: `src/storage/vector_query.rs` (new file)
**Duration**: 4 hours
**Description**: Port AnnQuery from POC
```rust
pub struct AnnQuery {
    // Implementation from Test 8
}
impl Query for AnnQuery {
    // Implementation
}
```
**Test**: Port tests from POC
**Reference**: `tantivy_ivfflat_poc_test.rs::test_08_custom_tantivy_query`
**Validation**: Query integrates with Tantivy

#### Task 4.3: Implement RRF Scoring
**Files**: `src/storage/scoring.rs` (new file)
**Duration**: 2 hours
**Description**: Reciprocal Rank Fusion implementation
```rust
pub fn combine_scores_rrf(text_scores: &[(DocId, f32)], 
                          vector_scores: &[(DocId, f32)], 
                          k: f32) -> Vec<(DocId, f32)> {
    // Implementation from Test 9
}
```
**Test**: Create `test_rrf_scoring`
**Reference**: `tantivy_ivfflat_poc_test.rs::test_09_hybrid_search`
**Validation**: RRF produces expected rankings

#### Task 4.4: Add Vector Search to DocumentIndex
**Files**: `src/storage/tantivy.rs`
**Duration**: 3 hours
**Description**: Add vector search methods
```rust
impl DocumentIndex {
    pub fn search_similar(&self, symbol_id: SymbolId, limit: usize) 
        -> StorageResult<Vec<SearchResult>> {
        // Get embedding for symbol
        // Create AnnQuery
        // Execute search
        // Apply RRF if hybrid
    }
}
```
**Test**: Create `test_vector_search_api`
**Validation**: Search returns relevant results

#### Task 4.5: Query Parser Extensions
**Files**: `src/query/parser.rs` (if exists) or `src/storage/tantivy.rs`
**Duration**: 2 hours
**Description**: Add vector query syntax
```rust
// Support syntax like:
// ~similar_to:parse_function
// ~near:SymbolId
```
**Test**: Create `test_vector_query_parsing`
**Validation**: Parser handles vector queries

### Phase 5: CLI Integration (2 days)

#### Task 5.1: Add Vector Subcommand Structure
**Files**: `src/main.rs`
**Duration**: 1 hour
**Description**: Add vector-related CLI commands
```rust
#[derive(Subcommand)]
enum VectorCommands {
    Similar { symbol: String, limit: Option<usize> },
    Stats,
}
```
**Test**: CLI parsing tests
**Validation**: Commands parse correctly

#### Task 5.2: Implement Similar Command
**Files**: `src/main.rs`
**Duration**: 2 hours
**Description**: Implement `codanna retrieve similar <symbol>`
```rust
fn execute_similar_search(index: &DocumentIndex, symbol: &str, limit: usize) {
    // Resolve symbol
    // Execute vector search
    // Format results
}
```
**Test**: Integration test with real index
**Validation**: Command returns similar symbols

#### Task 5.3: Add Vector Indexing Progress
**Files**: `src/indexing/simple.rs`, `src/main.rs`
**Duration**: 2 hours
**Description**: Show vector processing progress
```rust
// Add progress callbacks for:
// - Embedding generation
// - Clustering
// - Vector storage
```
**Test**: Manual testing with progress flag
**Validation**: Progress updates are informative

#### Task 5.4: Add Configuration Support
**Files**: `src/config.rs` (if exists) or create new
**Duration**: 2 hours
**Description**: Vector search configuration
```rust
pub struct VectorConfig {
    pub enabled: bool,
    pub dimension: usize,
    pub clusters_to_probe: f32,
    pub rebalance_threshold: f32,
}
```
**Test**: Configuration loading tests
**Validation**: Config affects behavior correctly

#### Task 5.5: CLI Integration Tests
**Files**: `tests/cli_vector_test.rs`
**Duration**: 2 hours
**Description**: End-to-end CLI tests
**Test Coverage**:
- Index with vectors
- Search similar symbols
- View statistics
**Validation**: All commands work as expected

### Phase 6: Performance & Polish (2-3 days)

#### Task 6.1: Add SIMD Dot Product
**Files**: `src/vector/simd.rs` (new file)
**Duration**: 3 hours
**Description**: SIMD-accelerated similarity
```rust
#[cfg(target_arch = "x86_64")]
pub fn dot_product_simd(a: &[f32], b: &[f32]) -> f32 {
    // AVX2 implementation
}
```
**Test**: Benchmark against scalar version
**Validation**: 2-4x performance improvement

#### Task 6.2: Implement Vector Caching
**Files**: `src/vector/cache.rs` (new file)
**Duration**: 2 hours
**Description**: LRU cache for hot vectors
```rust
pub struct VectorCache {
    cache: LruCache<VectorId, Vec<f32>>,
}
```
**Test**: Cache hit rate tests
**Validation**: Reduces mmap calls

#### Task 6.3: Add Monitoring Metrics
**Files**: `src/metrics.rs` (new file)
**Duration**: 2 hours
**Description**: Vector operation metrics
```rust
pub struct VectorMetrics {
    pub embeddings_generated: Counter,
    pub clustering_time_ms: Histogram,
    pub search_latency_ms: Histogram,
}
```
**Test**: Metrics collection tests
**Validation**: Metrics are accurate

#### Task 6.4: Memory Usage Optimization
**Files**: Various vector files
**Duration**: 3 hours
**Description**: Reduce memory footprint
- Implement vector quantization option
- Optimize data structures
- Add memory usage tracking
**Test**: Memory benchmarks
**Validation**: Stay within 100MB for 1M symbols

#### Task 6.5: Final Integration Tests
**Files**: `tests/vector_integration_final_test.rs`
**Duration**: 3 hours
**Description**: Comprehensive system tests
**Test Coverage**:
- Large codebase indexing
- Concurrent operations
- Performance validation
- Memory usage validation
**Validation**: All targets met

## Risk Mitigation

### For Each Task:
1. **Backup**: Git commit before changes
2. **Feature Flag**: Keep vector features optional
3. **Rollback**: Each task is independently revertible
4. **Testing**: Comprehensive tests before moving on

### Critical Decision Points:
1. After Task 1.7: Validate DocumentIndex integration
2. After Task 2.5: Confirm incremental clustering works
3. After Task 3.5: Ensure update detection is accurate
4. After Task 4.5: Verify query performance
5. After Task 5.5: User acceptance of CLI

## Success Metrics

### Per-Task Validation:
- All existing tests pass
- New tests pass
- No performance regression
- Memory usage acceptable

### Overall Success:
- 10,000+ files/second indexing
- <10ms vector search latency
- <100MB memory for 1M symbols
- Incremental updates <100ms
- All POC tests still passing

## Timeline Summary

- **Phase 1**: 5-7 days (DocumentIndex Integration)
- **Phase 2**: 3-4 days (Incremental Clustering)  
- **Phase 3**: 3-4 days (Symbol Change Detection)
- **Phase 4**: 3-4 days (Query Integration)
- **Phase 5**: 2 days (CLI Integration)
- **Phase 6**: 2-3 days (Performance & Polish)

**Total**: 18-25 days for complete integration

## Notes

- Each task is designed to be completed in 1-4 hours
- Tasks within a phase can often be parallelized
- Testing time is included in estimates
- Buffer time included for debugging
- Can pause after any task without breaking system