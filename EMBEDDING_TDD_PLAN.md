# TDD Plan for Embedding Proof of Concept

## Overview
Create a minimal, isolated proof of concept that demonstrates embedding generation and vector search alongside our existing Tantivy infrastructure, without modifying any production code initially.

## Phase 1: Isolated Embedding Tests (Week 1)

### 1.1 Create Basic Embedding Test Module
- Create `tests/embedding_poc_test.rs`
- Test fastembed-rs basic functionality:
  - Generate embeddings for code snippets
  - Verify embedding dimensions and quality
  - Benchmark generation speed

### 1.2 Memory & Performance Tests
- Test quantization (int8 vs float32)
- Measure memory usage for 1000 symbols
- Verify <100 bytes/symbol target achievable

## Phase 2: pgvector Integration Tests (Week 1-2)

### 2.1 Docker-based pgvector Tests
- Create `docker-compose.test.yml` with pgvector
- Create `tests/pgvector_integration_test.rs`
- Test basic operations:
  - Store embeddings
  - HNSW index creation
  - Similarity search

### 2.2 Transactional Consistency Tests
- Test atomic updates (symbol + embedding)
- Test rollback scenarios
- Verify data integrity

## Phase 3: Hybrid Search Prototype (Week 2)

### 3.1 Create Minimal Hybrid Searcher
- Create `src/search/hybrid.rs` (experimental module)
- Implement simple RRF merger
- Test parallel Tantivy + pgvector queries

### 3.2 Integration Tests
- Create `tests/hybrid_search_test.rs`
- Test search quality:
  - Text-only queries
  - Semantic-only queries
  - Combined queries
- Compare results with current Tantivy-only search

## Phase 4: Proof of Concept CLI (Week 3)

### 4.1 Add Experimental Commands
- Add `--experimental-embeddings` flag to existing commands
- `codanna index --experimental-embeddings` - generates embeddings during indexing
- `codanna search --experimental-hybrid` - uses hybrid search

### 4.2 Side-by-Side Comparison
- Index same codebase with/without embeddings
- Compare:
  - Indexing time
  - Memory usage
  - Search quality
  - Storage size

## Phase 5: Decision Point (Week 3-4)

### 5.1 Metrics to Evaluate
- Performance impact: <20% indexing slowdown acceptable
- Memory usage: Must stay under 100MB/1M symbols
- Search quality: Must show measurable improvement
- Operational complexity: Docker requirement acceptable?

### 5.2 Integration Path
If POC successful:
1. Create feature branch for full integration
2. Add embeddings as optional feature flag
3. Gradually migrate search logic
4. Maintain backward compatibility

## Implementation Details

### Test Structure
```
tests/
├── embedding_poc_test.rs         # Basic embedding tests
├── pgvector_integration_test.rs  # pgvector tests
├── hybrid_search_test.rs         # Combined search tests
└── fixtures/
    └── sample_code/              # Test code files

src/
└── search/
    └── hybrid.rs                 # Experimental hybrid search

docker/
└── docker-compose.test.yml       # pgvector test setup
```

### Dependencies to Add (test-only initially)
```toml
[dev-dependencies]
fastembed = "3.0"
pgvector = "0.4"
tokio-postgres = "0.7"
testcontainers = "0.15"  # For Docker-based tests
```

### Key Test Cases

1. **Embedding Generation**
   - Generate embeddings for 100 Rust functions
   - Verify consistency (same input → same embedding)
   - Test batch vs single generation performance

2. **Vector Storage**
   - Store 10,000 embeddings in pgvector
   - Query performance at different scales
   - Memory usage monitoring

3. **Search Quality**
   - Find semantically similar functions
   - Handle synonyms (e.g., "parse" → "analyze")
   - Cross-file semantic relationships

4. **Integration Safety**
   - Existing tests must continue passing
   - No performance regression in text-only search
   - Backward compatibility maintained

## Research Summary

Based on comprehensive analysis of three research reports, the recommended architecture is:

### Embedding Generation: fastembed-rs
- 50% faster than PyTorch Transformers
- CPU-optimized, no CUDA dependencies
- Supports batch processing (256 default)
- Low memory footprint

### Vector Storage: PostgreSQL with pgvector
- **Performance**: 28x lower latency than Pinecone (benchmarked)
- **Features**: ACID compliance, transactional integrity
- **pgvector 0.8.0**: Iterative index scans for efficient hybrid search
- **Cost**: 75-79% lower than specialized vector databases

### Memory Optimization Strategy
To achieve <100 bytes/symbol:
- Use int8 quantization (768 dims → 768 bytes → ~100 bytes compressed)
- Or reduce to 128 dimensions + int8 (128 bytes)
- Store in pgvector with HNSW index

### Hybrid Search with RRF
```rust
// Reciprocal Rank Fusion
fn merge_with_rrf(text_results: Vec<Result>, vector_results: Vec<Result>) -> Vec<Result> {
    let k = 60.0; // Standard RRF parameter
    // Merge rankings from both sources
}
```

This approach allows us to thoroughly validate the embedding concept before any production code changes, minimizing risk while providing concrete performance data.