# Research Report: Project Resolver Infrastructure Workflow

**Date**: 2026-01-02 12:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The project resolver infrastructure in `src/project_resolver/` provides a cross-language system for resolving project-level configurations (tsconfig.json, jsconfig.json, pom.xml, Package.swift) into resolution rules. These rules enable path alias resolution and module path computation during indexing. The system uses SHA-based invalidation for efficient caching and a persistence layer that stores rules as JSON in `.codanna/index/resolvers/`.

## Key Findings

### 1. Provider Trait Architecture

Each language implements `ProjectResolutionProvider` trait with these methods:

```rust
pub trait ProjectResolutionProvider: Send + Sync {
    fn language_id(&self) -> &'static str;
    fn is_enabled(&self, settings: &Settings) -> bool;
    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf>;
    fn compute_shas(&self, configs: &[PathBuf]) -> ResolutionResult<HashMap<PathBuf, Sha256Hash>>;
    fn rebuild_cache(&self, settings: &Settings) -> ResolutionResult<()>;
    fn select_affected_files(&self, settings: &Settings) -> Vec<PathBuf>;
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/provider.rs:12-32`

### 2. ResolutionRules Structure

The `ResolutionRules` struct captures path resolution configuration:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionRules {
    /// Base URL for path resolution (e.g., "." or "./src")
    pub base_url: Option<String>,

    /// Path alias mappings (e.g., "@app/*" -> ["src/app/*"])
    pub paths: HashMap<String, Vec<String>>,
}
```

- `base_url`: The root directory for non-relative module resolution
- `paths`: Maps import aliases to actual file paths (supports wildcards)

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/persist.rs:32-42`

### 3. Provider Implementations

Four providers exist:

| Provider | Config File | Language ID |
|----------|-------------|-------------|
| TypeScriptProvider | tsconfig.json | typescript |
| JavaScriptProvider | jsconfig.json | javascript |
| JavaProvider | pom.xml | java |
| SwiftProvider | Package.swift | swift |

**TypeScript** and **JavaScript** providers extract `baseUrl` and `paths` from their respective config files.
**Java** provider extracts source roots from Maven/Gradle configs.
**Swift** provider extracts module sources from Package.swift.

**Evidence**:
- `/Users/bartolli/Projects/codanna/src/project_resolver/providers/typescript.rs`
- `/Users/bartolli/Projects/codanna/src/project_resolver/providers/java.rs`

### 4. rebuild_cache() Workflow

The `rebuild_cache()` method follows this flow:

1. Load config paths from settings
2. Create `ResolutionPersistence` manager
3. Load or create `ResolutionIndex`
4. For each config file:
   - Compute SHA-256 hash
   - Check if rebuild needed via `index.needs_rebuild()`
   - Parse config file (e.g., resolve tsconfig extends chain)
   - Update SHA in index
   - Set resolution rules
   - Add file pattern mappings (e.g., `src/**/*.ts`)
5. Save updated index to disk

**TypeScript Example:**
```rust
// Parse tsconfig and resolve extends chain
let mut visited = std::collections::HashSet::new();
let tsconfig = resolve_extends_chain(config_path, &mut visited)?;

// Update index
index.update_sha(config_path, &sha);
index.set_rules(config_path, ResolutionRules {
    base_url: tsconfig.compilerOptions.baseUrl,
    paths: tsconfig.compilerOptions.paths,
});

// Add file mappings
let pattern = format!("{}/**/*.ts", parent.display());
index.add_mapping(&pattern, config_path);
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/providers/typescript.rs:117-159`

### 5. Persistence Layer

`ResolutionPersistence` manages JSON files at `.codanna/index/resolvers/{language}_resolution.json`:

```rust
pub struct ResolutionIndex {
    pub version: String,                              // Schema version "1.0"
    pub hashes: HashMap<PathBuf, String>,             // Config file SHA-256s
    pub mappings: HashMap<String, PathBuf>,           // Pattern -> config file
    pub rules: HashMap<PathBuf, ResolutionRules>,     // Config -> rules
}
```

Key methods:
- `load(language_id)` - Load index from disk
- `save(language_id, &index)` - Save index to disk
- `get_config_for_file(file_path)` - Longest-prefix match to find applicable config

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/persist.rs:17-30`

### 6. Rule Loading During Resolution

The `load_project_rules_for_file()` method in language behaviors loads rules at resolution time:

```rust
fn load_project_rules_for_file(&self, file_id: FileId) -> Option<ResolutionRules> {
    thread_local! {
        static RULES_CACHE: RefCell<Option<(Instant, ResolutionIndex)>> = const { RefCell::new(None) };
    }

    RULES_CACHE.with(|cache| {
        // Check if cache is fresh (< 1 second old)
        // Load from disk if needed
        // Get rules for file via get_config_for_file()
    })
}
```

Uses a thread-local cache with 1-second TTL to avoid repeated disk reads.

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/typescript/behavior.rs:35-79`

### 7. Integration in Resolution Context

During `build_resolution_context_with_cache()`, rules are used to enhance import paths:

```rust
if let Some(rules) = self.load_project_rules_for_file(file_id) {
    let enhancer = TypeScriptProjectEnhancer::new(rules);

    if let Some(enhanced_path) = enhancer.enhance_import_path(&import.path, file_id) {
        // Path alias resolved - use enhanced path
        enhanced_path.trim_start_matches("./").replace('/', ".")
    } else {
        // Not an alias - use relative normalization
        normalize_ts_import(&import.path, &importing_module)
    }
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/typescript/behavior.rs:730-755`

## Architecture: Complete Data Flow

```
                       Settings.toml
                            |
                            v
         +------------------+------------------+
         |                                     |
    config_files                          enabled
         |                                     |
         v                                     v
  +--------------+                    +----------------+
  | Provider     | <-- is_enabled() --| SimpleProvider |
  | TypeScript   |                    | Registry       |
  | JavaScript   |                    +----------------+
  | Java         |                           |
  | Swift        |                           |
  +--------------+                           |
         |                                   v
         |                          initialize_providers()
         v                          (main.rs:47-118)
  rebuild_cache()                            |
         |                                   |
         v                                   v
  +----------------+              +-------------------+
  | Config Parsing |              | validate paths    |
  | - tsconfig.json|              | - rebuild_cache() |
  | - pom.xml      |              +-------------------+
  | - Package.swift|
  +----------------+
         |
         v
  +------------------+
  | ResolutionIndex  |
  | - hashes (SHA)   |
  | - mappings       |
  | - rules          |
  +------------------+
         |
         v
  .codanna/index/resolvers/
    typescript_resolution.json
    javascript_resolution.json
    java_resolution.json
    swift_resolution.json
         |
         v
  load_project_rules_for_file()
  (thread-local cached)
         |
         v
  TypeScriptProjectEnhancer
         |
         v
  enhance_import_path()
         |
         v
  Resolution Context
```

## File Mapping Strategy

The `get_config_for_file()` method uses longest-prefix matching:

1. Canonicalize file path (resolve symlinks)
2. For each mapping pattern, extract base directory
3. Check if file path starts with pattern base
4. Sort matches by pattern length (longest first)
5. Return the config from longest match

This ensures nested tsconfig files (e.g., `packages/web/tsconfig.json`) take precedence over root configs.

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/persist.rs:88-120`

## Conclusions

1. **Modular Design**: Each language provider is self-contained with its own config parsing logic
2. **SHA-Based Invalidation**: Only rebuilds when config files change
3. **Thread-Local Caching**: Avoids disk I/O during resolution with 1-second TTL
4. **Longest-Prefix Matching**: Correctly handles monorepo structures with nested configs
5. **Graceful Degradation**: Missing configs produce warnings, not errors

The system correctly separates concerns:
- `project_resolver`: "Which config applies to this file?"
- `parsing::resolution`: "What does this identifier resolve to?"

Key integration point is `initialize_providers()` in `main.rs`, which validates configs and triggers cache building before indexing begins.
