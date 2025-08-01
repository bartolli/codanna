# Vector Search Integration Plan (Simplified)

## Overview
Build semantic search for documentation comments as an MCP-first feature, starting with model evaluation to find the best code-optimized embeddings.

## Strategy
1. **MCP-Only**: No CLI commands - semantic search is an LLM feature
2. **Start Simple**: Use existing doc comments as initial semantic content
3. **POC First**: Build minimal API to test and tune similarity thresholds
4. **Model Selection**: Evaluate code-optimized embedding models before committing

## Phase 0: Model Evaluation (Start Here!)

### Task 0.1: Compare Embedding Models
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

### Task 0.2: Benchmark Selected Model
**Duration**: 1 hour  
**File**: `tests/embedding_model_benchmark.rs` (new)
**Description**: Deep benchmark of chosen model
- Test with 1000+ real doc comments
- Measure batch performance
- Test similarity thresholds
- Evaluate compression options
**Validation**: Performance meets targets, good similarity scores

## Phase 1: Simple API (Doc Comment Search)

### Task 1.1: Basic Semantic API
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

### Task 1.2: Integration with SimpleIndexer
**Duration**: 2 hours  
**Files**: `src/indexing/simple.rs`
**Description**: Add semantic indexing for symbols with doc comments
```rust
// In SimpleIndexer
if let Some(ref mut semantic) = self.semantic_search {
    if let Some(doc) = symbol.doc_comment() {
        semantic.index_doc_comment(symbol.id, doc)?;
    }
}
```
**Validation**: Doc comments are indexed during normal indexing

### Task 1.3: Add to SimpleIndexer API
**Duration**: 1 hour  
**Files**: `src/lib.rs`, `src/indexing/simple.rs`
**Description**: Expose semantic search through SimpleIndexer
```rust
impl SimpleIndexer {
    pub fn semantic_search_docs(&self, query: &str, limit: usize) -> Result<Vec<(Symbol, f32)>>;
}
```
**Validation**: API method returns relevant symbols with scores

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

## Success Metrics

### Simplicity
- [ ] Under 500 lines of new code
- [ ] Single embedding model
- [ ] No CLI changes needed
- [ ] Works with existing index

### Quality
- [ ] Finds semantically related docs
- [ ] Good results for natural language queries
- [ ] Reasonable similarity thresholds
- [ ] Fast enough for interactive use

### Integration
- [ ] MCP tool works smoothly
- [ ] No impact when disabled
- [ ] Easy to enable/disable
- [ ] Clear documentation

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