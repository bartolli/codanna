# Source File Map

**Generated**: 2025-07-26  
**Project Type**: Rust Code Intelligence System  
**Primary Language**: Rust  

## 📁 Project Structure

### Configuration
- `Cargo.toml` - Rust project manifest defining codanna package and dependencies
- `clippy.toml` - Linter configuration for code quality
- `.codanna/settings.toml` - Default application settings
- `.mcp.json` - Model Context Protocol server configuration

### Core Source (`src/`)

#### Entry Points
- `main.rs` - CLI application entry with command parsing and routing
- `lib.rs` - Public API exports for library consumers

#### Error Handling
- `error.rs` - Structured error types using thiserror for all modules
- `config.rs` - Layered configuration system (TOML, env vars, CLI)

### Module Architecture

#### Types (`src/types/`)
- `mod.rs` - Core type definitions and re-exports

#### Symbol Management (`src/symbol/`)
- `mod.rs` - Symbol representation and compact storage structures

#### Relationship Tracking (`src/relationship/`)
- `mod.rs` - Code relationship graph (calls, implements, uses)

#### Language Parsing (`src/parsing/`)
- `mod.rs` - Parser module exports
- `parser.rs` - Common parser trait definition
- `rust.rs` - Rust language parser implementation
- `language.rs` - Language enumeration and detection
- `factory.rs` - Parser factory for language selection

#### Indexing Engine (`src/indexing/`)
- `mod.rs` - Indexing module exports
- `simple.rs` - Main indexer implementation
- `walker.rs` - Parallel file system traversal
- `file_info.rs` - File metadata and hash calculations
- `progress.rs` - Indexing statistics and progress tracking
- `resolver.rs` - Import and dependency resolution
- `transaction.rs` - Atomic index update operations
- `simple_old.rs` - Legacy indexer (deprecated)

#### Storage Layer (`src/storage/`)
- `mod.rs` - Storage module exports
- `tantivy.rs` - Full-text search index using Tantivy
- `persistence.rs` - Index save/load operations
- `metadata.rs` - Index metadata and versioning
- `metadata_keys.rs` - Metadata key constants
- `graph.rs` - Dependency graph storage (deprecated)
- `memory.rs` - In-memory symbol store (deprecated)
- `error.rs` - Storage-specific error types

#### MCP Integration (`src/mcp/`)
- `mod.rs` - Model Context Protocol server exports
- `client.rs` - MCP client implementation

## 🔗 Key Relationships

1. **CLI Flow**: `main.rs` → `Commands` → `SimpleIndexer` → `Storage`
2. **Parsing Pipeline**: `FileWalker` → `ParserFactory` → `RustParser` → `Symbol`
3. **Indexing Flow**: `SimpleIndexer` → `IndexTransaction` → `DocumentIndex` → `IndexPersistence`
4. **Storage Layer**: `DocumentIndex` (Tantivy) → `IndexMetadata` → File System
5. **API Surface**: External → `lib.rs` → Internal modules

## 📊 Architecture Notes

- **Pattern**: Modular service architecture with clear separation of concerns
- **Concurrency**: Parallel file processing using crossbeam channels
- **Storage**: Tantivy for full-text search, moving away from custom graph storage
- **Configuration**: Layered system supporting TOML, environment variables, and CLI args
- **Error Handling**: Structured errors with thiserror for actionable messages
- **Entry Points**: 
  - CLI: `cargo run -- <command>` via main.rs
  - Library: Import `codanna` crate via lib.rs
  - MCP Server: `cargo run -- serve` for AI assistant integration

## 🚀 Key Features

1. **Multi-language Support**: Extensible parser system (currently Rust)
2. **Incremental Indexing**: Transaction-based updates for large codebases
3. **Fast Search**: Tantivy-powered full-text and semantic search
4. **MCP Integration**: Direct AI assistant connectivity
5. **Progress Tracking**: Real-time indexing statistics
6. **Git-aware**: Respects .gitignore via ignore crate

## 📦 Dependencies Highlights

- `tantivy` - High-performance full-text search
- `tree-sitter` - Multi-language parsing framework
- `clap` - CLI argument parsing
- `figment` - Configuration management
- `dashmap` - Concurrent hash maps
- `crossbeam` - Parallel processing utilities
- `thiserror` - Error handling
- `candle` - ML inference for embeddings