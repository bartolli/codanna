# Research Report: JavaScript Language Behavior for Pipeline Integration

**Date**: 2026-01-02 16:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

JavaScript has `build_resolution_context` and `build_resolution_context_with_cache` methods but lacks `build_resolution_context_with_pipeline_cache` required for the parallel indexing pipeline. The `JavaScriptProjectEnhancer` exists and implements `ProjectResolutionEnhancer` trait, so adding pipeline support requires implementing the pipeline-specific method following the TypeScript pattern.

## Key Findings

### 1. Current Resolution Methods in JavaScriptBehavior

JavaScript behavior.rs (1055 lines) implements:

- `build_resolution_context` (line 512-604): Standard resolution using DocumentIndex
- `build_resolution_context_with_cache` (line 693-874): Fast path using ConcurrentSymbolCache

**Missing**: `build_resolution_context_with_pipeline_cache` - the method called by the CONTEXT stage of the parallel pipeline.

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/javascript/behavior.rs:512-874`

### 2. JavaScriptProjectEnhancer Implementation

The enhancer is fully implemented at resolution.rs:596-656:

```rust
pub struct JavaScriptProjectEnhancer {
    resolver: Option<crate::parsing::javascript::jsconfig::PathAliasResolver>,
}

impl JavaScriptProjectEnhancer {
    pub fn new(rules: ResolutionRules) -> Self { ... }
}

impl ProjectResolutionEnhancer for JavaScriptProjectEnhancer {
    fn enhance_import_path(&self, import_path: &str, _from_file: FileId) -> Option<String> { ... }
    fn get_import_candidates(&self, import_path: &str, _from_file: FileId) -> Vec<String> { ... }
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/javascript/resolution.rs:596-656`

### 3. Provider Registration Pattern

JavaScript uses the standard language definition pattern:

```rust
// definition.rs:49-51
pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(JavaScriptLanguage));
}
```

Provider is a separate struct (`JavaScriptProvider`) in project_resolver/providers/javascript.rs that handles jsconfig.json parsing and SHA-based invalidation.

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/javascript/definition.rs:49-51`

### 4. Rules Loading Pattern

JavaScript uses thread-local caching for resolution rules:

```rust
fn load_project_rules_for_file(&self, file_id: FileId) -> Option<ResolutionRules> {
    thread_local! {
        static RULES_CACHE: RefCell<Option<(Instant, ResolutionIndex)>> = ...;
    }
    // Cache invalidates after 1 second
    // Loads from ResolutionPersistence for "javascript"
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/javascript/behavior.rs:86-126`

### 5. TypeScript Reference Implementation

TypeScript's `build_resolution_context_with_pipeline_cache` (line 912-1032) provides the template:

```rust
fn build_resolution_context_with_pipeline_cache(
    &self,
    file_id: FileId,
    imports: &[crate::parsing::Import],
    cache: &dyn crate::parsing::PipelineSymbolCache,
) -> (
    Box<dyn crate::parsing::ResolutionScope>,
    Vec<crate::parsing::Import>,
) {
    // 1. Create TypeScriptResolutionContext
    // 2. Load project rules via load_project_rules_for_file()
    // 3. Create TypeScriptProjectEnhancer from rules
    // 4. For each import:
    //    - Get local_name (alias or last path segment)
    //    - Enhance path via enhancer.enhance_import_path()
    //    - Lookup candidates in cache by local_name
    //    - Match by module_path
    //    - Register import binding
    // 5. Populate context with enhanced imports
    // 6. Add local symbols from cache
    // Return (context, enhanced_imports)
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/typescript/behavior.rs:912-1032`

## Architecture/Patterns Identified

### Pipeline Integration Flow

1. CONTEXT stage calls `behavior.build_resolution_context_with_pipeline_cache()`
2. Method receives: `file_id`, raw `imports`, `PipelineSymbolCache`
3. Method returns: `(ResolutionScope, enhanced_imports)`
4. Enhanced imports have path aliases resolved (e.g., `@/components` -> `src.components`)

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/context.rs:110-140`

### Key Differences from `build_resolution_context_with_cache`

| Aspect | with_cache | with_pipeline_cache |
|--------|-----------|---------------------|
| Cache type | `ConcurrentSymbolCache` | `PipelineSymbolCache` trait |
| Returns | `Box<dyn ResolutionScope>` | `(Box<dyn ResolutionScope>, Vec<Import>)` |
| Import source | `self.get_imports_for_file()` | Passed as parameter |
| Purpose | SimpleIndexer fast path | Parallel pipeline CONTEXT stage |

## Implementation Requirements

To add `build_resolution_context_with_pipeline_cache` to JavaScript:

1. **Method signature** (must match trait default):
```rust
fn build_resolution_context_with_pipeline_cache(
    &self,
    file_id: FileId,
    imports: &[crate::parsing::Import],
    cache: &dyn crate::parsing::PipelineSymbolCache,
) -> (
    Box<dyn crate::parsing::ResolutionScope>,
    Vec<crate::parsing::Import>,
)
```

2. **Required components** (all exist):
   - `JavaScriptResolutionContext::new(file_id)` - resolution.rs:63
   - `self.load_project_rules_for_file(file_id)` - behavior.rs:86
   - `JavaScriptProjectEnhancer::new(rules)` - resolution.rs:601
   - `enhancer.enhance_import_path()` - resolution.rs:626
   - `context.register_import_binding()` - resolution.rs:446
   - `context.populate_imports()` - resolution.rs:435

3. **Key adaptations from TypeScript**:
   - Remove `is_type_only` handling (JavaScript has no type-only imports)
   - Use `normalize_js_import()` helper (behavior.rs:26-69) for relative path resolution
   - Strip JS extensions (`.js`, `.jsx`, `.mjs`, `.cjs`)

## Conclusions

JavaScript has all the infrastructure needed for pipeline integration:
- `JavaScriptResolutionContext` implements `ResolutionScope`
- `JavaScriptProjectEnhancer` implements `ProjectResolutionEnhancer`
- Rules loading via `load_project_rules_for_file()` is already implemented

The missing piece is the `build_resolution_context_with_pipeline_cache` method. Implementation should follow TypeScript's pattern (~120 lines) with JavaScript-specific simplifications (no type-only imports, JS extension handling).

Estimated effort: 100-150 lines of code, primarily adapting TypeScript's implementation.
