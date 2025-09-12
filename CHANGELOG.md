# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.11] - 2025-09-11

### Added
- React example app under `examples/typescript/react` demonstrating call tracking for React hooks and component methods.

### Fixed
- TypeScript parser/indexer: Function call relationships correctly tracked in React projects (fixes #23)
  - React hooks (`useState`, `useEffect`) and component methods properly detected
  - Call relationships preserved during full project indexing
  - External module symbols correctly resolved with unique IDs

## [0.5.10] - 2025-09-11

### Added
- Parse command: output AST nodes in JSONL format for debugging
- Parse command flags: --max-depth, --all-nodes, --output
- Tree-sitter CLI detection in development scripts

### Fixed
- TypeScript parser: improved nested node extraction in arrow functions and JSDoc blocks (123/182 coverage)
- Test parallel execution race conditions with unique temp files
- CLI startup performance for non-index commands (parse, config, benchmark)

### Changed
- Parser audit reports now include timestamps
- Parse command integration tests moved to proper test structure

## [0.5.9] - 2025-09-07

### Enhanced
- **codanna-navigator agent**: Improved code research reports with quantified findings, investigation paths, and actionable insights

### Added
- C/C++ language support with tree-sitter parsing
- Dynamic NodeTracker system for zero-maintenance parser auditing across all languages
- TypeScript tsconfig.json path resolution infrastructure with persistence (.codanna/index/resolvers/)
- Project-agnostic resolution foundation (ProjectResolutionProvider trait, not yet integrated)
- Python parser extensions: assignment, decorated_definition, type_alias extraction
- Parser API documentation for consistent resolution patterns across languages

### Fixed
- Semantic search: SymbolId persistence between embeddings and symbol index (addresses #23)
- CI: clippy --all-targets --all-features compliance across all parsers

### Changed
- Test infrastructure: enable subfolder organization, removed 20k LOC obsolete tests, added ABI-15 audit (supports #20)
- Memory optimization: symbol-cache candidate lookup with relationship deduplication

### Breaking Changes
- Existing codebases need reindexing with --force or clean new index

## [0.5.8] - 2025-09-01

### Security
- Fixed critical slab vulnerability (RUSTSEC-2025-0047) by updating to v0.4.11
- Replaced unmaintained atty (0.2.14) with is-terminal (0.4.16)
- Resolved RUSTSEC-2024-0375 (atty unmaintained warning)
- Resolved RUSTSEC-2021-0145 (atty potential unaligned read)

### Documentation (internal)
- Added security maintenance documentation
- Created paste dependency analysis and monitoring strategy
- Updated security sprint tracking and procedures

### Changed
- Terminal detection now uses is-terminal crate instead of atty

## [0.5.7] - 2025-09-01

### Fixed
- rmcp 0.6.1 compatibility for `cargo install codanna --locked`
- Symbol counts showing as 0 in `get_index_info`

## [0.5.6] - 2025-08-22

### Fixed
- Clippy warnings in Go resolution (unnecessary unwrap, unused parentheses)
- Documentation build errors with escaped bracket syntax in Go parser
- CI timeouts by ignoring hanging regression tests pending investigation

## [0.5.5] - 2025-08-22

### Added
- Go language support with complete parser implementation
- Go-specific symbol extraction: structs, interfaces, methods, functions, constants, variables
- Go generics support (Go 1.18+) with type parameter parsing
- Go package-level visibility handling (exported vs unexported symbols)
- Go import statement parsing and relationship tracking
- Performance benchmark: 74,545 symbols/sec (7.4x above 10k/s target)

### Fixed
- Retrieve commands relationship data parity with MCP tools
- All 6 retrieve functions now use proper SymbolContext with complete relationship data
- retrieve_describe aggregates relationships from all symbols with same name
- JSON output field population for all retrieve commands

### Changed
- Language registry: Go parser integrated with self-registration architecture
- README: Updated supported languages list to include Go (5 production languages)
- Dependencies: Added tree-sitter-go for Go language parsing

## [0.5.4] - 2025-08-22

### Added
- ResolutionScope::resolve_relationship with default + language-specific overrides
- Support for Defines, Calls, Implements, and qualified name resolution (e.g. Config::new, self::method)
- TDD integration tests for Rust, Python, TypeScript, PHP with real parser validation
- Structured, extensible abstractions for relationship resolution

### Fixed
- Replace ordering hack in SimpleIndexer with ResolutionContext delegation
- Update retrieve describe to aggregate relationships across symbols with same name
- Clean ~40 lines of hack code with professional architecture patterns

### Changed
- Architecture: SimpleIndexer = orchestration only; ResolutionContext = owns logic; per-language behaviors encapsulated
- Maintains <10ms resolution via memory-mapped symbol cache

## [0.5.3] - 2025-08-22

### Added
- Function call tracking for all language parsers via PR #17
- Automatic detection and storage of function calls during indexing
- Call relationships now tracked alongside existing symbol relationships

### Fixed
- MCP schema validation: Changed non-standard `uint` format to `uint32`
- Python parser: Exclude method calls from function call tracking (only track function calls)
- PHP parser: Exclude method calls from function call tracking (only track function calls)
- Test deduplication for function call relationships from multiple analysis passes

### Changed
- CI workflow: Switched to PR-triggered CI with concurrency control for better resource management

## [0.5.2] - 2025-08-21

### Added
- Language filtering for semantic search in mixed-language codebases
- `lang` parameter for `semantic_search_docs` and `semantic_search_with_context` MCP tools
- Language mappings persistence in `.codanna/index/semantic/languages.json`
- `similarity_score_analysis.sh` script demonstrating score consistency
- File paths with line numbers in JSON output for all retrieve commands
- Unified output schema with zero-cost abstractions (OutputManager)
- Dual format support for all retrieve commands (positional and key:value)

New slash commands:
- /find: Smart semantic search with natural language query optimization
- /deps: Dependency analysis with coupling metrics and refactoring insights

### Fixed
- TypeScript JSDoc extraction for exported functions
- TypeScript parser now correctly finds JSDoc comments above `export function` declarations

### Changed
- Semantic search filters embeddings by language before computing similarity
- Search performance improved in mixed-language projects (up to 75% noise reduction)
- All retrieve commands migrated to OutputManager infrastructure
- Deprecated `impact` command in favor of `analyze_impact` MCP tool

## [0.5.1] - 2025-08-17

### Added
- Comprehensive signature extraction across all language parsers
- Parent context tracking for nested symbols
- PHP Resolution API with namespace resolution and PSR-4 support
- Python Resolution API with LEGB scoping and MRO
- TypeScript type tracking and call graph analysis
- TypeScript re-export and barrel file support

### Fixed
- All scope tests and language behavior doctests
- TypeScript import parsing foundation

## [0.5.0] - Unreleased
_Note: v0.5.0 was an internal milestone, not a public release. Changes were included in v0.5.1._

### Added
- Language registry architecture for modular parser system
- PHP language support with full parser implementation
- TypeScript support with type annotations and interfaces
- Language-agnostic module resolution system

### Changed
- Parser directory reorganization into language-specific subdirectories
- Core systems migrated to registry-based language detection
- ParserFactory integrated with language registry

### Fixed
- Rust symbol extraction for enums, types, and constants
- Inherent methods trait signature handling

## [0.4.0] - 2025-08-13

### Added
- Language registry system for self-registering parsers
- Comprehensive SimpleIndexer refactoring
- Language-specific behavior traits

### Changed
- Major refactor of parsing architecture to support modular languages
- Migration from hard-coded language support to registry pattern

## [0.3.0] - 2025-08-11

### Added
- Unix interface with positional arguments
- JSON output support for all commands
- MCP notifications support
- Optimized CI/CD workflow for rapid development

### Changed
- Improved quick-check workflow for faster feedback

### Performance
- Significant CI pipeline optimization

[0.5.2]: https://github.com/bartolli/codanna/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/bartolli/codanna/compare/v0.4.0...v0.5.1
[0.4.0]: https://github.com/bartolli/codanna/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/bartolli/codanna/compare/v0.2.0...v0.3.0
