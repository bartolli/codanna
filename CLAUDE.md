# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a high-performance code intelligence system written in Rust, designed to provide AI assistants with deep understanding of codebases. Currently in the architecture/planning phase with a comprehensive technical specification in the README file.

## Development Commands

```bash
# Build the project
cargo build --release

# Run tests (currently 42 tests passing)
cargo test

# Run benchmarks
cargo bench

# Build and run in development mode
cargo run -- <command>
```

## CLI Commands

### Currently Implemented
```bash
# Index a single Rust file
./target/debug/codebase-intelligence index <file.rs>

# Find a symbol by name
./target/debug/codebase-intelligence retrieve symbol <name>

# Show what functions a given function calls
./target/debug/codebase-intelligence retrieve calls <function>

# Show what functions call a given function
./target/debug/codebase-intelligence retrieve callers <function>
```

### Example Usage
```bash
# Index a file
./target/debug/codebase-intelligence index src/types/mod.rs

# Find the "new" method
./target/debug/codebase-intelligence retrieve symbol new

# See what "process_batch" calls
./target/debug/codebase-intelligence retrieve calls process_batch

# See what calls "helper"
./target/debug/codebase-intelligence retrieve callers helper
```

**Note**: The CLI currently indexes single files only. The last indexed file is remembered and automatically re-indexed for retrieve commands.

### Planned Commands (from README)
```bash
# Index entire directory (not yet implemented)
./target/release/codebase-intelligence index /path/to/code

# Start as MCP server (not yet implemented)
codebase-intelligence serve --mcp --port 7777
```

## Architecture Overview

### Core Design Principles
- **Type Safety First**: Pure Rust implementation with no dynamic types
- **Performance Focused**: Target 10,000+ files/second indexing
- **Memory Efficient**: ~100 bytes per symbol, compact representations
- **Multi-Target**: Standalone binary, library, or MCP server

### Key Technology Stack
- **tree-sitter**: Multi-language parsing
- **petgraph**: Type-safe graph operations
- **tantivy**: Full-text search optimized for code
- **candle**: Pure Rust ML inference for embeddings
- **DashMap**: Lock-free concurrent data structures
- **tokio**: Async runtime
- **sqlx + SQLite**: Metadata persistence

### Performance Targets
- Indexing: 10,000+ files/second
- Search latency: <10ms for semantic search
- Memory: ~100MB for 1M symbols
- Incremental updates: <100ms per file
- Startup: <1s with memory-mapped cache

## Key Implementation Details

### Data Structures
- `CompactSymbol`: 32-byte cache-line aligned structure
- `CompactReference`: 16-byte reference structure
- Use `NonZeroU32` for space optimization
- String interning for efficient storage

### Parallel Processing Strategy
- Work-stealing queues for file processing
- Thread-local parser pools
- Chunk size: `num_cpus::get() * 4`
- Parallel git walk for file discovery

### Memory Optimization
- Zero-copy serialization with rkyv
- Memory-mapped files for instant loading
- Cache-line aligned structures (32 bytes, 2 per cache line)

## Development Status

### ✅ Completed Components

- Core data structures (Symbol, Relationship, Storage)
- Basic Rust parser using tree-sitter
- Single file indexer with relationship detection
- Basic CLI with index and retrieve commands
- 42 tests passing

### 🚧 Current Implementation

- Indexes single Rust files
- Extracts functions, methods, structs, and traits
- Detects function call relationships
- Provides symbol search and call graph queries

### 📋 Next Steps

1. Directory walking for multi-file indexing
2. Persistent index storage
3. Parser pool for performance
4. Language detection and multi-language support
5. Parallel indexing pipeline
6. MCP server implementation

## Important Notes

- The README file contains the complete technical architecture and should be consulted for detailed design decisions
- Focus on maintaining the performance targets outlined above
- Use Rust idioms and leverage the type system for safety
- Prioritize zero-copy operations and memory efficiency
