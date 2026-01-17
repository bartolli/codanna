# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.13] - 2026-01-16

### Added

- Go project resolver provider for go.mod parsing and module path resolution
- Python project resolver with multi-backend support (Poetry, Hatch, Maturin, Setuptools, PDM, Flit)
- Kotlin project resolver with source layout config (jvm, standard-kmp, flat-kmp)
- PHP PSR-4 provider for composer.json autoload namespace resolution
- C# project resolver for .csproj SDK-style projects (RootNamespace, AssemblyName extraction)
- Shared helper functions in helpers.rs for provider implementations
- `[[languages.X.projects]]` config block for per-project source layout override
- PHP method call tracking (member_call_expression, scoped_call_expression)
- Animated spinner during pipeline warmup phase (DISCOVER/READ/PARSE/COLLECT)

### Changed

- Settings.languages uses IndexMap for alphabetical ordering in init output
- LoggingConfig.modules uses IndexMap for consistent ordering
- Empty indexes now auto-trigger force mode for dual EMBED+INDEX progress bars
- DualProgressBar shows preparing spinner when both bars at 0%

### Fixed

- Languages without providers no longer show empty config_files in settings.toml
- Rustdoc invalid-html-tags error in C# provider doc comments

## [0.9.12] - 2026-01-14

### Added

- Unified JSON envelope format (Envelope type) with schema_version, status, code, exit_code, message, hint, data, meta
- `--fields` flag for JSON field filtering on data payload (mcp and retrieve commands)
- QueryContext abstraction for symbol resolution and error handling in retrieve commands
- `lang` field to Meta struct with `with_lang()` builder method
- `paths.rs` module with normalize_for_module_path, strip_source_root, strip_extension helpers
- `format_path_as_module` as required trait method for language-specific path formatting
- Document collections info to get_index_info output

### Changed

- Migrated all 9 MCP tool commands to unified Envelope format for --json output
- Migrated retrieve symbol/callers/calls/describe/search/implementations to Envelope format
- Lazy load ML model for embedding generation (deferred until first semantic search)
- Flattened tuple output in get_calls/find_callers to avoid nested array waste
- Standardized search_symbols output with nested symbol object
- Unified logging to stderr for all commands
- Skip embedding pool loading for retrieve commands via load_facade_lite()
- `module_path_from_file` now accepts `extensions: &[&str]` parameter (fixes deadlock when registry lock held)

### Removed

- `file_extensions()` method from LanguageBehavior trait (duplicated LanguageDefinition::extensions())
- `file_extensions()` implementations from all 12 language behaviors
- `init_with_config_stderr` function (replaced by unified init_with_config)
- Tantivy-based resolution methods from trait: `resolve_import`, `build_resolution_context`, `resolve_import_path`, `classify_import_origin`, `resolve_import_path_with_context`
- Dead Go behavior methods: `get_current_package_path_for_file`, `get_project_root_for_file`
- `store_file_info` from DocumentIndex (replaced by `store_file_registration`)

### Breaking

- `--json` output format changed for mcp and retrieve commands (now uses Envelope schema)

## [0.9.11] - 2026-01-12

### Fixed

- MCP servers now load document store on all transports (stdio, HTTP, HTTPS)
- Consolidated document store loading into reusable helper

### Documentation

- Added transport notes to MCP Persistent and MCP Network docs

## [0.9.10] - 2026-01-07

### Changed

- Bump version to 0.9.10
- Update serde_json from 1.0.148 to 1.0.149
- Disable ort dev-dependency (commented out)

### Fixed

- Pipeline wall_time tracking (aa01269)
- Add EMBED to pipeline metrics (aa01269)

### Documentation

- Fix JSON examples in MCP tools
- Add lang parameter to tools documentation
- Update architecture for v0.9.0
- Add install methods documentation

### CI

- Add standalone brew-update workflow

## [0.9.0] - 2026-01-05

### Added

- Parallel indexing pipeline with 5-stage architecture (DISCOVER, READ, PARSE, COLLECT, INDEX)
- SemanticEmbedStage for parallel embedding generation
- EmbeddingPool with configurable model instances for parallel embedding
- Progress bars for Phase 1 and Phase 2 indexing with dual progress bar support
- Pipeline metrics with stage timing, memory tracking, and bottleneck detection
- GPU execution provider feature flags (CUDA, TensorRT, CoreML, DirectML, OpenVINO, ROCm)
- GPU embedding benchmarks for CPU vs accelerator comparison
- Incremental file-level change detection with mtime fast path
- Document search auto-sync with mtime-based change detection
- Install scripts for curl-based installation (install.sh, install.ps1)
- IndexFacade as unified interface wrapping DocumentIndex, Pipeline, and SemanticSearch
- `--watch` flag to mcp command for pre-tool reindex
- `--no-progress` flag (progress enabled by default)

### Changed

- Replaced SimpleIndexer with IndexFacade across codebase
- Consolidated thread settings into single parallelism value (derives stage threads)
- Batch embeddings in groups of 64 for throughput
- DocumentIndex.writer uses RwLock instead of Mutex for concurrent writes
- Release workflow: removed slim variants (8 builds to 4 builds)
- Release workflow: use macos-latest for both macOS targets
- Manifest format simplified (removed variant field)

### Removed

- SimpleIndexer (6000+ lines, replaced by IndexFacade)
- SymbolCache and related cache methods
- Slim build variants from release pipeline
- Unused retrieve commands: Uses, Defines, Dependencies

### Fixed

- Multi-directory symbol ID collisions by querying IDs before spawning threads
- Pipeline metrics logging race with StatusLine
- Child paths incorrectly processed when parent already indexed

## [0.8.9] - 2025-12-30

### Changed

- Extracted 14 CLI command handlers from main.rs to src/cli/commands/
- Moved Clap argument definitions to src/cli/args.rs
- Replaced unreachable!() arms with single exhaustive match pattern
- Made indexer optional for commands that don't require index access

### Removed

- Unused rkyv dependency
- Unused bincode dependency

### Fixed

- Parse command tests failing without index directory
- Added index.db and index.scip to .gitignore

## [0.8.8] - 2025-12-30

### Added

- `language_id()` method to LanguageBehavior trait for cross-language resolution filtering
- `RelationRole` enum for relationship disambiguation (From/To)
- `resolve_instance_method()` and `resolve_method_call()` methods to LanguageBehavior
- `MethodCallResolver` struct for per-file variable types and method call storage
- `CapturedIncomingRelationship` for relationship preservation during file reindex
- `get_implemented_traits()` query for symmetric trait display
- Bidirectional relationship display in MCP (implements/implemented_by, extends/extended_by)
- `symbol_id:XXX` format support in find_symbol for direct lookup
- Calculator example files for method call resolution testing

### Changed

- Replaced `variable_types` + `method_calls_by_file` with unified `method_call_resolvers`
- Simplified `resolve_symbol_for_relationship()` - kind filtering moved to behavior layer
- Language filtering added to `build_resolution_context_with_cache` candidate verification
- `find_symbol` MCP tool includes all ContextIncludes flags
- Upgraded serde_json from 1.0.145 to 1.0.148
- Upgraded tempfile from 3.23.0 to 3.24.0
- Upgraded testcontainers from 0.26.2 to 0.26.3

### Removed

- Legacy conversion methods from MethodCall (`to_simple_call`, `from_legacy_format`)

## [0.8.7] - 2025-12-20

### Added

- Custom MCP requests: `requests/codanna/index-stats`, `requests/codanna/force-reindex`
- Custom MCP notifications: `file-reindexed`, `file-deleted`, `index-reloaded`
- HotReloadWatcher for external index changes (CI/CD, other processes)
- Debug logging for unmatched file watcher events

### Changed

- Renamed IndexWatcher to HotReloadWatcher with clearer purpose
- Moved hot reload watcher from `mcp/watcher.rs` to `watcher/hot_reload.rs`
- Removed source file polling from HotReloadWatcher (UnifiedWatcher handles this)
- Notification listener now spawned in HTTPS server for client notifications
- Simplified async fn signature for `on_custom_request`

### Removed

- `check_and_reindex_source_files()` from hot reload watcher (redundant with UnifiedWatcher)
- `with_mcp_server()` method (notifications now use broadcaster pattern)

## [0.8.6] - 2025-12-20

### Added

- Unified watcher module replacing three separate implementations
- Path registry for tracking monitored files across multiple watchers
- Debouncer module to prevent duplicate event processing
- Handler trait with code, config, and document implementations
- Logging module with tracing-subscriber and per-module level support
- RUST_LOG environment variable support for runtime log level control
- Documents command documentation in CLI reference
- Logging configuration section in configuration guide

### Changed

- Replaced debug_print macros with tracing calls across indexer and parsers
- MCP servers now use unified watcher and structured logging
- Language parsers updated to use tracing
- README restructured with Quick Start moved to top
- Features presented as table with documentation links
- Advanced features (Profiles, Documents) moved to collapsible sections

### Removed

- documents/watcher.rs (replaced by unified watcher)
- indexing/fs_watcher.rs (replaced by unified watcher)
- indexing/config_watcher.rs (replaced by unified watcher)

## [0.8.5] - 2025-12-18

### Added

- Document embedding system for RAG (Retrieval-Augmented Generation)
- Document chunking with hybrid strategy (paragraph-based sizing with merge/split)
- DocumentStore with tantivy metadata index and mmap vector storage
- Semantic search for documents filtered by collection and path
- File watcher for automatic document re-indexing on changes
- CLI commands: `documents add-collection`, `remove-collection`, `index`, `search`, `list`, `stats`
- MCP tool `search_documents` for AI assistant integration
- Settings section `[documents]` with chunking and search configuration
- Documentation in `docs/user-guide/documents.md`
- KWIC (Keyword In Context) preview mode with highlighting

### Changed

- Upgraded rmcp from 0.11.0 to 0.12.0
- Upgraded fastembed from 5.4.0 to 5.5.0
- Upgraded tree-sitter from 0.26.2 to 0.26.3
- Upgraded console from 0.16.1 to 0.16.2
- Upgraded rcgen from 0.14.5 to 0.14.6
- Settings save now preserves inline comments via `add_config_comments`

### Fixed

- `add-dir` command now handles already-indexed paths gracefully (shows message instead of error)
- `add-dir`/`remove-dir`/`list-dirs` skip index loading and semantic search initialization
- `-c` short flag conflict resolved (removed from `--collection` in documents commands)
- `documents index` now syncs stale collections even when no collections configured

## [0.8.4] - 2025-12-12

### Changed

- Migrated from rmcp 0.9.1 to 0.11.0
- Replaced SSE transport with streamable HTTP transport
- Updated MCP HTTP/HTTPS server to use StreamableHttpService
- Updated tree-sitter from 0.25.10 to 0.26.2
- Fixed tree-sitter Node API calls to use u32 instead of usize
- Updated fastembed from 5.3.1 to 5.4.0
- Updated axum-server from 0.7.3 to 0.8.0
- Updated tower-http from 0.6.7 to 0.6.8
- Updated git2 from 0.20.2 to 0.20.3
- Updated criterion from 0.8.0 to 0.8.1
- Updated testcontainers from 0.25.2 to 0.26.0
- MCP endpoint changed from `/mcp/sse` to `/mcp`
- MCP client config type changed from `"sse"` to `"http"`
- Updated HTTP server documentation with new transport configuration

### Removed

- Removed `transport-sse-server` feature from rmcp
- Removed SSE-specific transport code
- Removed early address parsing in HTTPS server

### Fixed

- Fixed clippy unnecessary_unwrap warnings in Swift test files

## [0.8.3] - 2025-11-30

### Added

- Swift language support with tree-sitter-swift parser
- SwiftParser with class, struct, enum, protocol, function, property extraction
- SwiftBehavior with Swift module path formatting
- SwiftResolutionContext with Swift scope order
- SwiftProvider for project structure handling
- Swift ERROR recovery for `@unchecked Sendable` syntax
- Nested type extraction in ERROR-recovered Swift classes
- TypeScript ERROR recovery for `export type *` pattern
- Kotlin context receiver function detection from `infix_expression` pattern
- Test suite for Swift error recovery, relationships, visibility, nested types
- Test suite for TypeScript error recovery
- Test suite for Kotlin context receivers, value classes
- Test suite for PHP readonly classes
- Swift grammar documentation and audit reports
- Example Swift files for testing
- tree-sitter-swift dependency

### Fixed

- Swift nested types now extracted when parent class has ERROR nodes
- TypeScript `export type * as Name` now resolves as Module symbol
- Kotlin `context(...)` receiver functions now extracted as Function symbols
- Swift recursive helper functions converted to static methods for clippy compliance

### Changed

- Swift parser handles both regular and ERROR-recovered class declarations
- Kotlin audit.rs includes `infix_expression` in key_nodes tracking
- Updated grammar-versions.lock with Swift grammar info

## [0.8.2] - 2025-11-28

### Added

- JavaScript language support
- JavaScriptProvider for jsconfig.json parsing and path alias resolution
- JavaScriptParser with class, function, method, arrow function, export extraction, etc
- JavaScriptBehavior with ES module path formatting and .js extension handling
- JavaScriptResolutionContext with JavaScript scope order (local → module → imported → global)
- Example JavaScript projects: React app, Next.js app, vanilla JavaScript
- JavaScript inheritance resolution with extends/extended_by relationships
- Test suite for JavaScript nested functions
- Project resolution documentation in docs/advanced/project-resolution.md

### Fixed

- JavaScript module path normalization now strips .js/.jsx/.mjs/.cjs extensions
- JavaScript class inheritance resolution (extends relationships now resolve correctly)

### Changed

- Refactored JavaScript behavior to use shared normalize_js_import helper
- Updated all language documentation to include JavaScript
- JavaScript audit reports regenerated with current timestamps

### Documentation

- Added docs/advanced/project-resolution.md explaining tsconfig.json/jsconfig.json resolution
- Added JavaScript to supported languages list across all documentation

## [0.8.1] - 2025-11-25

### Fixed

- Added AllMpnetBaseV2 support for fastembed 5.3.0 compatibility

### Changed

- Updated fastembed from 5.2.1 to 5.3.0
- Updated tower-http from 0.6.6 to 0.6.7

## [0.8.0] - 2025-11-19

### Added

- Resolution pipeline: Multi-symbol disambiguation with relationship-aware resolution
- Java language support with complete six-file architecture
- JavaProvider for Maven pom.xml parsing and dependency resolution
- Storage: find_symbols_by_module() for package-scoped resolution
- Symbol relationships: extends, extended_by, uses, used_by
- Symbol context: ClassMember now tracks class_name
- JavaParser with class, interface, method, field, enum extraction
- JavaBehavior with package-based module path formatting
- JavaResolutionContext with Java scope order (local → class → imported → package)
- JavaInheritanceResolver for interface and class hierarchy tracking
- Reranking comparison integration test
- Java audit test in abi15_grammar_audit.rs
- Java-specific inheritance test in tantivy.rs

### Changed

- Symbol map: HashMap<String, SymbolId> → HashMap<String, Vec<(SymbolId, SymbolKind)>>
- ClassMember: unit variant → struct with class_name field
- Relationship flags: expanded from 4 to 6 bits (added EXTENDS | USES)
- Version: 0.7.0 → 0.8.0
- Updated 15 dependencies (anyhow, bitflags, clap, crossbeam-channel, etc)
- Commented out release profile optimizations for faster dev builds

### Fixed

- MCP protocol: Added peer info registration in initialize handler
- mcp-test command: Moved to early execution before index loading
- stdout corruption: Changed diagnostic output to stderr
- Security: Replaced unmaintained json5 with serde_json5 (RUSTSEC-2025-0120)

### Enhanced

- Kotlin parser: More granular relationship tracking
- C# resolution: Better namespace handling
- Project resolver: Improved glob matching for multi-extension support
- All parser audit reports regenerated with current timestamps

### Documentation

- Updated README.md with Java in multi-language feature list
- Updated language support tables across docs/ and contributing/ (7 files updated)
- Added Java to implementation status table with Maven integration details
- Synchronized language lists across architecture, getting-started, and reference docs

## [0.7.0] - 2025-11-08

### Added

- Kotlin generic type flow with parameter inference and substitution
- Kotlin extension function resolution with receiver type tracking
- Cow-based type substitution for zero-copy simple types and owned complex types
- Trait method find_variable_types_with_substitution for opt-in owned type inference
- Regex word-boundary type parameter replacement to prevent partial matches
- Expression type registration in behavior layer for cross-component access
- Kotlin resolution context with expression type injection
- Test suite for generic inference (simple, nested, multi-param, complex substitution)
- Test suite for extension function resolution with receiver tracking
- Example demonstrating generic + extension resolution
- Debug test for tracking function and method call collection

### Fixed

- Function call deduplication now includes line and column to track each call site separately

### Changed

- Indexer prefers complex type substitution when available, falls back to zero-copy
- Function call deduplication key changed from (caller, callee, kind) to (caller, callee, kind, line, column)

## [0.6.9] - 2025-11-05

### Added

- TypeScript JSX component usage tracking with find_uses extraction
- Symbol name to ID mapping during extraction phase for TypeScript
- Test suite for JSX component relationship tracking (test_jsx_uses.rs)
- Indexed paths cache in Settings for O(1) ancestor checks
- Path deduplication in Settings to skip children when parent exists and replace children when adding parent
- Path deduplication in SimpleIndexer to remove descendant paths when adding ancestor

### Fixed

- Sync operation now respects `--progress` flag when indexing new directories
- Sync no longer shows "Index already up to date" after successfully indexing new directories
- Sync failures now route through error handling system with proper exit codes and recovery suggestions
- Metadata load failures during sync now show recovery steps instead of silent fallback
- Debug output (DEBUG: prefix) now only appears when debug mode is enabled

### Changed

- Sync state tracking changed from boolean to `Option<bool>` to distinguish metadata unavailable (None), no changes (Some(false)), and changes applied (Some(true))
- Single-file paths in `codanna index` are now skipped with clear message instead of attempting to add to settings
- File removal during sync now shows progress bar for multiple files
- Settings::add_indexed_path now deduplicates paths and manages parent-child relationships
- add_relationships_by_name now accepts optional from_id parameter to skip lookups when symbol ID is known
- resolve_relationships skips lookups when from_id exists in TypeScript parser
- Index command now reuses cached results and prints skip messages for already-indexed paths

### Documentation

- Added profiles feature documentation with examples
- Updated README with profiles configuration details
- Updated cli-reference.md with idempotent indexing behavior and skip messages

## [0.6.8] - 2025-11-03

### Added

- Kotlin language support with symbol extraction for classes, objects, functions, properties, and interfaces
- Context save/restore pattern in Kotlin parser for handle_class_declaration, handle_object_declaration, and handle_function_declaration
- Test suite for Kotlin nested scope handling (test_nested_scopes.rs with 3 tests)
- Pinned tree-sitter-kotlin dependency to commit 57fb4560 for reproducible builds
- Kotlin to supported languages list in README.md
- Kotlin implementation status in language-support.md

### Fixed

- Nested scope context loss in Kotlin parser where methods after nested classes would lose parent class context
- Clippy len_zero warning in test_interfaces_and_enums.rs
- Clippy uninlined_format_args warning in test_kotlin_semantic_search.rs

## [0.6.7] - 2025-11-01

### Fixed

- MCP commands now support `symbol_id:N` syntax with JSON output
- Added symbol_id parameter handling to find_callers JSON data collection
- Added symbol_id parameter handling to get_calls JSON data collection
- Added symbol_id parameter handling to analyze_impact JSON data collection
- Updated error messages to show correct parameter (symbol_id vs name) when symbol not found
- Fixed empty impact result to handle symbol_id in identifier string

Closes #63

## [0.6.6] - 2025-10-31

### Added

**Documentation**
- Created language-architecture.md with design principles and resolution patterns
- Created language-patterns.md with implementation patterns from TypeScript/Rust
- Created development README.md as documentation index
- Updated language-support.md with accurate resolution implementation status

**Grammar Version Tracking**
- Added grammar-versions.lock to track tree-sitter grammar commits and ABI versions
- Added update-grammar-lock.sh to generate/update lockfile automatically
- Added check-grammar-updates.sh to detect remote grammar updates
- Lockfile tracks commit hash, timestamp, ABI version, and repo URL for each grammar

**GDScript Enhancement**
- Added relationship tracking for GDScript (#62)

**MCP Enhancement**
- Added guidance messages to all MCP tool responses (find_symbol, get_calls, find_callers, analyze_impact, search_symbols, semantic_search_docs, semantic_search_with_context)

### Changed

- Renamed grammar-node-types.json to node-types.json across all languages
- Updated setup.sh to copy node-types.json and update lockfile
- Updated abi15_grammar_audit.rs to use new node-types.json filename
- Moved `generate_mcp_guidance()` function to top of module for clarity

### Fixed

- Resolved clippy warning in method_call test data (changed vec![] to array)
- Prevented CI model downloads by marking gdscript semantic search test as ignored

### Removed

- Removed unused `from_persistence()` method from MCP server
- Removed `IndexPersistence` import from MCP module
- Removed outdated parsers_api.md (superseded by new docs)

## [0.6.5] - 2025-10-29

### Added

**GDScript Language Support**
- Added GDScript parser with tree-sitter integration for Godot game engine projects
- Implemented StatefulBehavior architecture for cross-file symbol tracking
- Added relative path resolution for GDScript imports (`./file.gd`, `../dir/file.gd`)
- Implemented `res://` protocol handling for GDScript module paths
- Added class hierarchy, export variables, and signal extraction
- Created example files and audit reports for GDScript grammar
- Added test suite with 850+ lines covering parser, behavior, imports, and resolution

**Retrieve Command Enhancement**
- Added `symbol_id` parameter support to `retrieve symbol` command
- Enabled direct symbol lookup by ID without name ambiguity

### Fixed

**Index Command Workflow**
- Fixed force flag to trigger complete re-index regardless of path source
- Added automatic path persistence when CLI paths provided to `index` command
- Prevented redundant auto-sync when force flag is present
- Added indexed_paths cleanup in `remove_paths()` to maintain tracking state
- Extracted `add_paths_to_settings()` helper for shared logic between `index` and `add-dir`

**Test Infrastructure**
- Fixed Windows file URL handling in marketplace resolution tests
- Resolved path normalization issues for cross-platform compatibility

**Configuration**
- Updated settings file references to remove non-existent commands
- Enabled GDScript in default language configuration

### Technical Details

- Total additions: 6,870 lines across 58 files
- GDScript implementation: 2,000+ lines of parser and behavior code
- Test coverage: 850+ lines of language-specific tests
- Co-authored-by: nguyenchiencong (GDScript foundation)

## [0.6.4] - 2025-10-29

### Profile System

Share workspace configurations (gitignore, hooks, documentation) across projects with version control.

**Commands:**
- `codanna profile sync` - Register providers and install team profiles
- `codanna profile install <profile>` - Install individual profile from provider
- `codanna profile remove <profile>` - Uninstall with directory cleanup
- `codanna profile status/list/verify` - Inspection and validation
- `codanna profile provider add/list/remove` - Provider registry management

**Features:**
- Three-tier configuration: global registry, team config, local lockfile
- Atomic transactional installation with pre-flight validation and rollback
- Provider registry supports GitHub, Git URL, and local directory sources
- File ownership tracking with conflict resolution via sidecars
- SHA-256 integrity verification
- Team sync from .codanna/profiles.json with extraKnownProviders

### Multi-Directory Indexing

Index multiple directories with automatic sync mechanism.

**Commands:**
- `codanna add-dir <path>` - Add directory to indexed paths
- `codanna remove-dir <path>` - Remove directory from indexed paths
- `codanna list-dirs` - Display configured indexed directories
- `codanna index [paths...]` - Accept multiple paths, use config when none provided

**Features:**
- Automatic sync on every command compares settings.toml with index metadata
- settings.toml is source of truth, index metadata is derived state
- New directories in config automatically indexed
- Removed directories automatically cleaned (symbols, embeddings, metadata)
- ConfigFileWatcher monitors settings.toml for changes in HTTP/HTTPS modes
- FileWatcher tracks both config file and source file changes

**Fixed:**
- Batch management in remove_file now self-contained (calls start_batch before operations)

### Documentation

- CLI reference
- Configuration documentation simplified

## [0.6.3] - 2025-10-24

### Changed
- Simplified CLAUDE.md with focused code intelligence workflow
- Removed multi-hop agent instructions in favor of direct workflow
- Streamlined query optimization and exploration patterns

### Fixed
- Symbol location display now includes line ranges in semantic search results
- Symbol context formatting shows symbol_id in location output

## [0.6.2] - 2025-10-23

### Added
- Binary release workflow with dual variants
  - Tag-triggered automated releases
  - 8 pre-built binaries (4 platforms × 2 variants)
  - Full variant includes MCP server support (--all-features)
  - Slim variant is CLI only
  - SHA256/SHA512 checksums for verification
  - Dist manifest with download URLs for universal installer
  - Preparation for https://setup.codanna.sh installer
- C# benchmark command for performance testing
- C# documentation examples (file-scoped namespaces, comprehensive.cs)

### Fixed
- C# import extraction fallback for using directives
- Stats display showing accurate symbol counts and timing
- Windows test compatibility (platform-agnostic path assertions)

## [0.6.1] - 2025-10-21

### Added
- Symbol ID parameter support for unambiguous queries
  - `symbol_id` parameter for retrieve commands (calls, callers, describe)
  - `symbol_id` parameter for MCP tools (get_calls, find_callers, analyze_impact)
  - CLI help text with symbol_id examples
  - Token-efficient workflow: search returns `[symbol_id:123]`, use `symbol_id:123` for precise follow-up
  - Eliminates disambiguation prompts, reduces token usage
- Import binding system for external dependency detection
  - Tracks import statements and their bindings
  - Foundation for external dependency resolution
- Documentation updates
  - Symbol_id workflows in User Guide, CLI Reference, and Search Guide
  - Advanced section with unambiguous query patterns
  - Plugin documentation with Node.js wrapper examples
  - Slash command updates with `<relationship_symbol_name|symbol_id:ID>` pattern

### Changed
- Plugin scripts updated to display and accept symbol_id
  - Formatters show `[symbol_id:123]` in headers and relationships
  - Context provider accepts symbol_id for all relationship queries
  - Applied to Claude Code plugin, codanna-cc, and codanna-base
- Dependency updates
  - clap 4.5.41 → 4.5.50
  - memmap2 0.9.7 → 0.9.9
  - indicatif 0.18.0 → 0.18.1
  - rmcp 0.7.0 → 0.8.2

## [0.6.0] - 2025-10-18

### Added
- **Plugin Management System**: Install, remove, and manage plugins
  - Transactional installs with automatic rollback on failure
  - Smart update detection skips I/O when no changes needed
  - Marketplace resolution for external plugin sources
- **Documentation Hub**: Centralized navigation at `docs/README.md`
  - Organized sections: Getting Started, User Guide, Integrations, Architecture, Advanced, Contributing, Plugins, Reference
  - Navigation footers across all documentation pages

### Changed
- Symbol display now includes file paths with line numbers for precise navigation
- Enhanced relationship formatting for better readability
- Improved plugins documentation with marketplace and MCP setup details

### Fixed
- TypeScript call tracking from object property functions
- Documentation cross-references updated for new structure

## [0.5.26] - 2025-10-09

### Added
- Configurable Tantivy heap size via `tantivy_heap_mb` setting (default 50MB)
- Configurable retry attempts via `max_retry_attempts` setting (default 3)
- Universal defaults work across all platforms without cfg checks

### Changed
- Tantivy heap size now user-configurable instead of hardcoded
- Retry logic moved to helper function with configurable attempts
- DocumentIndex constructor accepts Settings parameter

### Fixed
- Error detection uses ErrorKind instead of locale-dependent strings
- Transient permission errors handled with exponential backoff (100ms, 200ms, 400ms)
- Tests updated to use Settings parameter

## [0.5.25] - 2025-10-08

### Fixed
- C++ parser: Member function call detection for method invocations
  - Extract method names from field_expression nodes (obj->method, obj.method)
  - Extract method names from qualified_identifier in function context (Class::method)
  - Function context tracking now handles qualified method implementations
  - Register call_expression, field_expression, qualified_identifier in audit system
- MCP analyze_impact: Handle all symbols with same name instead of first match only
  - Changed from find_symbol (single) to find_symbols_by_name (all matches)
  - Aggregate impact across all symbols with same name
  - Show locations and direct caller counts for each symbol variant

## [0.5.24] - 2025-10-07

### Fixed
- C++ parser: Extract class methods from declarations and implementations
  - Method declarations inside classes now extracted as SymbolKind::Method
  - Out-of-class implementations (Class::method) identified as methods
  - Qualified_identifier pattern (Class::method) detection in function_definition
  - Class_specifier enters class scope and processes children recursively
  - Field_declaration extracts methods from function_declarator nodes
  - Tested with Qt QWindow: 144 methods extracted (was 0 before)

## [0.5.23] - 2025-10-07

### Changed
- Bump rmcp from 0.7.0 to 0.8.0

## [0.5.22] - 2025-10-07

### Added
- C++ parser: Doxygen doc comment extraction (/** */ and ///)
- C++ parser: Recursive call tracking with function context
- C++ parser: Scope context tracking via ParserContext

### Fixed
- MCP get_index_info now displays all symbol kinds dynamically
- C++ Audit system uses proper tree-sitter node names to generate the report

## [0.5.21] - 2025-10-03

### Added
- Recursion depth guards across all language parsers
  - `check_recursion_depth()` prevents stack overflow on deeply nested AST structures
  - All parsers (TypeScript, Python, Rust, Go, PHP, C++, C#) now track depth in `extract_symbols_from_node()`
  - Safely handles pathological code with excessive nesting (tested on Qt keyboard at depth 3521)

### Changed
- **PERFORMANCE**: Optimized resolution pipeline for large codebases
  - Indexed method calls as HashMap for O(1) lookup instead of linear search
  - Added symbol lookup cache to eliminate duplicate Tantivy queries
  - Qt qtbase (8,508 files, 413K symbols): 7m38s total, relationship resolution processes 4.68M relationships with 4,778 resolved, 4.39M skipped
  - Skipped relationships: external symbols not in index (Qt framework dependencies, system libraries)
- Parser method signatures updated to accept depth parameter
- Audit reports and grammar analysis regenerated for all languages

## [0.5.20] - 2025-10-02

### Added
- C# language support with full parser implementation (PR#39)
  - Symbol extraction for classes, interfaces, structs, enums, methods, properties, fields
  - Relationship tracking for inheritance, interface implementation, and method calls
  - XML documentation comment extraction
  - File extensions: `.cs`, `.csx`, `.cshtml`
- Fuzzy search on non-tokenized name field for whole-word typo tolerance
  - Handles missing character typos in full symbol names (e.g., "ArchivService" finds "ArchiveService")
  - Dual fuzzy strategy: ngram tokens for partial matches + whole words for full name typos

### Changed
- **BREAKING**: Tantivy schema `name` field changed from TEXT to STRING
  - Enables exact matching without tokenization for fuzzy search
  - Requires full reindex: `codanna index --force`
- **PERFORMANCE**: Batch commits every 100 files instead of per-file commits
  - 10-50x faster indexing (varies by platform and file count)
  - macOS: ~10x improvement on typical projects
  - Windows: 25-50x improvement (1-2 files/s → 46 files/s on 4,453 file project)
  - Reduces disk I/O, segment creation, and cache rebuilds
- Automatic reverse relationship creation for bidirectional graph navigation
  - Implements ↔ ImplementedBy, Extends ↔ ExtendedBy, Calls ↔ CalledBy, Uses ↔ UsedBy

### Fixed
- File ID counter race condition during batch operations
  - Pending counter prevents stale committed values from causing duplicate IDs
- Windows file locking issues with proper retry logic and error logging
  - Symbol cache and persistence layer handle OS error 1224 and permission denied

## [0.5.19] - 2025-10-01

### Added
- Full symbol boundary tracking for precise editor navigation
  - `create_symbol()` accepts `full_node` parameter for complete range extraction
  - Tantivy schema extended with `end_line` and `end_column` fields
  - MCP tools now return precise symbol ranges (start_line, start_column, end_line, end_column)

### Changed
- C parser: Functions, structs, unions, enums, fields, and macros now use full boundaries
- Rust parser: Functions, structs, enums, traits, and modules now use full boundaries
- README: Added documentation for precise symbol boundary support

## [0.5.18] - 2025-09-30

### Added
- JSX component usage tracking in TypeScript parser
  - New `component_usages` field tracks function → component relationships
  - `extract_jsx_uses_recursive()` traverses AST to find JSX elements
  - `track_jsx_component_usage()` filters components by uppercase naming convention
  - Supports `jsx_element` and `jsx_self_closing_element` nodes
  - Generator functions (`generator_function_declaration`) included in function context
- Test fixtures for JSX usage patterns
  - Profile.tsx: React component with JSX
  - test_documented_jsx.tsx: JSX with documentation
  - test_jsx_same_file.tsx: JSX defined and used in same file
  - test_jsx_usage.tsx: Multiple components using shared JSX

### Changed
- Audit reports regenerated to reflect JSX and generator function support
- All language parser audit reports updated with latest node counts

## [0.5.17] - 2025-09-29

### Changed
- Refactored relationship compatibility logic from indexer to language behaviors
  - Moved `is_compatible_relationship` from SimpleIndexer to ResolutionScope trait
  - Each language now controls its own relationship validation rules
  - Cleaner separation between orchestration and language-specific logic

### Fixed
- UTF-8 character boundary parsing error when encountering Unicode characters
  - Added `safe_substring_window()` utility for UTF-8-safe string slicing
  - TypeScript parser now handles box-drawing characters and emojis correctly
  - Prevents panic when checking for export modifiers before symbols
  - Fixes Issue #38

## [0.5.16] - 2025-09-28

### Added
- TypeScript path alias resolution with full cross-module support
  - Aliases like `@/*` resolved to actual paths (`./src/*`)
  - Symbols added by module_path for cross-module resolution
  - Import paths enhanced at storage time for correct resolution
- Default export visibility tracking for TypeScript
  - `export default` symbols now marked as Public
  - Enables proper cross-module access to default exports
- React component relationship support
  - Constants and Variables now callable (React functional components)
  - Proper relationship tracking for component hierarchies

### Changed
- **BREAKING**: External stub symbols no longer created for unresolved imports
  - Cleaner index without placeholder symbols
  - Requires full project reindex: `codanna index --force`
- TypeScript behavior enhanced with module_path resolution
- Relationship validation extended for JavaScript/TypeScript patterns

### Fixed
- TypeScript imports using path aliases not resolving across modules
- Default exported symbols incorrectly marked as Private
- React components (Constants) not creating proper call relationships
- Cross-module visibility checks for exported symbols

### Migration Required
To benefit from improved TypeScript resolution:
```bash
codanna index --force
```

## [0.5.15] - 2025-09-27

### Added
- Cross-module resolution: Full qualified path resolution for all languages
  - Symbols now resolvable by both simple name and full module path
  - Example: `crate::init::init_global_dirs`, `app.utils.helper.process_data`
- Python parser: Methods now use qualified names (e.g., `Calculator.__init__`)
- Resolution tests for Rust and Python cross-module calls
- Architectural documentation: Universal vs language-specific concepts

### Changed
- **BREAKING**: Python method naming convention - requires reindexing Python codebases
- Resolution context: Module paths added during symbol population

### Fixed
- Cross-module function calls not being resolved (e.g., `crate::module::function`)
- Python parser tests updated for new qualified naming convention

## [0.5.14] - 2025-09-25

### Added
- Global model cache system at `~/.codanna/models` for shared FastEmbed models across projects
- Project registry tracking all indexed projects with unique IDs
- `codanna init` command to initialize project structure and create model symlinks
- Test isolation with separate directories (`~/.codanna-test`) for development

### Changed
- **BREAKING**: Existing `.fastembed_cache` directories must be deleted before running `init --force`
- Model storage moved from per-project directories to global cache via symlinks
- Settings validation now checks for proper initialization on startup


### Migration Required
To upgrade existing projects:
```bash
rm -rf .fastembed_cache
codanna init --force
```

## [0.5.13] - 2025-09-13

### Fixed
- Python parser: Module-level function calls and class instantiations now tracked (fixes #32)
  - Module symbol created for each Python file to represent module scope
  - Module-level calls tracked with `<module>` as caller, mapped to actual module path for queries
  - `normalize_caller_name()` maps synthetic names to searchable module paths
  - `configure_symbol()` renames module symbols for searchability
  - Module type accepted as valid caller in relationship validation
  - External symbol resolution handles unresolved import targets
  - Method call resolution normalizes caller names for consistent matching

### Added
- Python parser: Module-level execution tracking for better code analysis
- Tests: Module-level class instantiation detection verification

## [0.5.12] - 2025-09-12

### Fixed
- MCP server: Fixed tool discovery issue after rmcp 0.6.4 upgrade (fixes #31)
  - Tools without parameters now generate proper `{"type": "object"}` schema
- Parser safety: Fixed UTF-8 string truncation panic when encountering emojis or multi-byte characters (fixes #29)
  - Added `safe_truncate_str` and `truncate_for_display` utilities that respect UTF-8 boundaries
  - Applied fix to Python and PHP parsers where manual truncation was used
  - Zero-cost implementation returning string slices without allocation

### Improved
- MCP server instructions: Updated workflow guidance to emphasize semantic search first approach for better code exploration

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
