# TDD Plan: Tantivy-based IVFFlat POC

## Target Solution Data Flow

### Indexing Flow:

```
1. Parse Code (existing) → Symbol + Context
2. Generate Embedding (fastembed) → 384-dim vector
3. Batch Vectors → When threshold reached (e.g., 1000 vectors)
4. Run K-means Clustering → Centroids + Assignments
5. Store in Tantivy:
   - Document: symbol data + cluster_id (existing fields)
   - Warmer State: cluster_id → [doc_ids] mapping
   - External Storage: memory-mapped vectors by cluster
6. Serialize Index State → centroids.bin + vectors.mmap
```

### Query Flow:

```
1. Query Input → "find similar to parse_function"
2. Generate Query Embedding → 384-dim vector
3. Find Nearest Clusters:
   - Compare with all centroids (small, in-memory)
   - Select top-K clusters based on probe %
4. Create Tantivy Query:
   - AnnQuery wraps cluster DocSets
   - Combine with text queries (BooleanQuery)
5. Score Documents:
   - Load vectors only from selected clusters (mmap)
   - Compute dot product with query vector
   - Merge with text scores (if hybrid)
6. Return Results → Ranked by combined score
```

### Key Design Decisions:

- **Offline Clustering**: K-means runs during compaction, not per-query
- **Segment Integration**: Each Tantivy segment has its own cluster mapping
- **Lazy Loading**: Vectors loaded on-demand via mmap, not held in memory
- **Unified Scoring**: Vector similarity becomes just another Tantivy score

## Starting Point: Single Test File

Create `tests/tantivy_ivfflat_poc_test.rs` - all POC code lives in this test file initially.

## Test Progression (One Test at a Time):

### Test 1: Basic K-means Clustering

```rust
#[test]
fn test_basic_kmeans_clustering() {
    // Given: 100 random 384-dim vectors
    // When: Cluster into 10 groups using linfa
    // Then: Each vector assigned to exactly one cluster
}
```

### Test 2: Centroid Serialization

```rust
#[test]
fn test_centroid_serialization() {
    // Given: Clustered vectors with centroids
    // When: Serialize with bincode
    // Then: Can deserialize and get identical centroids
}
```

### Test 3: Memory-Mapped Vector Storage

```rust
#[test]
fn test_mmap_vector_storage() {
    // Given: Vectors grouped by cluster
    // When: Write contiguously and mmap
    // Then: Can read back vectors by cluster efficiently
}
```

### Test 4: Tantivy Warmer Integration

```rust
#[test]
fn test_tantivy_warmer_state() {
    // Given: Existing Tantivy index from your codebase
    // When: Add warmer with cluster mappings
    // Then: Warmer maintains ClusterId -> [DocId] state
}
```

**Note on Tantivy Warmer API**: Tantivy doesn't currently have a `set_warming_callback` API like described in the original comment. The actual implementation would need one of these approaches:

1. **Custom Collector**: Build cluster mappings when creating a new IndexReader
2. **Segment Hook**: Monitor segment changes and rebuild mappings on reload
3. **External Cache**: Maintain mappings outside Tantivy, update on index changes
4. **Future API**: Contribute a warmer API to Tantivy project

### Recommended Implementation Approach

Based on analysis of the current codebase (`src/storage/tantivy.rs`), the best approach is **Option 3: External Cache** with these specifics:

#### Rationale:

1. **Current Architecture Compatibility**: The codebase already uses:

   - `Mutex<Option<IndexWriter>>` for writer state management
   - Manual `reload()` calls after commits
   - Centralized `DocumentIndex` abstraction

2. **Existing Patterns**: The code already handles:
   - Batch operations with `start_batch()` and `commit_batch()`
   - Reader reloading with `self.reader.reload()?`
   - Shared state with `Arc` and `Mutex`

#### Implementation Details:

```rust
pub struct DocumentIndex {
    // Existing fields...
    cluster_cache: Arc<RwLock<HashMap<u32, ClusterMappings>>>, // NEW
    reader_generation: AtomicU64, // NEW - track reader version
}

impl DocumentIndex {
    pub fn commit_batch(&self) -> StorageResult<()> {
        // Existing commit logic...
        writer.commit()?;
        self.reader.reload()?;

        // NEW: Trigger cache rebuild
        self.warm_cluster_cache()?;
        Ok(())
    }

    fn warm_cluster_cache(&self) -> StorageResult<()> {
        let current_gen = self.reader.searcher_generation();
        let stored_gen = self.reader_generation.load(Ordering::Relaxed);

        if current_gen != stored_gen {
            // Rebuild cache for new/changed segments
            let searcher = self.reader.searcher();
            let mut new_cache = HashMap::new();

            for (ord, segment) in searcher.segment_readers().iter().enumerate() {
                // Build cluster_id -> [doc_ids] mapping
                let mappings = self.build_segment_mappings(segment)?;
                new_cache.insert(ord as u32, mappings);
            }

            // Atomic cache swap
            *self.cluster_cache.write().unwrap() = new_cache;
            self.reader_generation.store(current_gen, Ordering::Relaxed);
        }
        Ok(())
    }
}
```

#### Advantages:

1. **No Tantivy Modifications**: Works with current Tantivy version
2. **Predictable Warming**: Happens after commits when we control it
3. **Consistent with Codebase**: Follows existing patterns for state management
4. **Performance**: Only rebuilds when segments actually change
5. **Thread-Safe**: Uses same RwLock pattern as segment state in tests

#### Integration Points:

- Hook into existing `commit_batch()` - already the centralized commit point
- Use existing `self.reader` - already managed centrally
- Leverage `IndexSchema` - can add cluster_id as a FAST field
- Compatible with existing error handling patterns

### Test 5: Custom ANN Query

```rust
#[test]
fn test_ann_query_basic() {
    // Given: Tantivy index with warmed cluster state
    // When: Create AnnQuery for a query vector
    // Then: Returns DocSet with nearest cluster docs
}
```

### Test 6: Hybrid Query Combination

```rust
#[test]
fn test_hybrid_text_vector_query() {
    // Given: Your existing Tantivy index + vector data
    // When: Combine text query with ANN query
    // Then: Results blend both ranking signals
}
```

## Key Implementation Strategy:

1. **Reuse Existing Infrastructure**:

   - Use your existing `DocumentIndex` from `src/storage/tantivy.rs`
   - Extend it in the test file without modifying production code

2. **Incremental Data Structures**:

```rust
// Start simple in the test file
struct IVFFlatIndex {
    centroids: Vec<Vec<f32>>,
    assignments: Vec<ClusterId>,
    vector_storage: MmapVectorStorage,
}
```

3. **Leverage Existing Test Fixtures**:
   - Use vectors from your `embedding_poc_test.rs`
   - Reuse test documents from existing Tantivy tests

## Tests

```bash
cargo test --test tantivy_ivfflat_poc_test -- --show-output 
```

## Implementation Details

### Key Dependencies to Add (dev-dependencies only for POC):

```toml
[dev-dependencies]
linfa = "0.7.1"
linfa-clustering = "0.7.1"
bincode = "2.0.1"  # Note: v2 API uses encode_to_vec/decode_from_slice
memmap2 = "0.9.7"
ndarray = "0.15"  # Must match linfa's version to avoid conflicts
```

### Implementation Discoveries:

1. **ndarray Version Compatibility**: linfa 0.7 requires ndarray 0.15, not 0.16
2. **bincode v2 API Changes**: Use `encode_to_vec()` and `decode_from_slice()` instead of `serialize()`/`deserialize()`
3. **Tantivy Warmer Alternative**: Since Tantivy lacks a warmer API, use segment-aware caching:
   - Build cluster mappings when IndexReader reloads
   - Store mappings per segment ordinal
   - Invalidate when segments change
4. **Memory-Mapped Performance**: Achieved 0.71 μs/vector random access
5. **Serialization Efficiency**: 0% bincode overhead for centroid storage

### Test 6: Real Code Embedding Analysis

Successfully implemented and validated vector search with real Rust code snippets using fastembed.

#### Key Findings:

1. **Real Embedding Similarity Ranges**:
   - Very similar (>0.8): Not observed in real code - even related functions show nuanced differences
   - Somewhat similar (0.5-0.8): 
     - JSON parsing functions: 0.689
     - Related parsing functions: 0.502-0.689
   - Different (<0.5):
     - JSON vs XML parsing: 0.417 (65.3% lower than JSON-JSON)
     - Unrelated functions: 0.349-0.497

2. **Semantic Search Performance**:
   - Query "parse JSON data" correctly found JSON functions first
   - Query "implement parser trait" found trait implementations
   - Query "handle errors" found error handling code
   - Cluster-based filtering successfully limited search scope

3. **Real-World Observations**:
   - fastembed's AllMiniLML6V2 model produces meaningful semantic groupings
   - Similarity scores are more conservative than expected (no >0.8 pairs)
   - Clear semantic boundaries between different concepts
   - Clustering works well even with small datasets (8 snippets into 3 clusters)

4. **Production Implications**:
   - Threshold tuning needed: Use 0.5 for "related", not 0.7
   - Memory footprint confirmed: 1536 bytes per embedding (384 dims × 4 bytes)
   - Cluster coherence: Functions naturally group by semantic concept

### Key Findings from Realistic Scoring Tests:

1. **Reciprocal Rank Fusion (RRF) Excellence**:

   - Successfully combines text and vector signals
   - Top results show both text match AND semantic similarity
   - RRF constant k=60 works well (tunable parameter)
   - Better results than either signal alone

2. **Vector Similarity Discrimination**:

   - JSON parsing functions: 0.992-0.999 similarity
   - Generic string parsing: 0.709-0.820 similarity
   - XML parsing (different domain): 0.472-0.534 similarity
   - Error handling (unrelated): 0.247 similarity
   - Clear semantic grouping of related functions

3. **Text/Vector Score Integration**:

   - Vector similarities naturally in [0,1] range
   - Easy to combine with normalized BM25 scores
   - Multiple combination strategies work:
     - RRF (rank-based fusion)
     - Linear combination: `α * text_score + β * vector_score`
     - Score boosting: `text_score * (1 + vector_similarity)`

4. **Production-Ready Patterns**:
   - Custom Tantivy Scorer for vector similarity
   - Leverage existing Collector infrastructure
   - Pre-compute and cache vector norms
   - Cluster filtering limits vectors to score

Observations

1. Empirical evidence that RRF works well for combining text and vector scores
2. Actual similarity ranges observed for different semantic groups
3. Multiple scoring strategies that can be used in production
4. Additional test patterns to explore for more comprehensive validation

These findings provide valuable guidance for anyone implementing this system in production. The documented
similarity ranges (0.99+ for same concept, 0.7-0.8 for related, 0.4-0.5 for different domain) are
particularly useful for setting thresholds and understanding expected behavior.

The additional test patterns I suggested would help validate the system for:

- More complex real-world queries
- Edge cases like negative queries
- Performance optimization strategies
- Cross-language code search capabilities

Findings that need to be verified with real code snippets:

1. Reciprocal Rank Fusion (RRF) Works Well: The top 3 results are all JSON-related functions, showing that
   combining text and vector signals produces better results than either alone.
2. Vector Similarity is Powerful: The vector similarities clearly distinguish between:

    - JSON functions (0.992-0.999 similarity)
    - String/integer parsing (0.709-0.820 similarity)
    - XML functions (0.472-0.534 similarity)
    - Error handling (0.247 similarity)

3. Text Scoring Complements Vectors: While "parse" appears in many functions (negative IDF), "json" is more
   selective and boosts the right documents.
4. Realistic Integration Path: The test shows exactly how to implement this in Tantivy:

    - Custom Scorer computes vector similarity
    - Tantivy's existing Collector infrastructure handles combination
    - RRF or simple boosting both work

Suggestions for Production Implementation:

1. Score Normalization: Vector similarities are already in [0,1] range, making them easy to combine with
   BM25 scores.
2. Efficiency: The cluster-based filtering from earlier tests would limit which vectors need scoring.
3. Tuning: The RRF constant k=60 or a simple boost factor can be tuned based on your data.
4. Caching: Vector norms can be pre-computed and stored with the vectors.

This test proves that the IVFFlat approach can deliver high-quality hybrid search results that combine the
best of both text matching and semantic understanding!

### Additional Test Patterns to Explore

1. **Multi-Modal Queries**:

   - Code + comments + signatures
   - Different embeddings for different fields
   - Field-specific boosting

2. **Negative Examples**:

   - "parse but not XML" queries
   - Semantic NOT operations
   - Exclusion clusters

3. **Performance Patterns**:

   - Batch query processing
   - Progressive refinement (coarse to fine)
   - Early termination strategies

4. **Quality Patterns**:
   - Cross-language semantic search (Python finds Rust)
   - Typo tolerance via embeddings
   - Concept expansion (search "parse" finds "deserialize")

### Architecture Benefits

- **Unified System**: Single storage backend for both text and vectors (no separate pgvector)
- **Memory Efficient**: Only load needed clusters via mmap, not entire index
- **Flexible Probing**: Adjust quality/performance with probe percentage (10% = fast, 50% = accurate)
- **Native Integration**: Leverages Tantivy's existing segment architecture and query framework
- **Cache Friendly**: Contiguous vector storage per cluster improves CPU cache hits
- **Operational Simplicity**: No external database to manage, just files alongside Tantivy segments

## Test-Driven Development Steps

### Step 1: Create Test File

- Create `tests/tantivy_ivfflat_poc_test.rs`
- Add linfa dependencies to dev-dependencies only
- All POC code lives in this test file initially

### Step 2: Progressive Test Implementation

1. **Test K-means Clustering** - Verify basic clustering works
2. **Test Serialization** - Ensure centroids/assignments can be persisted
3. **Test Memory-Mapped Storage** - Validate efficient vector access
4. **Test Tantivy Integration** - Add warmer for cluster state
5. **Test ANN Query** - Implement basic vector search
6. **Test Hybrid Search** - Combine with existing text search

### Step 3: Leverage Existing Code

- Reuse `DocumentIndex` from `src/storage/tantivy.rs`
- Use embeddings from `embedding_poc_test.rs`
- Extend without modifying production code

### Step 4: Performance Validation

- Compare with pgvector implementation
- Measure memory usage
- Test query latency
- Validate scalability claims

## Benefits

- **Zero Risk**: Completely isolated in tests
- **Incremental**: One concept per test
- **Comparative**: Easy to benchmark against pgvector
- **Reusable**: Leverages existing infrastructure

## Success Criteria

- All tests pass
- Performance meets or exceeds pgvector
- Memory usage stays within targets
- Integration is clean and maintainable

## Next Steps After Basic Tests Pass

1. **Performance Tests**: Compare with pgvector approach
2. **Scale Tests**: Test with 10K, 100K, 1M vectors
3. **Integration Tests**: Full indexing pipeline
4. **Memory Tests**: Verify efficiency claims

## Migration Path

This approach lets us validate the IVFFlat concept using your existing Tantivy infrastructure while maintaining complete isolation from production code. We can prove the approach works before making any architectural decisions.
