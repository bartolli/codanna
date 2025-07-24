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

For comprehensive CLI documentation, see [CLI.md](./CLI.md).

### Quick Examples
```bash
# Index entire directory with progress
./target/debug/codebase-intelligence index src --progress

# Find a symbol by name
./target/debug/codebase-intelligence retrieve symbol SimpleIndexer

# Show function dependencies
./target/debug/codebase-intelligence retrieve dependencies parse_function

# Start MCP server
./target/debug/codebase-intelligence serve
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

## Current Capabilities

The system can:
- Index both single files and entire directory trees
- Extract symbols (functions, methods, structs, traits) from Rust code
- Detect and track relationships between symbols (calls, implements, uses, defines)
- Persist and load indexes from disk
- Provide comprehensive querying capabilities via CLI
- Serve as an MCP server for AI assistant integration
- Report progress and performance metrics during indexing

## Development Guidelines

When implementing new features:
1. Always check CLI.md for existing command documentation
2. Update CLI.md when adding new commands or options
3. Follow the existing pattern in the parser trait for new languages
4. Ensure all file paths are stored with symbols for navigation
5. Run tests with `cargo test` before committing
6. Use `cargo clippy` for linting
7. Track development progress in TODO.md, not here

## Important Notes

- The README file contains the complete technical architecture and should be consulted for detailed design decisions
- Focus on maintaining the performance targets outlined above
- Use Rust idioms and leverage the type system for safety
- Prioritize zero-copy operations and memory efficiency
