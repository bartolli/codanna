# Vector Search Integration Plan (Simplified)

## Overview
Build semantic search for documentation comments as an MCP-first feature, starting with model evaluation to find the best code-optimized embeddings.

## Strategy
1. **MCP-Only**: No CLI commands - semantic search is an LLM feature
2. **Start Simple**: Use existing doc comments as initial semantic content
3. **POC First**: Build minimal API to test and tune similarity thresholds
4. **Model Selection**: Evaluate code-optimized embedding models before committing

## Phase 0: Model Evaluation (Start Here!)

### Task 0.1: Compare Embedding Models ✅
**Duration**: 2 hours  
**File**: `tests/embedding_model_comparison.rs` (new)
**Description**: Test different code-optimized models from fastembed
```rust
Models to evaluate:
1. AllMiniLML6V2 (current baseline - 384 dims)
2. jinaai/jina-embeddings-v2-base-code (768 dims, code-optimized)
3. nomic-ai/nomic-embed-text-v1.5 (768 dims, includes code)

Evaluation criteria:
- Code similarity accuracy
- Performance (embeddings/second)
- Memory usage per embedding
- Model size and download time
```
**Test Cases**:
- Similar function implementations
- Same concept, different languages
- Documentation vs implementation
- Error handling patterns
**Validation**: Clear winner based on code similarity scores

### Task 0.2: Benchmark Selected Model ✅
**Duration**: 1 hour  
**File**: `tests/embedding_model_benchmark.rs` (new)
**Description**: Deep benchmark of chosen model
- Test with 1000+ real doc comments
- Measure batch performance
- Test similarity thresholds
- Evaluate compression options
**Validation**: Performance meets targets, good similarity scores

## Phase 1: Simple API (Doc Comment Search)

### Task 1.1: Basic Semantic API ✅
**Duration**: 2 hours  
**Files**: `src/semantic/mod.rs` (new), `src/semantic/simple.rs` (new)
**Description**: Minimal API for semantic search on doc comments
```rust
pub struct SimpleSemanticSearch {
    embeddings: HashMap<SymbolId, Vec<f32>>,
    model: TextEmbedding,
}

impl SimpleSemanticSearch {
    // Only what we need for POC
    pub fn new(model_name: &str) -> Result<Self>;
    pub fn index_doc_comment(&mut self, symbol_id: SymbolId, doc: &str) -> Result<()>;
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(SymbolId, f32)>>;
}
```
**Validation**: Can index and search doc comments

### Task 1.2: Integration with SimpleIndexer ✅
**Duration**: 2 hours  
**Files**: `src/indexing/simple.rs`
**Description**: Add semantic indexing for symbols with doc comments
**Completed**:
- Added `semantic_search` field to SimpleIndexer struct
- Modified `store_symbol` to index doc comments automatically
- Added cleanup in `clear_tantivy_index` method
- Thread-safe integration with Arc<Mutex<SimpleSemanticSearch>>
**Validation**: Doc comments are indexed during normal indexing ✅

### Task 1.3: Add to SimpleIndexer API ✅
**Duration**: 1 hour  
**Files**: `src/indexing/simple.rs`
**Description**: Expose semantic search through SimpleIndexer
**Completed**:
- `enable_semantic_search()` - Initialize semantic search capability
- `has_semantic_search()` - Check if semantic search is enabled
- `semantic_search_docs(query, limit)` - Search with natural language
- `semantic_search_docs_with_threshold(query, limit, threshold)` - Filtered search
**Validation**: API methods return relevant symbols with scores ✅

## Phase 2: MCP Integration

### Task 2.1: Create MCP Tool
**Duration**: 2 hours  
**Files**: `src/mcp/tools.rs`
**Description**: Add semantic_search_docs tool
```json
{
  "name": "semantic_search_docs",
  "description": "Search documentation using natural language",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {"type": "string", "description": "Natural language search query"},
      "limit": {"type": "integer", "default": 10},
      "threshold": {"type": "number", "description": "Minimum similarity score (0-1)"}
    }
  }
}
```
**Validation**: Tool appears in MCP tool list

### Task 2.2: Implement Tool Handler
**Duration**: 1 hour  
**Files**: `src/mcp/handlers.rs`
**Description**: Wire up semantic search to MCP
- Call SimpleIndexer::semantic_search_docs
- Format results for LLM consumption
- Include symbol context in response
**Validation**: Tool returns meaningful results

### Task 2.3: Test with Real Queries
**Duration**: 2 hours  
**Description**: Test semantic search with realistic queries
- "Find authentication functions"
- "Show error handling code"
- "Database connection methods"
- "Functions that parse user input"
**Validation**: Results make semantic sense

## Phase 3: Tuning and Enhancement

### Task 3.1: Similarity Threshold Tuning
**Duration**: 2 hours  
**Files**: `src/semantic/simple.rs`
**Description**: Find optimal similarity thresholds
- Test various thresholds (0.5 - 0.9)
- Balance precision vs recall
- Document recommended values
**Validation**: Clear threshold recommendations

### Task 3.2: Result Ranking
**Duration**: 1 hour  
**Files**: `src/semantic/simple.rs`
**Description**: Improve result ranking
- Consider symbol kind in ranking
- Boost exact matches
- Penalize very short docs
**Validation**: Better result ordering

### Task 3.3: Performance Optimization
**Duration**: 2 hours  
**Description**: Optimize for production use
- Batch embedding generation
- Cache query embeddings
- Optimize similarity calculations
**Validation**: <50ms search latency

## API Usage

### How to Use Semantic Search

```rust
use codanna::SimpleIndexer;

// 1. Create indexer with settings
let mut indexer = SimpleIndexer::new();
// or with custom settings:
// let settings = Arc::new(Settings { ... });
// let mut indexer = SimpleIndexer::with_settings(settings);

// 2. Enable semantic search
indexer.enable_semantic_search()?;

// 3. Index files normally - doc comments are automatically indexed
indexer.index_file("src/lib.rs")?;

// 4. Search using natural language queries
let results = indexer.semantic_search_docs("parse JSON data", 10)?;
for (symbol, score) in results {
    println!("{}: {:.3}", symbol.name, score);
}

// 5. Search with similarity threshold (only return scores > 0.6)
let results = indexer.semantic_search_docs_with_threshold(
    "user authentication", 
    10,     // limit
    0.6     // minimum similarity score
)?;
```

### Available Methods

- `enable_semantic_search() -> Result<()>` - Initialize semantic search capability
- `has_semantic_search() -> bool` - Check if semantic search is enabled
- `semantic_search_docs(query: &str, limit: usize) -> Result<Vec<(Symbol, f32)>>` - Search with natural language
- `semantic_search_docs_with_threshold(query: &str, limit: usize, threshold: f32) -> Result<Vec<(Symbol, f32)>>` - Search with minimum score filter

## Test Results & Performance

### Semantic Search Accuracy (from integration tests)
- **"parse JSON data"** → parse_json: 0.625 (good match)
- **"user authentication login"** → authenticate_user: 0.620 (good match)
- **"recursive calculation factorial"** → factorial: 0.769 (excellent match)
- **"matrix multiplication"** → all functions: <0.034 (correctly identified as unrelated)

### Updated Similarity Thresholds (based on real results)
- **Very Similar**: 0.75+ (e.g., exact concept match)
- **Similar**: 0.60+ (e.g., related concepts)
- **Related**: 0.40+ (e.g., somewhat related)
- **Unrelated**: <0.30

## Success Metrics

### Simplicity
- [x] Under 500 lines of new code (~400 lines)
- [x] Single embedding model (AllMiniLML6V2)
- [x] No CLI changes needed
- [x] Works with existing index

### Quality
- [x] Finds semantically related docs (validated with tests)
- [x] Good results for natural language queries (0.6+ scores for matches)
- [x] Reasonable similarity thresholds (empirically validated)
- [x] Fast enough for interactive use (<1s for indexing + search)

### Integration
- [ ] MCP tool works smoothly (Phase 2 pending)
- [x] No impact when disabled (opt-in via enable_semantic_search)
- [x] Easy to enable/disable (single method call)
- [x] Clear documentation (doc comments + tests demonstrate usage)

## Future Considerations

Once POC proves value:
1. Extend to symbol names and signatures
2. Add more sophisticated ranking
3. Consider hybrid search
4. Explore code-specific models
5. Add configuration options

## Key Decisions

1. **Doc Comments Only**: Start with high-quality natural language content
2. **MCP First**: Built for LLMs, not human CLI users  
3. **Simple Storage**: In-memory HashMap for POC
4. **Single Model**: Pick best one and stick with it
5. **Minimal API**: Only what's needed for POC

## Implementation Notes

1. Start with Phase 0 - model evaluation is critical
2. Keep it simple - resist over-engineering
3. Focus on doc comment quality
4. Test with real queries early
5. Document similarity thresholds