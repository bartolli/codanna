# Codebase Intelligence - Development TODO

This document tracks the development progress following TDD principles. Each task must have passing tests before moving to the next.

## ✅ Phase 1: Core Data Structures (COMPLETED)

### ✅ 1.1 Basic Type Definitions
- **Status**: COMPLETED
- **Files**: `src/types/mod.rs`
- **Tests**: 7 tests passing
- **What we built**: SymbolId, FileId with NonZeroU32 for memory efficiency, Range for positions, SymbolKind enum
- **Why**: Type safety and zero-cost abstractions are fundamental to the entire system

### ✅ 1.2 Symbol Structure
- **Status**: COMPLETED
- **Files**: `src/symbol/mod.rs`
- **Tests**: 6 tests passing
- **What we built**: Symbol struct, CompactSymbol (32-byte aligned), StringTable for interning
- **Why**: Symbols are the core unit of code intelligence; compact representation enables processing millions of symbols

### ✅ 1.3 Relationship Types
- **Status**: COMPLETED
- **Files**: `src/relationship/mod.rs`
- **Tests**: 7 tests passing
- **What we built**: RelationKind enum, Relationship struct with weights, metadata support
- **Why**: Relationships define how code elements connect; weights enable ranking and metadata provides context

## ✅ Phase 2: Storage Layer (COMPLETED)

### ✅ 2.1 In-Memory Symbol Store
- **Status**: COMPLETED
- **Files**: `src/storage/memory.rs`
- **Tests**: 9 tests passing
- **What we built**: Thread-safe SymbolStore using DashMap, indexes by name/file/kind
- **Why**: Concurrent access is critical for parallel indexing; multiple indexes enable fast queries

### ✅ 2.2 Graph Structure
- **Status**: COMPLETED
- **Files**: `src/storage/graph.rs`
- **Tests**: 5 tests passing
- **What we built**: DependencyGraph wrapping petgraph, BFS traversal, path finding
- **Why**: Graph algorithms are essential for impact analysis and understanding code relationships

## ✅ Phase 3: Parser Integration (PARTIALLY COMPLETED)

### 🔲 3.1 Parser Pool
- **Status**: SKIPPED FOR NOW
- **Note**: We implemented a simple parser first to validate the concept
- **Next Steps**: 
  1. Write test for creating language-specific parsers
  2. Test thread-local parser reuse
  3. Test concurrent parser access
- **Implementation Plan**:
  - Create `src/parsing/pool.rs`
  - Use ThreadLocal<RefCell<HashMap<Language, Parser>>>
  - Implement borrowing mechanism with RAII guards
- **Why**: Parser creation is expensive; reusing parsers improves performance by 10x

### ✅ 3.2 Basic Rust Parser
- **Status**: COMPLETED
- **Files**: `src/parsing/rust.rs`
- **Tests**: 9 tests passing
- **What we built**:
  - Tree-sitter based Rust parser
  - Extracts functions, methods, structs, and traits
  - Detects function calls for relationship building
  - Correctly distinguishes between functions and methods in impl blocks
  - Detects trait implementations (`impl Trait for Type`)
  - Detects type usage in struct fields and function signatures
  - Detects method definitions in traits and impl blocks
- **Why**: Starting with Rust allows us to test on our own codebase

### 🔲 3.3 Language Detection
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test detecting language from file extension
  2. Test detecting from shebang lines
  3. Test fallback strategies
- **Implementation Plan**:
  - Create `src/parsing/language.rs`
  - Map extensions to Language enum
  - Support content-based detection
- **Why**: Automatic language detection enables processing mixed codebases

## ✅ Phase 4: Basic Indexing Pipeline (PARTIALLY COMPLETED)

### 🔲 4.1 File Walker
- **Status**: NOT STARTED
- **Note**: We focused on single file indexing first
- **Next Steps**:
  1. Test walking directory with .gitignore respect
  2. Test filtering by language
  3. Test symlink handling
- **Implementation Plan**:
  - Create `src/indexing/walker.rs`
  - Use `ignore` crate for gitignore support
  - Yield PathBuf items for processing
- **Why**: Efficient file discovery respecting ignore rules prevents indexing unnecessary files

### ✅ 4.2 Single File Indexer
- **Status**: COMPLETED
- **Files**: `src/indexing/simple.rs`
- **Tests**: 6 tests passing
- **What we built**:
  - SimpleIndexer that indexes single Rust files
  - Extracts symbols and stores them in SymbolStore
  - Builds all relationship types in DependencyGraph:
    - Function calls (Calls)
    - Trait implementations (Implements)
    - Type usage (Uses)
    - Method definitions (Defines)
  - Provides queries for symbols and relationships
- **Why**: Single file indexing is the atomic unit that will be parallelized
  2. Test symbol extraction accuracy
  3. Test relationship detection (e.g., function calls)
  4. Test incremental update detection
- **Implementation Plan**:
  - Create `src/indexing/single.rs`
  - Parse file → Extract symbols → Detect relationships → Store
  - Track file hash for change detection
- **Why**: Single file indexing is the atomic unit that will be parallelized

### 🔲 4.3 Parallel Indexer
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test processing multiple files concurrently
  2. Test work distribution across threads
  3. Test progress reporting
  4. Benchmark against performance target (10k files/sec)
- **Implementation Plan**:
  - Create `src/indexing/parallel.rs`
  - Use rayon with custom chunk size
  - Implement work-stealing queue
  - Batch symbol insertions
- **Why**: Parallel processing is required to meet our performance targets

### 🔲 4.4 Incremental Indexer
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test detecting changed files via hash
  2. Test reindexing only affected files
  3. Test dependency invalidation
  4. Test <100ms update performance
- **Implementation Plan**:
  - Create `src/indexing/incremental.rs`
  - Track file hashes and timestamps
  - Build file dependency graph
  - Implement smart invalidation
- **Why**: Incremental updates keep the index fresh during development

## 🔲 Phase 5: Query Interface

### 🔲 5.1 Basic Symbol Queries
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test finding symbol by exact name
  2. Test fuzzy name matching
  3. Test filtering by kind/file
  4. Test query performance <10ms
- **Implementation Plan**:
  - Create `src/query/symbol.rs`
  - Implement exact and fuzzy matching
  - Add query builders for complex filters
- **Why**: Symbol lookup is the most common query operation

### 🔲 5.2 Relationship Queries
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test finding all callers of a function
  2. Test finding implementations of trait
  3. Test transitive dependency queries
  4. Test path finding between symbols
- **Implementation Plan**:
  - Create `src/query/relationship.rs`
  - Leverage graph traversal algorithms
  - Implement depth limiting
- **Why**: Understanding relationships enables impact analysis

### 🔲 5.3 Code Context Queries
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test getting symbol at file position
  2. Test gathering full context (definition, references, hierarchy)
  3. Test performance with large result sets
- **Implementation Plan**:
  - Create `src/query/context.rs`
  - Combine multiple query types
  - Implement result ranking
- **Why**: Rich context is what makes code intelligence useful for AI assistants

## ✅ Phase 6: CLI Interface (BASIC VERSION COMPLETED)

### ✅ 6.1 Basic CLI Structure
- **Status**: COMPLETED
- **Files**: `src/main.rs`
- **Tests**: 1 CLI validation test
- **What we built**:
  - CLI using clap with subcommands
  - `index` command for indexing files
  - `retrieve` command with subcommands (symbol, calls, callers)
  - Basic state persistence (remembers last indexed file)
- **Why**: CLI provides the primary interface for users

### ✅ 6.2 Index Command
- **Status**: COMPLETED (single file version)
- **What we built**:
  - `index <file.rs>` - Indexes a single Rust file
  - Shows symbol count summary (functions, methods, structs, traits)
  - Saves indexed file path for subsequent retrieve commands
- **Next Steps for full version**:
  1. Add directory recursion support
  2. Add progress reporting
  3. Persist full index to disk
- **Why**: Users need to build an index before querying

### ✅ 6.3 Retrieve Commands
- **Status**: COMPLETED
- **What we built**:
  - `retrieve symbol <name>` - Find symbols by name
  - `retrieve calls <function>` - Show what a function calls
  - `retrieve callers <function>` - Show what calls a function
  - `retrieve implementations <trait>` - Show types implementing a trait
  - `retrieve uses <symbol>` - Show what types a symbol uses
  - `retrieve defines <symbol>` - Show what methods a type/trait defines
  - `retrieve impact <symbol>` - Show impact radius of changes
  - `retrieve dependencies <symbol>` - Show comprehensive dependency analysis
  - Auto re-indexes the last indexed file on retrieve
- **Next Steps**:
  1. Add JSON output format
  2. Support regex patterns
  3. Add cross-file query support
- **Why**: Interactive querying helps users explore codebases

## 🎯 MVP Completion Tasks (HIGH PRIORITY)

### Critical Missing Pieces for Robust MVP

**Suggested Implementation Order:**
1. ✅ Enhanced Relationship Detection (foundation) - COMPLETED
2. Directory Walking (multi-file support)
3. Multi-File Relationship Building (cross-file deps)
4. Basic Persistence (practical usage)
5. ✅ Essential Graph Queries (leverage the graph) - COMPLETED
6. Basic MCP Server (deliver value to AI)

#### ✅ Enhanced Relationship Detection (COMPLETED)
- **Why Critical**: Currently only detecting function calls, missing 80% of relationship types
- **Status**: COMPLETED
- **What we built**:
  1. **Detect `impl Trait for Type`** → Creates "Implements" relationships ✅
     - Parse impl blocks with trait bounds
     - Link trait to implementing type
     - Test with standard traits (Debug, Clone, etc.)
  2. **Detect struct field types** → Creates "Uses" relationships ✅
     - Parse struct field declarations
     - Create relationship to field type
     - Handle generic parameters
  3. **Detect function parameter/return types** → Creates "Uses" relationships ✅
     - Parse function signatures
     - Link to parameter and return types
     - Handle references and lifetimes
  4. **Detect trait definitions** → Creates "Defines" relationships ✅
     - Parse trait method signatures
     - Link trait to its methods
  5. **Test relationship detection** ✅
     - Create comprehensive test fixtures
     - Verify all relationship types work

#### 🔲 Multi-File Relationship Building
- **Why Critical**: Real codebases have cross-file dependencies
- **Tasks**:
  1. **Symbol resolution across files**
     - Build symbol table for entire index
     - Resolve `use` statements
     - Handle module paths
  2. **Cross-file relationship detection**
     - Link calls to definitions in other files
     - Handle pub/private visibility
  3. **Test multi-file scenarios**
     - Create multi-file test project
     - Verify relationships span files

#### ✅ Essential Graph Queries (COMPLETED)
- **Why Critical**: The graph's value is in traversal and analysis
- **Status**: COMPLETED
- **What we built**:
  1. **Impact analysis query** ✅
     ```rust
     pub fn get_impact_radius(&self, symbol: SymbolId, max_depth: usize) -> Vec<SymbolId>
     ```
     - Find all symbols affected by changing one symbol
     - Use BFS with relationship filtering
  2. **Dependency analysis query** ✅
     ```rust
     pub fn get_dependencies(&self, symbol_id: SymbolId) -> HashMap<RelationKind, Vec<SymbolId>>
     pub fn get_dependents(&self, symbol_id: SymbolId) -> HashMap<RelationKind, Vec<SymbolId>>
     ```
     - Find all symbols that a given symbol depends on
     - Find all symbols that depend on a given symbol
     - Group by relationship type for clarity
  3. **Find implementations query** ✅
     ```rust
     pub fn get_implementations(&self, trait_id: SymbolId) -> Vec<Symbol>
     ```
     - Find all types implementing a trait
  4. **Add to CLI** ✅
     - `retrieve impact <symbol>` command
     - `retrieve dependencies <symbol>` command
     - `retrieve implementations <trait>` command
     - `retrieve uses <symbol>` command
     - `retrieve defines <symbol>` command

#### 🔲 Basic Persistence
- **Why Critical**: Re-indexing on every command is not scalable
- **Tasks**:
  1. **Save index to disk**
     - Serialize SymbolStore
     - Serialize DependencyGraph
     - Use rkyv for zero-copy deserialization
  2. **Load index from disk**
     - Detect saved index
     - Load on CLI startup
     - Version compatibility check
  3. **Incremental updates**
     - Detect file changes
     - Update only changed symbols
     - Maintain graph consistency

#### 🔲 Directory Walking
- **Why Critical**: Single file indexing is too limited
- **Tasks**:
  1. **Implement directory walker**
     - Use `ignore` crate for .gitignore
     - Filter by .rs extension
     - Progress reporting
  2. **Update CLI**
     - Support directory path in index command
     - Show file count and progress
  3. **Handle large codebases**
     - Test on rust-lang/rust repo
     - Memory usage monitoring

#### 🔲 Basic MCP Server Implementation
- **Why Critical**: MCP integration is the primary use case for AI assistants
- **Tasks**:
  1. **Implement MCP server mode**
     - Add `serve` command to CLI
     - Use rmcp crate for server implementation
     - Handle connection lifecycle
  2. **Core MCP tools**:
     ```rust
     #[tool(description = "Find symbol by name")]
     find_symbol(name: String) -> Vec<Symbol>
     
     #[tool(description = "Get symbol definition and context")]
     get_symbol_context(symbol_id: String) -> SymbolContext
     
     #[tool(description = "Find who calls this function")]
     find_callers(function_name: String) -> Vec<Symbol>
     
     #[tool(description = "Analyze impact of changing a symbol")]
     analyze_impact(symbol_name: String, max_depth: Option<usize>) -> ImpactAnalysis
     
     #[tool(description = "Find implementations of a trait")]
     find_implementations(trait_name: String) -> Vec<Symbol>
     ```
  3. **Context management**
     - Load persisted index on startup
     - Handle multiple concurrent requests
     - Efficient response formatting
  4. **Test with Claude Desktop**
     - Create test configuration
     - Verify tool discovery
     - Test real queries
  5. **Add to CLAUDE.md**
     - Document MCP server usage
     - Example tool invocations
     - Integration guide

### MVP Success Criteria
- [ ] Can index entire Rust project (multiple files)
- [x] Detects all major relationship types (calls, implements, uses, defines) ✅
- [ ] Relationships work across file boundaries
- [x] Can query impact of changes ✅
- [x] Can find all implementations of a trait ✅
- [ ] Index persists between runs
- [ ] MCP server works with Claude Desktop
- [ ] Performance: 1000+ files/second on single thread

### After MVP
- Parser pool optimization
- Additional languages (Python, TypeScript)
- Full-text search integration
- Semantic search with embeddings
- Advanced MCP tools (refactoring, code generation)

## 🔲 Phase 7: Persistence Layer

### 🔲 7.1 Binary Serialization
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test serializing symbol store
  2. Test serializing graph
  3. Test zero-copy deserialization with rkyv
  4. Test file size efficiency
- **Implementation Plan**:
  - Implement rkyv traits for all types
  - Create versioned file format
  - Add compression with lz4
- **Why**: Persistence enables reusing indexes across sessions

### 🔲 7.2 SQLite Metadata Store
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test storing file metadata
  2. Test querying by various attributes
  3. Test transaction performance
- **Implementation Plan**:
  - Create schema with sqlx migrations
  - Store non-critical metadata
  - Implement async queries
- **Why**: SQLite provides flexible querying for metadata without impacting core performance

## 🔲 Phase 8: Search Features

### 🔲 8.1 Full-Text Search
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test indexing symbol names and docs
  2. Test search query parsing
  3. Test ranking algorithms
  4. Benchmark search performance
- **Implementation Plan**:
  - Create `src/search/text.rs`
  - Build tantivy schema
  - Implement custom tokenizers for code
- **Why**: Text search complements structural queries

### 🔲 8.2 Semantic Search
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test embedding generation for symbols
  2. Test HNSW index building
  3. Test similarity search accuracy
  4. Test <10ms query performance
- **Implementation Plan**:
  - Create `src/search/semantic.rs`
  - Use candle for embeddings
  - Build HNSW index
  - Implement batched processing
- **Why**: Semantic search finds conceptually similar code

## 🔲 Phase 9: MCP Server

### 🔲 9.1 MCP Tool Implementation
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test tool registration
  2. Test request/response handling
  3. Test error handling
- **Implementation Plan**:
  - Create `src/mcp/mod.rs`
  - Implement rmcp tool traits
  - Define tool schemas
- **Why**: MCP integration enables AI assistants to use our tools

### 🔲 9.2 MCP Server Mode
- **Status**: NOT STARTED
- **Next Steps**:
  1. Test server startup and shutdown
  2. Test concurrent request handling
  3. Test memory management under load
- **Implementation Plan**:
  - Add MCP server to main.rs
  - Implement connection handling
  - Add request routing
- **Why**: Server mode provides always-ready code intelligence

## Performance Benchmarks to Implement

1. **Indexing Throughput**: Target 10,000+ files/second
2. **Memory Usage**: Target ~100 bytes per symbol
3. **Search Latency**: Target <10ms for queries
4. **Incremental Update**: Target <100ms per file
5. **Startup Time**: Target <1s with cache

## Testing Strategy

- Each module has unit tests (current: 33 passing)
- Integration tests for end-to-end workflows
- Property-based tests for data structures
- Benchmarks for performance-critical paths
- Fuzzing for parser robustness

## Notes for Future Sessions

1. Always run `cargo test` before starting new work
2. Check this TODO.md for next tasks
3. Follow TDD: Write test → See it fail → Implement → See it pass
4. Update this file after completing tasks
5. Keep performance targets in mind during implementation

## Current State Summary (Updated)

### ✅ Completed
- Core data structures implemented and tested
- Storage layer with concurrent access ready
- Rust parser with comprehensive relationship detection
- Single file indexer with all relationship types
- Full-featured CLI with 8 different query types
- Demo script showcasing POC capabilities

### 🎯 What Works Now
- Can index single Rust files
- Extracts functions, methods, structs, and traits
- Detects ALL major relationship types:
  - Function calls (who calls whom)
  - Trait implementations (type implements trait)
  - Type usage (in fields, parameters, returns)
  - Method definitions (trait/type defines methods)
- Comprehensive queries:
  - Find symbols by name
  - Show function call graphs (calls/callers)
  - Find trait implementations
  - Show type usage
  - Show method definitions
  - Impact analysis (what breaks if I change X)
  - Full dependency analysis (incoming/outgoing)
- Basic state persistence between commands

### 🚧 Next Priority Items (Following Recommended Strategy)
1. **Directory walking** - Index multiple files recursively (MVP CRITICAL)
2. **Basic Persistence** - Save/load index between runs (MVP CRITICAL)
3. **Cross-file relationships** - Resolve symbols across files (MVP CRITICAL)
4. **Basic MCP Server** - Deliver value to AI assistants (MVP CRITICAL)
5. **Additional languages** - Python, TypeScript, Go (POST-MVP)

### Recommended Strategy Updates:
- **Skip for MVP**: Parser pool optimization, full language detection system
- **Focus on**: Getting multi-file indexing working with basic persistence
- **Incremental approach**: Add languages one at a time based on user needs

### 📊 Metrics
- **Current test count**: 48 tests passing
- **Supported languages**: Rust only
- **Index scope**: Single file at a time
- **Relationship types**: Calls, Implements, Uses, Defines
- **Performance**: Not yet optimized