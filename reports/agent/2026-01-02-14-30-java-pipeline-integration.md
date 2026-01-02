# Research Report: Java Language Behavior for Pipeline Integration

**Date**: 2026-01-02 14:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

Java language implementation lacks `build_resolution_context` and `build_resolution_context_with_cache` methods. Unlike TypeScript/JavaScript, Java uses a simpler approach with `create_resolution_context` and `initialize_resolution_context`. Adding `build_resolution_context_with_pipeline_cache` requires implementing the method following the default trait implementation pattern, with Java-specific package path enhancement.

## Key Findings

### 1. Missing Resolution Context Methods

Java behavior does NOT implement:
- `build_resolution_context` (requires Tantivy)
- `build_resolution_context_with_cache` (requires Tantivy + ConcurrentSymbolCache)
- `build_resolution_context_with_pipeline_cache` (needed for parallel pipeline)

It uses instead:
- `create_resolution_context(file_id)` - Creates empty `JavaResolutionContext`
- `initialize_resolution_context(&mut context, file_id)` - Populates context from BehaviorState

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/java/behavior.rs:253-281`

```rust
/// Create resolution context for this file
pub fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
    Box::new(super::JavaResolutionContext::new(file_id))
}

/// Initialize resolution context with imports and file state
pub fn initialize_resolution_context(
    &self,
    context: &mut dyn ResolutionScope,
    file_id: FileId,
) {
    // Populates imports from BehaviorState
}
```

### 2. Provider Registration Pattern

Java provider registration follows the standard pattern:

**Evidence**: `/Users/bartolli/Projects/codanna/src/main.rs:35`
```rust
registry.add(Arc::new(JavaProvider::new()));
```

`JavaProvider` implements `ProjectResolutionProvider` trait with:
- `language_id()` returns `"java"`
- `rebuild_cache()` parses Maven/Gradle configs and persists to `.codanna/java.index`
- `package_for_file()` converts file path to package notation

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/providers/java.rs:36-216`

### 3. Resolution Rules Application

Java currently applies resolution rules through `module_path_from_file`:
- Uses thread-local cache with 1-second TTL
- Loads `ResolutionIndex` from `.codanna/java.index`
- Extracts package from file path using source roots

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/java/behavior.rs:166-228`

```rust
fn module_path_from_file(&self, file_path: &Path, _project_root: &Path) -> Option<String> {
    // Thread-local cache pattern (matches TypeScript)
    thread_local! {
        static RULES_CACHE: RefCell<Option<(Instant, ResolutionIndex)>> = const { RefCell::new(None) };
    }
    // ... loads from .codanna/java.index and extracts package
}
```

### 4. No JavaProjectEnhancer

Unlike TypeScript which has `TypeScriptProjectEnhancer` for path alias resolution, Java does NOT have a `JavaProjectEnhancer`.

**Evidence**: Grep search found no matches for `JavaProjectEnhancer` in `/Users/bartolli/Projects/codanna/src/parsing/java/`

Java does not need path alias enhancement because:
- Java imports use fully qualified class names (e.g., `import com.example.MyClass`)
- Package structure mirrors directory structure (convention-based)
- No tsconfig-style path aliases

### 5. JavaResolutionContext Implementation

Java has a 5-tier scope system:

1. **Local** - variables/parameters in methods/blocks
2. **Class** - fields/methods of current class
3. **File** - other classes in same file
4. **Imported** - symbols from import statements
5. **Package** - same package (package-private access)

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/java/resolution.rs:25-45`

Resolution order: local -> class -> file -> imported -> package

## Architecture/Patterns Identified

### Trait Method Pattern

The `LanguageBehavior` trait provides three resolution context builders:

| Method | Uses | TypeScript | JavaScript | Java |
|--------|------|------------|------------|------|
| `build_resolution_context` | Tantivy | Override | Override | Default |
| `build_resolution_context_with_cache` | Tantivy + Cache | Override | Override | Default |
| `build_resolution_context_with_pipeline_cache` | PipelineSymbolCache | Override | Default | Default |

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/language_behavior.rs:511-1130`

### Default Implementation

The default `build_resolution_context_with_pipeline_cache` in `LanguageBehavior`:
1. Creates language-specific context via `create_resolution_context()`
2. Normalizes import paths (replaces `/` with module separator)
3. Resolves imports via `PipelineSymbolCache::resolve()`
4. Adds local file symbols via `PipelineSymbolCache::symbols_in_file()`
5. Calls `initialize_resolution_context()`

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/language_behavior.rs:987-1130`

## Required Changes for Pipeline Integration

### Option A: Use Default Implementation

Java can use the default `build_resolution_context_with_pipeline_cache` from `LanguageBehavior` trait:

1. Ensure `create_resolution_context()` returns proper `JavaResolutionContext`
2. Ensure `initialize_resolution_context()` populates imports correctly
3. Ensure `is_resolvable_symbol()` filters appropriately

Current Java implementation already satisfies these requirements.

### Option B: Custom Implementation (If Needed)

If Java-specific enhancement is needed (e.g., static import handling):

```rust
// In /Users/bartolli/Projects/codanna/src/parsing/java/behavior.rs
fn build_resolution_context_with_pipeline_cache(
    &self,
    file_id: FileId,
    imports: &[Import],
    cache: &dyn PipelineSymbolCache,
) -> (Box<dyn ResolutionScope>, Vec<Import>) {
    let mut context = JavaResolutionContext::new(file_id);
    let importing_module = self.get_module_path_for_file(file_id);

    // Build enhanced imports (Java-specific handling for static imports)
    let enhanced_imports: Vec<Import> = imports
        .iter()
        .map(|import| {
            let enhanced_path = if import.path.starts_with("static ") {
                // Handle static imports: "static com.example.Class.method"
                import.path.strip_prefix("static ").unwrap_or(&import.path).to_string()
            } else {
                import.path.clone()
            };
            Import {
                path: enhanced_path,
                file_id: import.file_id,
                alias: import.alias.clone(),
                is_glob: import.is_glob,
                is_type_only: import.is_type_only,
            }
        })
        .collect();

    context.populate_imports(&enhanced_imports);

    // Add imported symbols via cache resolution
    let caller = CallerContext::from_file(file_id, self.language_id());
    for import in &enhanced_imports {
        let symbol_name = import.path.rsplit('.').next().unwrap_or(&import.path);
        if let ResolveResult::Found(id) = cache.resolve(symbol_name, &caller, None, imports) {
            context.add_symbol(symbol_name.to_string(), id, ScopeLevel::Global);
        }
    }

    // Add local file symbols
    for symbol_id in cache.symbols_in_file(file_id) {
        if let Some(symbol) = cache.get(symbol_id) {
            if self.is_resolvable_symbol(&symbol) {
                context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Module);
            }
        }
    }

    self.initialize_resolution_context(context.as_mut(), file_id);
    (Box::new(context), enhanced_imports)
}
```

### Missing: `load_project_rules_for_file`

Java lacks the helper method that TypeScript/JavaScript have:

```rust
fn load_project_rules_for_file(&self, file_id: FileId) -> Option<ResolutionRules>
```

This is used by TypeScript to load path alias rules for the ProjectEnhancer. Java may not need this since it does not use path aliases, but if source root enhancement is needed, this method should be added.

## Conclusions

1. **Java can use default implementation**: The current Java setup should work with the default `build_resolution_context_with_pipeline_cache` from the `LanguageBehavior` trait.

2. **No ProjectEnhancer needed**: Java's package-based imports do not require path alias resolution like TypeScript's tsconfig paths.

3. **Static imports may need special handling**: If static imports (e.g., `import static java.util.Collections.*`) are not resolving correctly, a custom override may be needed.

4. **Test with pipeline**: Verify that the default implementation works by running Java files through the parallel pipeline and checking resolution accuracy.

### Recommended Next Steps

1. Test Java with parallel pipeline using default implementation
2. If resolution issues arise, implement custom `build_resolution_context_with_pipeline_cache`
3. Consider adding `load_project_rules_for_file` if source root awareness is needed during resolution
