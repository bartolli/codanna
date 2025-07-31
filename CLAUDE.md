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
./target/debug/codanna index src --progress

# Find a symbol by name
./target/debug/codanna retrieve symbol SimpleIndexer

# Show function dependencies
./target/debug/codanna retrieve dependencies parse_function

# Start MCP server
./target/debug/codanna serve
```

## Architecture Overview

### Core Design Principles

- **Type Safety First**: Pure Rust implementation with no dynamic types
- **Performance Focused**: Target 10,000+ files/second indexing
- **Memory Efficient**: ~100 bytes per symbol, compact representations
- **Multi-Target**: Standalone binary, library, or MCP server

### Key Technology Stack

- **tree-sitter**: Multi-language parsing
- **tantivy**: Full-text search with integrated vector capabilities
- **fastembed**: High-performance embedding generation
- **linfa**: K-means clustering for IVFFlat vector indexing
- **candle**: Pure Rust ML inference and embeddings
- **memmap2**: Memory-mapped storage for vector data
- **bincode**: Efficient serialization for vector storage
- **rkyv**: Zero-copy serialization for performance
- **DashMap**: Lock-free concurrent data structures
- **tokio**: Async runtime
- **thiserror**: Structured error handling

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

**Core Indexing & Search:**
- Index both single files and entire directory trees at 10,000+ files/second
- Extract symbols (functions, methods, structs, traits) from Rust code
- Detect and track relationships between symbols (calls, implements, uses, defines)
- Persist and load indexes from disk
- Provide comprehensive querying capabilities via CLI
- Serve as an MCP server for AI assistant integration
- Report progress and performance metrics during indexing

**Vector Search (POC Complete, Ready for Production):**
- Generate 384-dimensional embeddings using fastembed (AllMiniLML6V2 model)
- Cluster vectors using K-means with linfa for IVFFlat indexing
- Store vectors in memory-mapped files achieving 0.71 μs/vector access
- Perform hybrid text + vector search with <10ms latency
- Combine scores using Reciprocal Rank Fusion (RRF) with k=60
- Filter searches to specific clusters, reducing comparisons by 99.8%
- Detect symbol-level changes for incremental vector updates
- Handle concurrent file updates with atomic transactions and rollback

**Performance Achievements:**
- Vector indexing: 1.4M vectors/second (single-threaded)
- Clustering: 100ms for 10,000 384-dim vectors
- Memory usage: 1536 bytes per embedding (384 dims × 4 bytes)
- Incremental updates: <100ms per file with symbol-level granularity
- Zero serialization overhead for cluster centroids with bincode v2

The vector search POC has been validated with 10 comprehensive tests (16 sub-tests) and is architecturally ready for production migration with minor refinements.

## Development Guidelines

**Rust Coding Principles**

**Function Signatures - Zero-Cost Abstractions**

- Use `&str` over `String`, `&[T]` over `Vec<T>` in parameters - maximizes caller flexibility
- Use `impl Trait` over trait objects when possible

**The Rule:**

- **Take owned types** (`String`, `Vec<T>`) when you need to **store or transform** the data
- **Take borrowed types** (`&str`, `&[T]`) when you only need to **read or process** the data

```rust
// ✅ Flexible - accepts any string-like data
fn parse_config(input: &str) -> Result<Config, Error> { ... }

// ❌ Forces ownership transfer or expensive clones
fn parse_config(input: String) -> Result<Config, Error> { ... }
```

**Functional Decomposition**

- Break complex parsing into helper functions by responsibility
- Use `iter().map().collect()` chains over manual loops
- Narrow scope to avoid lifetime complexity

**The Rule:**

- **One function, one responsibility** - if you're doing lexing AND parsing AND validation, split it
- **Break up when you hit nested pattern matching** deeper than 2 levels

```rust
pub fn parse_code(input: &str) -> Result<Ast, ParseError> {
    let tokens = tokenize(input)?;        // Lexing responsibility
    let ast = parse_tokens(&tokens)?;     // Parsing responsibility
    validate_ast(&ast)?;                  // Validation responsibility
    Ok(ast)
}
```

**Error Handling**

- Use `thiserror` for library errors with context
- Make errors actionable - include suggestions when possible
- Prefer `Result<T, E>` over panics; use `expect()` only for impossible states

**The Rule:**

- **Library code**: Use `thiserror` - callers need structured errors
- **Application code**: `anyhow` is fine - you're handling errors finally
- **Add context at boundaries** - when crossing module/crate boundaries

**Type-Driven Design**

- Use newtypes for domain modeling
- Make invalid states unrepresentable at compile time
- Leverage builder patterns for complex configuration

**The Rule:**

- **Primitive obsession is bad** - `UserId(u64)` instead of raw `u64`
- **If it can be invalid, make it a type** - don't rely on runtime validation alone
- **More than 3 constructor parameters** = time for a builder pattern

**API Ergonomics**

- Implement `Debug`, `Clone`, `PartialEq` where sensible
- Use `#[must_use]` on important return values
- Provide both owned/borrowed variants: `into_foo()` and `as_foo()`

**The Rule:**

- **Always implement `Debug`** unless you have a very good reason not to
- **If users might ignore your Result, add `#[must_use]`**
- **Conversion methods**: `into_` consumes, `as_` borrows, `to_` clones

**Performance**

- Prefer iterators over intermediate collections
- Use `Cow<'_, str>` when you might need owned or borrowed data

**The Rule:**

- **Hot path = no allocations** - use iterators and borrowing
- **One-time setup = allocations are fine** - optimize for readability
- **When in doubt, measure** - don't optimize prematurely

When implementing new features:

1. Always check CLI.md for existing command documentation
2. Update CLI.md when adding new commands or options
3. Follow the existing pattern in the parser trait for new languages
4. Ensure all file paths are stored with symbols for navigation
5. Run tests with `cargo test` before committing
6. Use `cargo clippy` for linting
7. Track development progress in TODO.md, not here

## Important Library Notes

### Rand Crate API (v0.9.0+)
The rand crate made breaking changes in v0.9.0 to prepare for Rust 2024:
- Use `rand::rng()` NOT `rand::thread_rng()` (deprecated)
- Use `rng.random()` NOT `rng.gen()` (deprecated)
- Use `rng.random_range()` NOT `rng.gen_range()` (deprecated)
- Use `rng.random_bool()` NOT `rng.gen_bool()` (deprecated)

These changes avoid conflicts with Rust 2024's new `gen` keyword.

## Important Notes

- The README file contains the complete technical architecture and should be consulted for detailed design decisions
- Focus on maintaining the performance targets outlined above
- Use Rust idioms and leverage the type system for safety
- Prioritize zero-copy operations and memory efficiency
- You **MUST** follow "## Development Guidelines"
