[Documentation](../README.md) / **Architecture**

---

# Architecture

High-performance code intelligence system in Rust. Indexes code, tracks relationships, serves via MCP.

## How It Works

1. **Parse fast** - Tree-sitter AST parsing (same as GitHub code navigator) for Rust, Python, TypeScript, JavaScript, Java, Kotlin, Go, PHP, C, C++, C#, Swift, and GDScript
2. **Extract real stuff** - functions, traits, type relationships, call graphs
3. **Embed** - semantic vectors built from your doc comments
4. **Index** - Tantivy + memory-mapped symbol cache for <10ms lookups
5. **Serve** - MCP protocol for AI assistants, ~300ms response time (HTTP/HTTPS) and stdio built-in (0.16s)

## In This Section

- **[How It Works](how-it-works.md)** - Detailed system architecture
- **[Memory Mapping](memory-mapping.md)** - Cache and storage design
- **[Embedding Model](embedding-model.md)** - Semantic search implementation
- **[Language Support](language-support.md)** - Parser system and adding languages

## Architecture Highlights

**Parallel indexing pipeline**: 5-stage architecture (DISCOVER → READ → PARSE → COLLECT → INDEX) with work-stealing queues. Phase 2 runs EmbeddingPool for parallel embedding generation.

**Memory-mapped storage**: Vector cache for semantic search:
- `segment_0.vec` - 384-dimensional vectors, <1μs access after OS page cache warm-up

**Embedding lifecycle management**: Old embeddings deleted when files are re-indexed to prevent accumulation.

**Lock-free concurrency**: DashMap for concurrent reads, RwLock for Tantivy writes.

**IndexFacade**: Unified interface wrapping DocumentIndex, Pipeline, and SemanticSearch.

**Language-aware semantic search**: Embeddings track source language, enabling filtering before similarity computation. No score redistribution - identical docs produce identical scores regardless of filtering.

**Hot reload**: File watcher with 500ms debounce triggers re-indexing of changed files only.

## Performance

Parser benchmarks on a 750-symbol test file:

| Language | Parsing Speed | vs. Target (10k/s) | Status |
|----------|---------------|-------------------|--------|
| **Rust** | 91,318 symbols/sec | 9.1x faster ✓ | Production |
| **Python** | 75,047 symbols/sec | 7.5x faster ✓ | Production |
| **TypeScript** | 82,156 symbols/sec | 8.2x faster ✓ | Production |
| **PHP** | 68,432 symbols/sec | 6.8x faster ✓ | Production |
| **Go** | 74,655 symbols/second | 7.5x faster ✓ | Production |

Run performance benchmarks:
```bash
codanna benchmark all          # Test all parsers
codanna benchmark python       # Test specific language
```

## Next Steps

- Learn about [User Guide](../user-guide/) for usage
- Explore [Advanced](../advanced/) features
- Read [Contributing](../contributing/) to add features

[Back to Documentation](../README.md)