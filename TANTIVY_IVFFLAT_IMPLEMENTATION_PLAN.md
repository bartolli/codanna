# Tantivy IVFFlat Vector Search Implementation Plan

## Overview
This plan outlines the step-by-step implementation of IVFFlat vector search integrated directly with Tantivy, following a Test-Driven Development (TDD) approach. The implementation is already partially complete with 6 tests passing in `tantivy_ivfflat_poc_test.rs`.

## Current State
- ✅ Test 1: Basic K-means clustering with linfa
- ✅ Test 2: Centroid serialization with bincode
- ✅ Test 3: Memory-mapped vector storage
- ✅ Test 4: Cluster state management (simulated warmer)
- ✅ Test 5: Custom ANN query implementation
- ✅ Test 6: Real Rust code vector search with fastembed

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
- [ ] Move from test-only structures to production-ready implementations
- [ ] Create `IVFFlatIndex` with proper error handling
- [ ] Implement `ClusterCache` with thread-safe access
- [ ] Add configuration for probe factor and clustering parameters

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

## Key Implementation Details

### Data Flow
1. **Indexing**: Parse → Embed → Cluster → Store in Tantivy + Vector Storage
2. **Querying**: Query → Find Clusters → Load Vectors → Score → Rank

### Architecture Decisions
- **External Cache** approach for cluster mappings (compatible with current Tantivy)
- **Memory-mapped** vectors for efficient loading
- **Per-segment** storage aligned with Tantivy's architecture
- **Unified scoring** through Tantivy's Scorer interface

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
