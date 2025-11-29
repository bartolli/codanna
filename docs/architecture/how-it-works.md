# How It Works

Codanna's architecture for fast, accurate code intelligence.

## System Overview

1. **Parse fast** - Tree-sitter AST parsing (same as GitHub code navigator) for Rust, Python, TypeScript, JavaScript, Java, Kotlin, Go, PHP, C, C++, C#, and GDScript
2. **Extract real stuff** - functions, traits, type relationships, call graphs
3. **Embed** - semantic vectors built from your doc comments
4. **Index** - Tantivy + memory-mapped symbol cache for <10ms lookups
5. **Serve** - MCP protocol for AI assistants, ~300ms response time (HTTP/HTTPS) and stdio built-in (0.16s)

## Technology Stack

- **tree-sitter**: Multi-language parsing
- **tantivy**: Full-text search with integrated vector capabilities
- **fastembed**: High-performance embedding generation
- **linfa**: K-means clustering for IVFFlat vector indexing
- **memmap2**: Memory-mapped storage for vector data
- **bincode**: Efficient serialization for vector storage
- **rkyv**: Zero-copy serialization for performance
- **DashMap**: Lock-free concurrent data structures
- **tokio**: Async runtime
- **thiserror**: Structured error handling

## Data Flow

### Indexing Pipeline

```
Source Files
    ↓
Tree-sitter Parser
    ↓
Symbol Extraction
    ↓
Relationship Analysis
    ↓
Doc Comment Embedding
    ↓
Tantivy Index + Vector Store
```

### Query Pipeline

```
User Query
    ↓
MCP Protocol
    ↓
Query Router
    ├→ Exact Match (find_symbol)
    ├→ Full-Text Search (search_symbols)
    ├→ Semantic Search (semantic_search_docs)
    └→ Relationship Queries (get_calls, find_callers)
    ↓
Index Lookup
    ↓
Result Formatting
    ↓
Response (JSON/Text)
```

## Core Components

### Parser System

- Language-agnostic parser trait
- Tree-sitter based implementations
- Symbol extraction from AST
- Relationship tracking (calls, uses, implements)
- Resolution context management

### Index System

**Text Index (Tantivy):**
- Full-text search capabilities
- Symbol metadata storage
- Fuzzy matching support

**Vector Index (Custom):**
- Memory-mapped vector storage
- IVFFlat clustering for fast lookup
- Configurable embedding dimensions (384/768/1024)
- K-means based organization

### MCP Server

- stdio transport (default)
- HTTP/HTTPS transport (optional)
- File watching with hot-reload
- OAuth authentication (HTTP)
- TLS encryption (HTTPS)

## Performance Architecture

### Symbol Cache
- FNV-1a hashed lookups
- Memory-mapped for instant loading
- <10ms response time
- ~100 bytes per symbol

### Vector Cache
- Configurable dimensions (384/768/1024 based on model)
- <1μs access after OS page cache warm-up
- Segmented storage for scalability

### Concurrency Model
- Lock-free reads via DashMap
- Single writer coordination
- Parallel indexing with work-stealing
- Thread-local parser pools

## Storage Layout

```
.codanna/
├── settings.toml           # Configuration
├── index/
│   ├── tantivy/           # Full-text search index
│   ├── vectors/           # Memory-mapped vector storage
│   │   ├── segment_0.vec  # Vector data
│   │   └── metadata.bin   # Vector metadata
│   ├── resolvers/         # Path resolution rules
│   └── symbol_cache.bin   # FNV-1a hashed symbols
└── plugins/
    └── lockfile.json      # Plugin installation tracking
```

## Embedding Lifecycle

1. **Generation**: Doc comments → fastembed → vectors (384/768/1024 dimensions based on model)
2. **Storage**: Vectors stored in memory-mapped files
3. **Clustering**: K-means for IVFFlat organization
4. **Cleanup**: Old embeddings deleted on re-index

## Language-Aware Search

Embeddings track source language, enabling filtering before similarity computation. No score redistribution - identical docs produce identical scores regardless of filtering.

## Hot Reload

File watcher with 500ms debounce triggers re-indexing of changed files only. Changes detected by:
- File modification timestamps
- Content hashing
- Symbol-level change detection

## See Also

- [Memory Mapping](memory-mapping.md) - Cache and storage details
- [Embedding Model](embedding-model.md) - Semantic search internals
- [Language Support](language-support.md) - Parser architecture