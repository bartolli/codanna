# Configuration Guide

Codanna configuration lives in `.codanna/settings.toml`.

## Configuration File Location

```bash
.codanna/
├── plugins/          # Plugin lockfile 
├── index/            # Index storage
├── .project-id       # Unique project id used in ~/.codanna to manage global configurations
└── settings.toml     # Main configuration
```

## Basic Configuration

```toml
# .codanna/settings.toml

# Semantic search model configuration
[semantic]
# Model to use for embeddings
# - AllMiniLML6V2: English-only, 384 dimensions (default)
# - MultilingualE5Small: 94 languages including, 384 dimensions (recommended for multilingual)
# - MultilingualE5Base: 94 languages, 768 dimensions (better quality)
# - MultilingualE5Large: 94 languages, 1024 dimensions (best quality)
# - BGESmallZHV15: Chinese-specialized, 512 dimensions
# - See documentation for full list of available models
model = "AllMiniLML6V2"
```

[Read more about embedding models](../architecture/embedding-model.md)

```toml
# Agent guidance configuration
[guidance]
enabled = true
```
[Learn more about agent guidance](../integrations/agent-guidance.md)

## Language Configuration

### TypeScript

Reads `tsconfig.json` to resolve path aliases and imports.

**Configuration:**
```toml
[languages.typescript]
enabled = true
config_files = [
    "tsconfig.json",
    "packages/web/tsconfig.json",  # For monorepos
    "packages/api/tsconfig.json"
]
```

**Process:**
1. Reads your `tsconfig.json` files
2. Extracts `baseUrl`, `paths`, and resolution rules
3. Stores rules in `.codanna/index/resolvers/`
4. Uses rules during indexing to resolve imports

**Example:** Given `tsconfig.json` with:
```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@app/*": ["src/app/*"],
      "@utils/*": ["src/utils/*"]
    }
  }
}
```

Codanna resolves:
- `@app/main` → `src/app/main`
- `@utils/config` → `src/utils/config`

### Java

Reads `pom.xml` to resolve Maven project structure and dependencies.

**Configuration:**
```toml
[languages.java]
enabled = true
config_files = [
    "pom.xml",
    "module-a/pom.xml",  # For multi-module projects
    "module-b/pom.xml"
]
```

**Process:**
1. Reads your `pom.xml` files
2. Extracts package structure and source directories
3. Stores rules in `.codanna/index/resolvers/`
4. Uses rules during indexing to resolve imports

**Example:** Given `pom.xml` with:
```xml
<project>
  <groupId>com.example</groupId>
  <artifactId>my-app</artifactId>
  <build>
    <sourceDirectory>src/main/java</sourceDirectory>
  </build>
</project>
```

Codanna resolves:
- `com.example.service.UserService` → `src/main/java/com/example/service/UserService.java`
- `com.example.util.Helper` → `src/main/java/com/example/util/Helper.java`

### Other Languages

Coming soon: Python (`pyproject.toml`), Go (`go.mod`), and other languages with project-specific import resolution.

## Semantic Search Models

### Available Models

| Model | Description | Use Case |
|-------|-------------|----------|
| `AllMiniLML6V2` | Fast, English-optimized (default) | English codebases |
| `MultilingualE5Small` | Better for non-English | Mixed language teams |
| `ParaphraseMultilingualMiniLML12V2` | Best multilingual | International projects |

### Switching Models

```toml
[semantic]
model = "MultilingualE5Small"
```

**Note:** Changing models requires re-indexing:
```bash
codanna index . --force 
```

## Agent Guidance Templates

Configure how Codanna guides AI assistants:

```toml
[guidance]
enabled = true

[guidance.templates.find_callers]
no_results = "No callers found. Might be an entry point or dynamic dispatch."
single_result = "Found 1 caller. Use 'find_symbol' to inspect usage."
multiple_results = "Found {result_count} callers. Try 'analyze_impact' for the full graph."

[guidance.templates.analyze_impact]
no_results = "No impact detected. Likely isolated."
single_result = "Minimal impact radius."
multiple_results = "Impact touches {result_count} symbols. Focus critical paths."

[[guidance.templates.analyze_impact.custom]]
min = 20
template = "Significant impact with {result_count} symbols. Break the change into smaller parts."
```

## Indexing Configuration

```toml
[indexing]
threads = 8  # Number of threads for parallel indexing
max_file_size_mb = 10  # Skip files larger than this
```

## Multi-Directory Indexing

Index multiple directories simultaneously with persistent configuration.

### Configuration

```toml
[indexing]
indexed_paths = [
    "/absolute/path/to/project1",
    "/absolute/path/to/project2",
    "/absolute/path/to/project3"
]
```

### Managing Indexed Directories

```bash
codanna add-dir /path/to/project
codanna list-dirs
codanna remove-dir /path/to/project
```

**Automatic Sync:**
- Commands update settings.toml (source of truth)
- Next command syncs index automatically
- New paths → indexed
- Removed paths → cleaned (symbols, embeddings, metadata)

### Use Cases

**Multi-project workspaces** - Index multiple related projects together for cross-project symbol resolution

**Monorepo support** - Index different components separately while maintaining cross-references

**Selective indexing** - Only index specific directories within large codebases

**Dynamic workflows** - Add and remove folders as your project structure changes

## Logging Configuration

Control debug output with per-module granularity.

### Configuration

```toml
[logging]
default = "warn"  # Default level: error, warn, info, debug, trace

[logging.modules]
cli = "debug"           # Enable CLI debug logs
watcher = "info"        # Watch file system events
indexing = "trace"      # Detailed indexing information
```

### Log Levels

| Level | Description |
|-------|-------------|
| `error` | Errors only (quietest) |
| `warn` | Errors + warnings (default) |
| `info` | Normal operation logs |
| `debug` | Detailed debugging |
| `trace` | Everything |

### Environment Variable

`RUST_LOG` takes precedence over configuration:

```bash
# Enable all debug output
RUST_LOG=debug codanna index

# Per-module control
RUST_LOG=cli=debug,indexer=trace codanna serve

# Quiet except errors
RUST_LOG=error codanna mcp semantic_search_with_context query:"test"
```

## Ignore Patterns

Codanna respects `.gitignore` and adds its own `.codannaignore`:

```bash
# .codannaignore
.codanna/       # Don't index own data
target/         # Skip build artifacts
node_modules/   # Skip dependencies
*_test.rs       # Optionally skip tests
```

## HTTP/HTTPS Server Configuration

For server mode configuration:

```toml
[server]
bind = "127.0.0.1:8080"
watch_interval = 5  # Seconds between index checks
```

## Performance Tuning

```toml
[performance]
cache_size_mb = 100  # Memory cache size
vector_cache_size = 10000  # Number of vectors to keep in memory
```

## Command-Line Overrides

Most settings can be overridden via command-line:

```bash
# Override config file
codanna --config /path/to/custom.toml index .

# Override thread count
codanna index . --threads 16

# Force specific settings
codanna serve --watch --watch-interval 10
```

## Viewing Configuration

```bash
# Display active settings
codanna config

# Show config with custom file
codanna --config custom.toml config
```

## Configuration Precedence

1. Command-line flags (highest priority)
2. Custom config file (via `--config`)
3. Project `.codanna/settings.toml`
4. Built-in defaults (lowest priority)

## Troubleshooting

### Index Not Updating

Check watch interval:
```toml
[server]
watch_interval = 5  # Lower for more frequent checks
```

### Semantic Search Not Working

1. Ensure documentation comments exist
2. Check model is appropriate for your language
3. Re-index after configuration changes

### Path Resolution Issues

**Check config files are listed:**
```bash
codanna config | grep config_files
```

**Verify paths in your project config:**
- TypeScript: Check `baseUrl` and `paths` in `tsconfig.json`
- Java: Check `sourceDirectory` in `pom.xml`

**Re-index after config changes:**
```bash
codanna index . --force 
```

### Monorepo Issues

Ensure all relevant config files are listed in settings.toml:
```toml
[languages.typescript]
config_files = [
    "tsconfig.json",
    "packages/web/tsconfig.json",
    "packages/api/tsconfig.json"
]

[languages.java]
config_files = [
    "pom.xml",
    "module-a/pom.xml",
    "module-b/pom.xml"
]
```

## See Also

- [First Index](../getting-started/first-index.md) - Creating your first index
- [Agent Guidance](../integrations/agent-guidance.md) - Configuring AI assistant behavior
- [CLI Reference](cli-reference.md) - Command-line options