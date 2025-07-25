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

## ✅ Phase 2: Storage Layer (MAJOR REFACTORING COMPLETED - JAN 2025)

### ✅ 2.1 Tantivy-Only Architecture
- **Status**: COMPLETED (Refactored Jan 2025)
- **Files**: `src/storage/tantivy.rs`, `src/storage/error.rs`, `src/storage/metadata_keys.rs`
- **Major Changes**:
  - Completely removed bincode storage - Tantivy is now single source of truth
  - Replaced `Box<dyn Error>` with proper `StorageError` using thiserror
  - Added type-safe `MetadataKey` enum replacing string literals
  - Fixed cross-file relationship resolution (0 → 598 relationships)
  - Implemented Arc<str> for memory-efficient string sharing

### ✅ 2.2 Graph Structure (RETAINED)
- **Status**: COMPLETED
- **Files**: `src/storage/graph.rs`
- **Tests**: 5 tests passing
- **What we built**: DependencyGraph wrapping petgraph, BFS traversal, path finding, impact analysis

### ✅ 2.3 Hybrid Persistence → Tantivy-Only Persistence
- **Status**: REFACTORED (Jan 2025)
- **Files**: `src/storage/persistence.rs`, `src/storage/metadata.rs`
- **What we built**: 
  - Originally: SQLite + bincode hybrid approach
  - **Now**: Pure Tantivy persistence with memory-mapped files
  - Removed unused checksum field from IndexMetadata
  - Enhanced metadata tracking with DataSource enum

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

## ✅ Phase 4: Indexing Pipeline (MAJOR REFACTORING COMPLETED - JAN 2025)

### ✅ 4.1 File Walker
- **Status**: COMPLETED
- **Files**: `src/indexing/walker.rs`
- **Tests**: 3 tests passing
- **What we built**: FileWalker with .gitignore support, language filtering

### ✅ 4.2 Simple Indexer (FULLY REFACTORED)
- **Status**: COMPLETED (Major refactoring Jan 2025)
- **Files**: `src/indexing/simple.rs`
- **Major Changes**:
  - Complete rewrite for Tantivy-only architecture
  - Decomposed 99-line `reindex_file_content` into 8 focused helper methods
  - Fixed cross-file relationship resolution with two-pass approach
  - Implemented proper hash-based change detection
  - Added `IndexingResult` enum for clear messaging (indexed vs cached)
  - Enhanced error handling with structured `StorageError` types

### ✅ 4.3 Progress Reporting
- **Status**: COMPLETED
- **Files**: `src/indexing/progress.rs`
- **What we built**: Real-time progress with ETA, performance metrics

### 🔲 4.4 Parallel Indexer
- **Status**: NOT STARTED
- **Why needed**: Current performance ~19 files/sec, target 1000+ files/sec
- **Implementation plan**: Use rayon for parallel file processing

## ✅ Phase 5: CLI Interface (ENHANCED - JAN 2025)

### ✅ 5.1 Basic CLI Structure
- **Status**: COMPLETED & ENHANCED (Jan 2025)
- **Files**: `src/main.rs`, `CLI.md`
- **What we built**: Full CLI with all commands documented
- **Recent improvements**: Fixed multiple symbol handling, enhanced error messages

### ✅ 5.2 Index Command (ENHANCED)
- **Status**: COMPLETED & ENHANCED (Jan 2025)
- **Features**: Single file, directory, progress, dry-run, max-files
- **New features**:
  - Proper `--force` flag behavior (directories vs files)
  - Hash-based change detection with clear messaging
  - `--config` flag for custom settings.toml files
  - Better progress reporting (indexed vs cached files)

### ✅ 5.3 Retrieve Commands (FIXED)
- **Status**: COMPLETED & FIXED (Jan 2025)
- **Commands**: symbol, calls, callers, implementations, uses, defines, impact, dependencies
- **Fixed**: Multiple symbols with same name now properly handled

### ✅ 5.4 Configuration (ENHANCED)
- **Status**: COMPLETED & ENHANCED (Jan 2025)
- **Commands**: init, config
- **Files**: Settings stored in `.codanna/settings.toml`
- **New features**: `--config` flag allows loading custom configuration files

## ✅ Phase 6: MCP Integration (COMPLETED & ENHANCED - JAN 2025)

### ✅ 6.1 MCP Server Implementation
- **Status**: COMPLETED & ENHANCED (Jan 2025)
- **Files**: `src/mcp/mod.rs`
- **What we built**: 
  - MCP server with stdio transport
  - Tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info
  - Serve command in CLI
- **Enhanced**: Fixed multiple symbol handling for consistency with CLI

### ✅ 6.2 MCP Client Testing
- **Status**: COMPLETED
- **Files**: `src/mcp/client.rs`
- **Commands**: mcp-test, mcp (embedded mode)

### 🔲 6.3 Claude Desktop Integration
- **Status**: NOT TESTED
- **Next steps**: Test with actual Claude Desktop, create example configuration

## 🎯 High Priority Tasks (MVP Completion)

### 1. ✅ Cross-File Relationship Building (COMPLETED - JAN 2025)
- **Status**: COMPLETED (Major breakthrough Jan 2025)
- **Previous issue**: Only linked symbols within same file (0 relationships)
- **Solution implemented**:
  - Two-pass relationship resolution approach
  - Commit symbols to Tantivy after each file before processing next
  - Proper cross-file symbol lookup and linking
  - **Result**: Fixed 0 relationships → 598 relationships detected
- **Files**: `src/indexing/simple.rs` (completely refactored)

### 2. ✅ Tantivy Integration (COMPLETED - 2024)
- **Status**: COMPLETED
- **What we built**:
  - Full Tantivy schema for code symbols and relationships
  - Symbol names, documentation, and metadata indexing
  - Search query interface with CLI and MCP tools
  - **Enhanced**: Now single source of truth (removed bincode)

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

## 📊 Current Metrics (Updated Jan 2025)

- **Test count**: 42+ tests passing (some updated for Tantivy-only architecture)
- **Supported languages**: Rust (fully implemented)
- **Language infrastructure**: Ready for Python, JavaScript, TypeScript, Go
- **Index scope**: Single files and full directories
- **Relationship types**: Calls, Implements, Uses, Defines (all working cross-file)
- **Cross-file relationships**: ✅ WORKING (598 relationships detected vs 0 before)
- **Performance**: ~19 files/second (not optimized, but stable with Tantivy)
- **Persistence**: **Pure Tantivy-based** (removed SQLite/bincode hybrid)
- **MCP Server**: Implemented with 5 tools, enhanced for multiple symbols
- **Progress reporting**: Real-time with ETA + indexed vs cached messaging
- **Configuration**: Enhanced with `--config` flag support
- **Error handling**: Proper `thiserror` types throughout
- **Architecture**: Single source of truth with memory-mapped Tantivy storage

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

## Success Criteria Checklist (Updated Jan 2025)

- [x] Can index entire Rust project ✅
- [x] Detects all major relationship types ✅
- [x] Relationships work across file boundaries ✅ **FIXED JAN 2025**
- [x] Can query impact of changes ✅
- [x] Can find all implementations ✅
- [x] Index persists between runs ✅
- [x] MCP server works ✅
- [x] Documentation search with Tantivy ✅ **COMPLETED**
- [ ] MCP tested with Claude Desktop ❌
- [ ] Performance: 1000+ files/second ❌
- [ ] Multiple language support ❌ (only Rust)

## 🎉 Major Accomplishments (Jan 2025 Refactoring)

1. **Architecture Simplification**: Removed complex bincode/SQLite hybrid → Pure Tantivy
2. **Cross-File Relationships**: Fixed from 0 to 598 detected relationships
3. **Type Safety**: Replaced `Box<dyn Error>` with proper `StorageError` types
4. **Memory Efficiency**: Implemented `Arc<str>` for shared string data
5. **CLI Enhancements**: Fixed multiple symbol handling, added `--config` flag
6. **Code Quality**: Decomposed complex methods, added Debug implementations
7. **User Experience**: Clear messaging for indexed vs cached files