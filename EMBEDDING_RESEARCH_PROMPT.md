# Research Prompt: Rust Embedding Solutions for Codanna's Tantivy-based Architecture

## Context

We are developing **Codanna**, a high-performance code intelligence system written in pure Rust. We need to research Rust libraries that can generate code embeddings and integrate seamlessly with our existing Tantivy-based search architecture.

## Current System Architecture

### Core Storage: Tantivy 0.24.2
- Full-text search engine with memory-mapped indices
- Schema-based document storage
- Currently indexes: symbols, relationships, documentation, file metadata
- All data stored as Tantivy documents with fields

### Existing Schema Structure
```rust
pub struct IndexSchema {
    // Symbol fields
    pub symbol_id: Field,        // Numeric, INDEXED | STORED
    pub name: Field,             // Text, searchable
    pub doc_comment: Field,      // Text, searchable
    pub signature: Field,        // Text, searchable
    pub module_path: Field,      // String, faceted
    pub kind: Field,             // String (enum discriminant)
    pub file_path: Field,        // String, stored
    pub line_number: Field,      // Numeric
    
    // Metadata fields
    pub file_id: Field,          // Numeric
    pub file_hash: Field,        // String (SHA256)
    pub indexed_at: Field,       // Timestamp
}
```

### Current Dependencies
```toml
tantivy = "0.24.2"
candle-core = "0.9.1"        # Already included but not used
candle-nn = "0.9.1"          # Already included but not used
candle-transformers = "0.9.1" # Already included but not used
hnsw = "0.11.0"              # Already included but not used
```

### System Specifications
- **Language**: Pure Rust (no Python dependencies)
- **Performance Target**: Index 10,000+ files/second
- **Memory Budget**: ~100MB for 1M symbols
- **Architecture**: Can run as CLI, library, or MCP server
- **Platform**: Cross-platform (Linux, macOS, Windows)

## Research Requirements

### 1. Embedding Generation Libraries
Research Rust libraries that can:
- Generate embeddings for code snippets (functions, classes, methods)
- Support multiple programming languages (Rust, Python, JS, etc.)
- Work with pre-trained models (CodeBERT, CodeT5, etc.)
- Run efficiently on CPU (GPU optional)
- Have minimal memory overhead

Key questions:
- Can we use Candle (already in dependencies) effectively?
- Are there Rust bindings for popular code models?
- What's the performance overhead of embedding generation?
- Can we batch process embeddings during indexing?

### 2. Vector Storage Integration with Tantivy

Research approaches for storing embeddings alongside Tantivy documents:

**Option A: Embeddings as Tantivy Fields**
- Store as binary field: `schema.add_bytes_field("embedding", STORED)`
- Store as array of floats: Custom serialization
- Pros/cons of each approach?

**Option B: Separate Vector Index**
- Use HNSW (already in dependencies) alongside Tantivy
- Synchronization strategies between indices
- Transaction consistency considerations

**Option C: Tantivy Extensions**
- Are there Tantivy plugins for vector search?
- Community efforts for adding vector support?
- Timeline for native vector support in Tantivy?

### 3. Query Pipeline Integration

Research how to integrate vector search into existing query flow:
```rust
// Current query flow
pub fn search(&self, query: &str) -> Result<Vec<Symbol>> {
    let query_parser = QueryParser::for_index(&self.index, vec![name, doc_comment]);
    let query = query_parser.parse_query(query)?;
    let top_docs = searcher.search(&query, &TopDocs::with_limit(10))?;
    // ... convert to symbols
}

// Desired: Hybrid search combining text + vectors
pub fn hybrid_search(&self, query: &str) -> Result<Vec<Symbol>> {
    // 1. Text search in Tantivy
    // 2. Generate query embedding
    // 3. Vector similarity search
    // 4. Merge and re-rank results
}
```

### 4. Incremental Updates

How to handle embedding updates efficiently:
- Can we generate embeddings on-the-fly during indexing?
- Batch processing strategies for better GPU utilization?
- Caching strategies for unchanged files?
- Impact on our current indexing speed (~19 files/second)?

### 5. Memory and Performance Constraints

Research memory-efficient approaches:
- Quantization options (int8, binary embeddings)?
- Dimensionality reduction techniques?
- Trade-offs between embedding quality and size?
- Expected memory usage for 1M symbols with embeddings?

## Specific Libraries to Investigate

1. **fastembed-rs** - Rust port of FastEmbed
2. **ort** - ONNX Runtime Rust bindings
3. **burn** - Pure Rust deep learning framework
4. **candle** - Already in deps, how to use effectively?
5. **linfa** - Rust ML toolkit (for dimensionality reduction)
6. **hora** - Approximate nearest neighbor search
7. **usearch** - High-performance vector search

## Integration Requirements

The solution must:
1. Add minimal overhead to current indexing pipeline
2. Store embeddings efficiently (target: <100 bytes per symbol)
3. Support incremental updates
4. Work with our existing Tantivy transaction model
5. Maintain type safety (no dynamic types)
6. Be deployable as single binary (no external services)

## Deliverables from Research

1. **Recommended architecture** for adding embeddings to Tantivy
2. **Specific library choices** with benchmarks
3. **Code examples** showing integration approach
4. **Performance analysis** (indexing speed, memory usage, query latency)
5. **Migration plan** from current pure-text to hybrid search

## Additional Context

Our indexing currently works in two phases:
1. Parse files and extract symbols
2. Commit to Tantivy in transaction batches

The embedding generation would need to fit into this flow, ideally:
- Generate embeddings during parsing phase
- Store alongside symbol data in Tantivy
- Enable hybrid search without major architectural changes

Current codebase: https://github.com/[username]/codebase-intelligence