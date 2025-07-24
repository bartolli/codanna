# Codebase Intelligence - Development TODO

This document tracks the development progress following TDD principles. Each task must have passing tests before moving to the next.

**Note for Claude**: See [CLAUDE.md](./CLAUDE.md) for project instructions and development guidelines.

## ✅ Phase 1: Core Data Structures (COMPLETED)

### ✅ 1.1 Basic Type Definitions
- **Status**: COMPLETED
- **Files**: `src/types/mod.rs`
- **Tests**: 7 tests passing
- **What we built**: SymbolId, FileId with NonZeroU32 for memory efficiency, Range for positions, SymbolKind enum

### ✅ 1.2 Symbol Structure
- **Status**: COMPLETED
- **Files**: `src/symbol/mod.rs`
- **Tests**: 6 tests passing
- **What we built**: Symbol struct, CompactSymbol (32-byte aligned), StringTable for interning

### ✅ 1.3 Relationship Types
- **Status**: COMPLETED
- **Files**: `src/relationship/mod.rs`
- **Tests**: 7 tests passing
- **What we built**: RelationKind enum, Relationship struct with weights, metadata support

## ✅ Phase 2: Storage Layer (COMPLETED)

### ✅ 2.1 In-Memory Symbol Store
- **Status**: COMPLETED
- **Files**: `src/storage/memory.rs`
- **Tests**: 9 tests passing
- **What we built**: Thread-safe SymbolStore using DashMap, indexes by name/file/kind

### ✅ 2.2 Graph Structure
- **Status**: COMPLETED
- **Files**: `src/storage/graph.rs`
- **Tests**: 5 tests passing
- **What we built**: DependencyGraph wrapping petgraph, BFS traversal, path finding, impact analysis

### ✅ 2.3 Basic Persistence
- **Status**: COMPLETED
- **Files**: `src/storage/persistence.rs`, `src/storage/index_data.rs`
- **What we built**: SQLite-based persistence, save/load complete index, versioning support

## ✅ Phase 3: Parser Integration (COMPLETED FOR RUST)

### ✅ 3.1 Language Abstraction
- **Status**: COMPLETED
- **Files**: `src/parsing/language.rs`, `src/parsing/parser.rs`, `src/parsing/factory.rs`
- **What we built**:
  - Language enum supporting Rust, Python, JavaScript, TypeScript
  - LanguageParser trait defining common interface
  - ParserFactory for creating language-specific parsers
  - File extension-based language detection

### ✅ 3.2 Rust Parser
- **Status**: COMPLETED
- **Files**: `src/parsing/rust.rs`
- **Tests**: 9 tests passing
- **What we built**: Complete Rust parser with all relationship types

### 🔲 3.3 Additional Language Parsers
- **Status**: Infrastructure ready, parsers not implemented
- **Priority order** (based on popularity and ecosystem importance):
  1. **JavaScript/TypeScript** - Most popular, essential for web development
  2. **Python** - Data science, ML, scripting
  3. **Go** - Cloud infrastructure, DevOps
  4. **Java** - Enterprise applications
  5. **C/C++** - System programming
- **Note**: Only Rust parser is currently implemented

## ✅ Phase 4: Indexing Pipeline (COMPLETED)

### ✅ 4.1 File Walker
- **Status**: COMPLETED
- **Files**: `src/indexing/walker.rs`
- **Tests**: 3 tests passing
- **What we built**: FileWalker with .gitignore support, language filtering

### ✅ 4.2 Simple Indexer
- **Status**: COMPLETED
- **Files**: `src/indexing/simple.rs`
- **Tests**: 1 test passing
- **What we built**: Single and multi-file indexing with all relationship types

### ✅ 4.3 Progress Reporting
- **Status**: COMPLETED
- **Files**: `src/indexing/progress.rs`
- **What we built**: Real-time progress with ETA, performance metrics

### 🔲 4.4 Parallel Indexer
- **Status**: NOT STARTED
- **Why needed**: Current performance ~19 files/sec, target 1000+ files/sec
- **Implementation plan**: Use rayon for parallel file processing

## ✅ Phase 5: CLI Interface (COMPLETED)

### ✅ 5.1 Basic CLI Structure
- **Status**: COMPLETED
- **Files**: `src/main.rs`, `CLI.md`
- **What we built**: Full CLI with all commands documented

### ✅ 5.2 Index Command
- **Status**: COMPLETED
- **Features**: Single file, directory, progress, dry-run, max-files

### ✅ 5.3 Retrieve Commands
- **Status**: COMPLETED
- **Commands**: symbol, calls, callers, implementations, uses, defines, impact, dependencies

### ✅ 5.4 Configuration
- **Status**: COMPLETED
- **Commands**: init, config
- **Files**: Settings stored in `.code-intelligence/settings.toml`

## 🚧 Phase 6: MCP Integration (PARTIALLY COMPLETED)

### ✅ 6.1 MCP Server Implementation
- **Status**: COMPLETED
- **Files**: `src/mcp/mod.rs`
- **What we built**: 
  - MCP server with stdio transport
  - Tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info
  - Serve command in CLI

### ✅ 6.2 MCP Client Testing
- **Status**: COMPLETED
- **Files**: `src/mcp/client.rs`
- **Commands**: mcp-test, mcp (embedded mode)

### 🔲 6.3 Claude Desktop Integration
- **Status**: NOT TESTED
- **Next steps**: Test with actual Claude Desktop, create example configuration

## 🎯 High Priority Tasks (MVP Completion)

### 1. 🔲 Cross-File Relationship Building
- **Why Critical**: Current implementation only links symbols within same file
- **Tasks**:
  - Implement module path resolution
  - Handle `use` statements and imports
  - Link symbols across file boundaries
  - Test on multi-file projects

### 2. 🔲 Tantivy Integration (Documentation Search)
- **Why Critical**: Need to search documentation and comments
- **Tasks**:
  - Create tantivy schema for code
  - Index symbol names and documentation
  - Implement search query interface
  - Add to CLI and MCP tools

### 3. 🔲 Performance Optimization
- **Current**: ~19 files/second
- **Target**: 1000+ files/second
- **Tasks**:
  - Implement parallel indexing with rayon
  - Add parser pool for reuse
  - Batch database operations
  - Profile and optimize hot paths

### 4. 🔲 JavaScript/TypeScript Parser
- **Why**: Most requested language after Rust
- **Tasks**:
  - Implement parser using tree-sitter-javascript/typescript
  - Handle JSX/TSX syntax
  - Extract ES6 modules, classes, functions
  - Test on popular JS frameworks

### 5. 🔲 Python Parser
- **Why**: Essential for data science and ML codebases
- **Tasks**:
  - Implement parser using tree-sitter-python
  - Handle type hints and docstrings
  - Extract classes, functions, imports
  - Test on major Python projects

## 📊 Current Metrics

- **Test count**: 50+ tests passing
- **Supported languages**: Rust (fully implemented)
- **Language infrastructure**: Ready for Python, JavaScript, TypeScript, Go
- **Index scope**: Single files and full directories
- **Relationship types**: Calls, Implements, Uses, Defines
- **Performance**: ~19 files/second (not optimized)
- **Persistence**: SQLite-based, functional
- **MCP Server**: Implemented with 5 tools
- **Progress reporting**: Real-time with ETA

## 🚀 Post-MVP Roadmap

### Phase 1: Language Support
1. JavaScript/TypeScript parser
2. Python parser
3. Go parser
4. Java parser
5. C/C++ parser

### Phase 2: Advanced Features
1. Semantic search with embeddings
2. Incremental indexing
3. Real-time file watching
4. Advanced refactoring tools
5. Code generation assistance

### Phase 3: Integrations
1. VS Code extension
2. IntelliJ plugin
3. GitHub integration
4. CI/CD integration
5. Cloud deployment

## Testing Strategy

- Unit tests for each module
- Integration tests for multi-file scenarios
- Performance benchmarks
- Language-specific test suites
- MCP integration tests

## Development Guidelines

1. Always run `cargo test` before committing
2. Update CLI.md when adding new commands
3. Follow the LanguageParser trait when adding languages
4. Maintain backward compatibility for persistence
5. Keep performance targets in mind
6. Document public APIs

## Success Criteria Checklist

- [x] Can index entire Rust project ✅
- [x] Detects all major relationship types ✅
- [ ] Relationships work across file boundaries ❌
- [x] Can query impact of changes ✅
- [x] Can find all implementations ✅
- [x] Index persists between runs ✅
- [x] MCP server works ✅
- [ ] MCP tested with Claude Desktop ❌
- [ ] Performance: 1000+ files/second ❌
- [ ] Documentation search with Tantivy ❌
- [ ] Multiple language support ❌ (only Rust)