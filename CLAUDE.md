# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a high-performance code intelligence system written in Rust, designed to provide AI assistants with deep understanding of codebases. The system features production-ready semantic search capabilities, comprehensive relationship tracking, and an MCP server for seamless AI integration.

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
./target/release/codanna index src --progress

# Find a symbol by name
./target/release/codanna retrieve symbol SimpleIndexer

# Show function dependencies
./target/release/codanna retrieve dependencies parse_function

# Natural language search
./target/release/codanna mcp semantic_search_docs --args '{"query": "parse JSON data", "limit": 5}'

# Comprehensive context search (dependencies + callers + impact)
./target/release/codanna mcp semantic_search_with_context --args '{"query": "authentication", "limit": 3}'

# Start MCP server
./target/release/codanna serve
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

The system provides:

**Core Indexing & Search:**
- Index both single files and entire directory trees at 10,000+ files/second
- Extract symbols (functions, methods, structs, traits) from Rust code
- Detect and track relationships between symbols (calls, implements, uses, defines)
- Full-text search with fuzzy matching support
- Persist and load indexes from disk
- Provide comprehensive querying capabilities via CLI
- Serve as an MCP server for AI assistant integration
- Report progress and performance metrics during indexing

**Semantic Search (Production Ready):**
- **Natural Language Search**: Query code using descriptive language instead of exact names
- **Semantic Embeddings**: 384-dimensional vectors using AllMiniLML6V2 model
- **Smart Storage**: Memory-mapped files with <1μs access time
- **Embedding Lifecycle**: Automatic cleanup during re-indexing prevents accumulation
- **MCP Integration**: Three semantic search tools available:
  - `semantic_search_docs`: Basic natural language search
  - `semantic_search_with_context`: Comprehensive single-query analysis (the "powerhorse" tool)
  - `search_symbols`: Full-text fuzzy search
- **Configuration**: Simple enable/disable via settings.toml
- **Performance**: <10ms search latency, 1.5KB per symbol overhead

**MCP Tools Available:**
1. **find_symbol** - Find symbols by exact name
2. **get_calls** - Show what a function calls
3. **find_callers** - Show what calls a function
4. **analyze_impact** - Analyze change impact radius
5. **get_index_info** - Get index statistics
6. **search_symbols** - Full-text fuzzy search
7. **semantic_search_docs** - Natural language documentation search
8. **semantic_search_with_context** - Enhanced search with full context (dependencies, callers, impact)

**Performance Achievements:**
- Core indexing: 10,000+ files/second maintained
- Semantic indexing: ~50-100ms per documented symbol
- Search latency: <10ms for both text and semantic search
- Memory usage: ~100MB for 1M symbols + 1.5KB per embedding
- Embedding cleanup: Zero accumulation during re-indexing
- Storage efficiency: Memory-mapped files with instant loading

## Configuration

To enable semantic search, add to `.codanna/settings.toml`:

```toml
[semantic_search]
enabled = true
model = "AllMiniLML6V2"
threshold = 0.6  # Optional: default similarity threshold
```

After enabling, re-index your codebase:
```bash
cargo run -- index . --force
```

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
