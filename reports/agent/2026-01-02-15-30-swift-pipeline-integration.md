# Research Report: Swift Language Behavior for Pipeline Integration

**Date**: 2026-01-02 15:30
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

Swift currently uses the default `build_resolution_context_with_pipeline_cache` from the base `LanguageBehavior` trait in `language_behavior.rs`. Unlike TypeScript which has a custom override with `TypeScriptProjectEnhancer` for tsconfig path alias resolution, Swift relies entirely on generic behavior. To add pipeline cache support with Swift-specific resolution, you would need to add a `SwiftProjectEnhancer` and override the method in `SwiftBehavior`.

## Key Findings

### 1. Swift Does NOT Override build_resolution_context Methods

Swift's `SwiftBehavior` in `src/parsing/swift/behavior.rs` does not implement:
- `build_resolution_context`
- `build_resolution_context_with_cache`
- `build_resolution_context_with_pipeline_cache`

It relies on the default implementations from the `LanguageBehavior` trait.

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/swift/behavior.rs:1-290` - No override methods present. Grep for `build_resolution_context` in swift directory returns no matches.

### 2. Swift Provider Registration

Swift has two registration points:

**Project Resolution Provider** (main.rs:38):
```rust
registry.add(Arc::new(SwiftProvider::new()));
```

**Language Registry** (parsing/swift/definition.rs:55-57):
```rust
pub fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(SwiftLanguage));
}
```

Called from `parsing/registry.rs:385`:
```rust
super::swift::register(registry);
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/main.rs:11,38` and `/Users/bartolli/Projects/codanna/src/parsing/swift/definition.rs:55-57`

### 3. Swift Has module_path_from_file with Cached Rules

Swift already has caching infrastructure in `module_path_from_file`:

```rust
fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
    use crate::project_resolver::persist::ResolutionPersistence;
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    // Thread-local cache with 1-second TTL
    thread_local! {
        static RULES_CACHE: RefCell<Option<(Instant, ResolutionIndex)>> = const { RefCell::new(None) };
    }
    // ... loads from .codanna/swift persistence
}
```

This pattern mirrors TypeScript's `load_project_rules_for_file`.

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/swift/behavior.rs:132-210`

### 4. No SwiftProjectEnhancer Exists

Unlike TypeScript which has `TypeScriptProjectEnhancer` implementing `ProjectResolutionEnhancer` trait for path alias resolution, Swift has no equivalent. Grep for `SwiftProjectEnhancer|SwiftEnhancer` returns no matches.

TypeScript pattern:
```rust
pub struct TypeScriptProjectEnhancer {
    resolver: Option<PathAliasResolver>,
}

impl ProjectResolutionEnhancer for TypeScriptProjectEnhancer {
    fn enhance_import_path(&self, import_path: &str, _from_file: FileId) -> Option<String>;
    fn get_import_candidates(&self, import_path: &str, _from_file: FileId) -> Vec<String>;
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/typescript/resolution.rs:853-920`

### 5. Swift Resolution Rules Structure

`SwiftProvider::build_rules_for_config` returns `ResolutionRules` with:
- `base_url: None` (always)
- `paths: HashMap<String, Vec<String>>` containing source roots

Swift rules store source directories (Sources/, Tests/) but no path aliases like tsconfig.

**Evidence**: `/Users/bartolli/Projects/codanna/src/project_resolver/providers/swift.rs:183-193`

### 6. Default Pipeline Cache Implementation

The base trait implementation in `language_behavior.rs:987-1125`:
1. Creates language-specific context via `create_resolution_context(file_id)`
2. Normalizes imports (converts `./` to module separators)
3. Uses `PipelineSymbolCache.resolve()` for multi-tier lookup
4. Adds local symbols via `cache.symbols_in_file(file_id)`
5. Calls `initialize_resolution_context()` for final setup

Swift would inherit this generic flow without any Swift-specific enhancement.

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/language_behavior.rs:987-1125`

## Architecture/Patterns Identified

### Resolution Context Hierarchy

```
LanguageBehavior (trait)
  build_resolution_context_with_pipeline_cache() // default impl
    |
    +-- TypeScriptBehavior (override)
    |     uses TypeScriptProjectEnhancer
    |     load_project_rules_for_file()
    |
    +-- JavaScriptBehavior (override)
    |     uses TypeScriptProjectEnhancer (shares tsconfig)
    |     load_project_rules_for_file()
    |
    +-- SwiftBehavior (uses default)
          NO override - uses base implementation
```

### Provider vs Enhancer Pattern

| Component | Role | Swift Status |
|-----------|------|--------------|
| `SwiftProvider` | Build/persist resolution rules from Package.swift | EXISTS |
| `SwiftProjectEnhancer` | Apply rules during resolution (path alias) | MISSING |
| `load_project_rules_for_file` | Load cached rules at resolution time | MISSING (but partial in module_path_from_file) |

## Changes Required for Pipeline Cache Integration

### Option A: Swift-Specific Override (Recommended if Swift needs path enhancement)

1. **Add `load_project_rules_for_file` to SwiftBehavior** (similar to TypeScript):
```rust
fn load_project_rules_for_file(&self, file_id: FileId) -> Option<ResolutionRules> {
    // Refactor existing RULES_CACHE from module_path_from_file
}
```

2. **Create `SwiftProjectEnhancer`** in `resolution.rs`:
```rust
pub struct SwiftProjectEnhancer {
    source_roots: Vec<PathBuf>,
}

impl ProjectResolutionEnhancer for SwiftProjectEnhancer {
    fn enhance_import_path(&self, import_path: &str, _from_file: FileId) -> Option<String> {
        // Swift module resolution logic
    }
}
```

3. **Override `build_resolution_context_with_pipeline_cache`** in `SwiftBehavior`:
```rust
fn build_resolution_context_with_pipeline_cache(
    &self,
    file_id: FileId,
    imports: &[Import],
    cache: &dyn PipelineSymbolCache,
) -> (Box<dyn ResolutionScope>, Vec<Import>) {
    let mut context = SwiftResolutionContext::new(file_id);
    let maybe_enhancer = self.load_project_rules_for_file(file_id)
        .map(SwiftProjectEnhancer::new);
    // ... similar to TypeScript impl
}
```

### Option B: Use Default Implementation (If Swift doesn't need path enhancement)

Swift imports are module-level (`import Foundation`), not path-based like TypeScript (`import { x } from '@/utils'`). If Swift doesn't need import path enhancement, the default implementation is sufficient.

The default already:
- Creates `SwiftResolutionContext`
- Normalizes import paths
- Resolves via `PipelineSymbolCache`
- Adds local symbols

## Conclusions

1. **Swift uses the default pipeline cache implementation** - no custom override exists
2. **Swift has the provider infrastructure** but lacks the enhancer pattern for resolution-time path transformation
3. **The existing `module_path_from_file` caching pattern** can be refactored into `load_project_rules_for_file`
4. **Swift Package Manager uses source roots, not path aliases** - enhancement may not be needed
5. **If enhancement is needed**, follow the TypeScript pattern: add `SwiftProjectEnhancer` + `load_project_rules_for_file` + override `build_resolution_context_with_pipeline_cache`

### Relevant Files

- Behavior: `/Users/bartolli/Projects/codanna/src/parsing/swift/behavior.rs`
- Resolution: `/Users/bartolli/Projects/codanna/src/parsing/swift/resolution.rs`
- Provider: `/Users/bartolli/Projects/codanna/src/project_resolver/providers/swift.rs`
- Base trait: `/Users/bartolli/Projects/codanna/src/parsing/language_behavior.rs:987-1125`
- TypeScript reference: `/Users/bartolli/Projects/codanna/src/parsing/typescript/behavior.rs:912-1048`
