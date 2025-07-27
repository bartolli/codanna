# Tantivy IVFFlat Vector Search Implementation Plan

## Overview
This plan outlines the step-by-step implementation of IVFFlat vector search integrated directly with Tantivy, following a Test-Driven Development (TDD) approach. The implementation is already partially complete with 6 tests passing in `tantivy_ivfflat_poc_test.rs`.

## Current State
- ✅ Test 1: Basic K-means clustering with linfa
- ✅ Test 2: Centroid serialization with bincode
- ✅ Test 3: Memory-mapped vector storage
- ✅ Test 4: Cluster state management (simulated warmer)
- ✅ Test 5: Custom ANN query implementation
- ✅ Test 5b: Realistic scoring and ranking with RRF
- ✅ Test 6: Real Rust code vector search with fastembed
- ✅ Test 7: Tantivy integration with cluster IDs
- ✅ Test 8: Custom Tantivy Query/Scorer
- ✅ Test 9: Hybrid search with multiple scoring strategies

## Implementation Steps

### Phase 1: Complete POC Tests (Days 1-2)

#### Step 1: Implement Tantivy Integration Test
- [x] Create test that demonstrates actual Tantivy document indexing with cluster IDs
- [x] Add cluster_id as a FAST field to Tantivy schema
- [x] Implement batch indexing with vector clustering
- [x] Test segment-aware cluster mappings

#### Step 2: Implement Custom Tantivy Query/Scorer
- [x] Create `AnnQuery` struct implementing Tantivy's `Query` trait
- [x] Implement `AnnWeight` for query weighting
- [x] Create `AnnScorer` for vector similarity scoring
- [x] Test integration with BooleanQuery for hybrid search

#### Step 3: Production-Ready Data Structures
- [ ] Extract `IvfFlatError` to `src/vector/error.rs`
- [ ] Move `ClusterId` newtype to `src/vector/types.rs`
- [ ] Extract `IVFFlatIndex` and builder to `src/vector/index.rs`
- [ ] Create `ClusterCache` in `src/vector/cache.rs`
- [ ] Add `IVFFlatConfig` for runtime configuration

### Discovered Dependencies for Production

Based on POC analysis, move these from dev-dependencies to dependencies:
- `bincode = "2.0.1"` - For centroid serialization
- `fastembed = "5.0.0"` - For embedding generation
- `linfa = "0.7.1"` - For K-means clustering
- `linfa-clustering = "0.7.1"` - For clustering algorithms

Note: memmap2 is already in dependencies.

### Phase 2: Production Implementation (Days 3-4)

#### Step 4: Integrate with DocumentIndex
- [ ] Extend `src/storage/tantivy.rs` to support vector operations
- [ ] Add vector storage alongside Tantivy segments
- [ ] Implement clustering during batch commits
- [ ] Add vector field to `IndexSchema`

#### Step 5: Implement Clustering Pipeline
- [ ] Create `VectorClusterer` trait for pluggable clustering
- [ ] Implement K-means clustering with linfa
- [ ] Add incremental clustering for new documents
- [ ] Implement centroid updates during compaction

#### Step 6: Memory-Mapped Vector Storage
- [ ] Create `VectorStorage` abstraction over mmap
- [ ] Implement per-segment vector files
- [ ] Add vector loading with cluster-based filtering
- [ ] Implement vector norm pre-computation

#### Step 6b: Implement Update/Reindexing Flow
- [ ] Add symbol-level content hashing to track changes
- [ ] Extend Tantivy schema with `symbol_content_hash` field
- [ ] Implement differential symbol comparison on file updates
- [ ] Create incremental vector update logic
- [ ] Add vector deletion for removed symbols
- [ ] Implement incremental clustering updates
- [ ] Add cluster quality metrics and re-clustering triggers

### Phase 3: Query Integration (Days 5-6)

#### Step 7: Query Parser Extensions
- [ ] Extend query parser to support vector queries
- [ ] Add syntax for k-NN queries (e.g., `~similar_to:function_name`)
- [ ] Implement hybrid query construction
- [ ] Add configuration for similarity thresholds

#### Step 8: Scoring and Ranking
- [ ] Implement Reciprocal Rank Fusion (RRF)
- [ ] Add configurable score combination strategies
- [ ] Implement score normalization
- [ ] Add query-time boosting parameters

#### Step 9: CLI Integration
- [ ] Add `--vector` flag to enable vector search
- [ ] Implement `codanna retrieve similar <symbol>`
- [ ] Add vector indexing progress reporting
- [ ] Implement vector index statistics command

### Phase 4: Optimization and Testing (Days 7-8)

#### Step 10: Performance Optimization
- [ ] Implement SIMD-accelerated dot product
- [ ] Add vector quantization (int8) option
- [ ] Implement multi-threaded cluster assignment
- [ ] Add caching for frequently accessed vectors

#### Step 11: Comprehensive Testing
- [ ] Create performance benchmarks for vector operations
- [ ] Test with 10K, 100K, 1M vectors
- [ ] Implement integration tests
- [ ] Add memory usage tests
- [ ] Validate against performance targets

#### Step 12: Documentation and Examples
- [ ] Document vector search API
- [ ] Create usage examples
- [ ] Add configuration guide
- [ ] Document performance tuning

### Phase 5: Update and Reindexing Support (Days 9-10)

#### Step 13: Symbol-Level Change Detection
- [ ] Implement `calculate_symbol_hash` in parser trait
- [ ] Add hash comparison logic to `SimpleIndexer`
- [ ] Create symbol diff algorithm
- [ ] Store symbol hashes in Tantivy

#### Step 14: Incremental Vector Updates
- [ ] Implement selective embedding regeneration
- [ ] Add vector update/delete operations
- [ ] Create embedding version tracking
- [ ] Implement vector storage cleanup

#### Step 15: Incremental Clustering
- [ ] Add cluster statistics tracking
- [ ] Implement online cluster assignment
- [ ] Create cluster quality metrics
- [ ] Add re-clustering scheduler
- [ ] Implement cluster cache invalidation

#### Step 16: Integration Testing
- [ ] Test file update scenarios
- [ ] Validate incremental clustering
- [ ] Benchmark update performance
- [ ] Test rollback scenarios

## Key Implementation Details

### Data Flow
1. **Indexing**: Parse → Embed → Cluster → Store in Tantivy + Vector Storage
2. **Querying**: Query → Find Clusters → Load Vectors → Score → Rank

### Architecture Decisions
- **External Cache** approach validated - integrates cleanly with existing Mutex patterns
- **Memory-mapped** vectors achieve 0.71 μs/vector access time
- **Per-segment** storage with segment ordinal mapping
- **Unified scoring** through Tantivy's Scorer interface with Column<u64> for FAST fields
- **RRF scoring** with k=60 for optimal text/vector balance
- **NonZeroU32** for ClusterId prevents off-by-one errors
- **Symbol-level hashing** for fine-grained change detection
- **Incremental clustering** to avoid full index rebuilds
- **Embedding versioning** for consistency tracking

### Performance Targets
- Indexing: Maintain 10,000+ files/second
- Query latency: <10ms for 1M vectors
- Memory: ~100 bytes per symbol (with quantization)
- Cluster probe: 10-50% configurable

## Testing Strategy
- All implementation in test file first
- Move to production only after validation
- Performance validation at each step
- Maintain test coverage >90%

## Risk Mitigation
- POC approach isolates changes from production
- Incremental implementation reduces complexity
- Performance validation at each step
- Reversible changes with feature flags

## Success Criteria
- All 12+ tests passing
- Performance meets targets (10K files/sec, <10ms query)
- Memory usage within targets (<100 bytes/symbol)
- Clean integration with existing codebase
- No impact on text-only search performance

## Progress Tracking
Update this file as tasks are completed. Use ✅ for completed tasks and note any significant findings or changes to the plan.

## Migration Checklist

### Code Extraction Priority
1. **Error types and domain models** (Week 1, Day 1)
   - [ ] Create `src/vector/` module structure
   - [ ] Extract error types with thiserror
   - [ ] Move domain types (ClusterId, etc.)

2. **Core algorithms** (Week 1, Days 2-3)
   - [ ] Extract clustering algorithms
   - [ ] Move similarity calculations
   - [ ] Add SIMD optimizations

3. **Tantivy integration** (Week 1, Days 4-5)
   - [ ] Extract Query/Weight/Scorer implementations
   - [ ] Integrate with DocumentIndex
   - [ ] Add cluster cache warming

4. **CLI and configuration** (Week 2)
   - [ ] Add vector search commands
   - [ ] Implement configuration system
   - [ ] Add monitoring/metrics

### Validation Steps
- [ ] All 9 POC tests pass after extraction
- [ ] Performance benchmarks match POC results
- [ ] Memory usage within targets
- [ ] No regression in text-only search

### Notable Findings from Completed Tests
1. **K-means Clustering**: Successfully clusters 384-dimensional vectors with even distribution
2. **Serialization**: Zero overhead with bincode v2 for centroid storage
3. **Memory-Mapped Storage**: Achieved 0.71 μs/vector random access performance
4. **Cluster State**: External cache approach validated as best fit for current architecture
5. **ANN Query**: Demonstrated 99.8% reduction in vector comparisons with 2/3 cluster probing
6. **Real Code Search**: fastembed produces meaningful semantic groupings with clear similarity boundaries:
   - Same concept: 0.99+ similarity
   - Related concepts: 0.5-0.8 similarity
   - Different concepts: <0.5 similarity
7. **Tantivy Integration**: Successfully demonstrated cluster-based filtering with FAST fields:
   - Cluster IDs stored as indexed u64 FAST fields
   - Boolean queries efficiently combine cluster filters with text queries
   - Segment-aware processing enables per-segment optimizations
   - Multiple segments handled correctly (3 segments in test)
8. **Custom Query/Scorer**: Successfully implemented Tantivy Query trait for ANN:
   - AnnQuery implements Query trait with Debug derivation
   - Weight creates per-segment Scorer with FAST field access
   - Column\<u64\> type used for efficient cluster_id reading
   - Architecture supports segment-parallel execution
   - Ready for integration with BooleanQuery for hybrid search
9. **Hybrid Search**: Multiple scoring strategies validated:
   - RRF with k=60 provides best overall results
   - Linear combination allows fine-tuning with α/β weights
   - Score boosting (text * (1 + vector_sim)) works for emphasis
   - All strategies maintain sub-10ms query times
