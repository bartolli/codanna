# Research Report: LanguageBehavior Resolution Overrides

**Date**: 2026-01-01 15:30
**Agent**: Research-Agent-v5
**Model**: Sonnet 4.5

## Summary

The `LanguageBehavior` trait defines a template method pattern for resolution context building. The base trait provides default implementations for `build_resolution_context*` methods that orchestrate the process, while languages customize behavior by overriding specific hook methods. The behavior prepares the scope; the scope provides resolution logic.

## Key Findings

### 1. Resolution Method Hierarchy

The base trait in `src/parsing/language_behavior.rs` provides three build methods:

| Method | Lines | Purpose |
|--------|-------|---------|
| `build_resolution_context` | 511-670 | Standard path using `DocumentIndex` (Tantivy) |
| `build_resolution_context_with_cache` | 674-970 | Fast path using `ConcurrentSymbolCache` |
| `build_resolution_context_with_pipeline_cache` | 985-1094 | Parallel pipeline (no Tantivy access) |

All three follow the same template:
1. Call `create_resolution_context(file_id)` to get language-specific scope
2. Populate imports via `get_imports_for_file()` and `resolve_import()`
3. Add file's local symbols via `is_resolvable_symbol()` filter
4. Add visible cross-file symbols via `is_symbol_visible_from_file()` filter
5. Call `initialize_resolution_context()` hook for post-processing

**Evidence**: `src/parsing/language_behavior.rs:511-670`

### 2. Hook Methods Each Language Can Override

| Method | Purpose | Override Count |
|--------|---------|----------------|
| `create_resolution_context()` | Return language-specific `ResolutionScope` impl | 13 languages |
| `is_resolvable_symbol()` | Filter which symbols enter context | 10 languages |
| `import_matches_symbol()` | Match import path to symbol module path | 12 languages |
| `build_resolution_context*()` | Completely replace context building | 4 languages |
| `initialize_resolution_context()` | Post-population hook | 2 languages (Swift, Kotlin) |

**Evidence**: Grep results across `src/parsing/*/behavior.rs`

### 3. Languages That Override build_resolution_context

Only four languages provide custom `build_resolution_context*` implementations:

1. **TypeScript** - All three variants (`build_resolution_context`, `build_resolution_context_with_cache`, `build_resolution_context_with_pipeline_cache`)
   - Uses `TypeScriptProjectEnhancer` for tsconfig path aliases
   - Handles type-only imports separately
   - Evidence: `src/parsing/typescript/behavior.rs:472-1026`

2. **JavaScript** - Two variants (`build_resolution_context`, `build_resolution_context_with_cache`)
   - Similar to TypeScript but without type-space handling
   - Evidence: `src/parsing/javascript/behavior.rs:511-888`

3. **Go** - One variant (`build_resolution_context`)
   - Package-based scoping with implicit visibility
   - Evidence: `src/parsing/go/behavior.rs:210-286`

4. **C#** - One variant (`build_resolution_context`)
   - Namespace-based resolution
   - Evidence: `src/parsing/csharp/behavior.rs:355-440`

### 4. TypeScript Path Enhancement Flow

The TypeScript behavior has a specialized path for tsconfig.json alias resolution:

```
import "@app/utils" in file.ts
       |
       v
TypeScriptBehavior.build_resolution_context_with_cache()
       |
       v
load_project_rules_for_file(file_id) -> ResolutionRules
       |
       v
TypeScriptProjectEnhancer::new(rules)
       |
       v
enhancer.enhance_import_path("@app/utils", file_id)
       |
       v
PathAliasResolver.resolve_import("@app/utils")
       |
       v
"./src/app/utils" (resolved from tsconfig paths)
```

Key implementation:
- `TypeScriptProjectEnhancer` wraps `ResolutionRules` (from persisted tsconfig)
- `enhance_import_path()` returns `Some(resolved_path)` for aliases, `None` for relative imports
- Relative imports (`./`, `../`) bypass enhancement and use `normalize_ts_import()`

**Evidence**: `src/parsing/typescript/resolution.rs:853-910`

### 5. Behavior -> Scope Relationship

The relationship is **one-directional preparation**:

1. **Behavior prepares Scope**: The behavior creates and populates the scope
   - `create_resolution_context()` instantiates the language-specific scope
   - `build_resolution_context*()` calls `context.add_symbol()`, `context.populate_imports()`, etc.

2. **Scope does NOT call back to Behavior**: The scope is a passive container
   - `ResolutionScope::resolve()` uses internal data structures only
   - `ResolutionScope::is_compatible_relationship()` uses language-specific rules internally

3. **The scope is used AFTER building completes**: Resolution happens during relationship extraction
   - `context.resolve(name)` returns `Option<SymbolId>`
   - `context.is_external_import(name)` checks import origins

**Evidence**: `src/parsing/resolution.rs:56-200` (trait definition)

### 6. Language-Specific ResolutionContext Implementations

Each language has a custom `ResolutionContext` that implements `ResolutionScope`:

| Language | Context | Key Feature |
|----------|---------|-------------|
| TypeScript | `TypeScriptResolutionContext` | Type/value space separation, hoisting |
| Python | `PythonResolutionContext` | LEGB scope chain |
| Rust | `RustResolutionContext` | Crate-based resolution |
| Go | `GoResolutionContext` | Package-level visibility |
| Java | `JavaResolutionContext` | Package imports, star imports |
| C# | `CSharpResolutionContext` | Namespace resolution |
| Generic | `GenericResolutionContext` | Default fallback |

**Evidence**: `src/parsing/resolution.rs` (trait) and each `src/parsing/*/resolution.rs`

### 7. import_matches_symbol Customization

This method determines if an import path matches a symbol's module path. Each language has different rules:

**Python** (`src/parsing/python/behavior.rs:370-420`):
- Handles relative imports (`.module`, `..parent`)
- Resolves against importing module path

**TypeScript** (`src/parsing/typescript/behavior.rs:1216+`):
- Path aliases via `enhance_import_path`
- Handles `index.ts` barrel files

**Go** (`src/parsing/go/behavior.rs:407+`):
- Package path matching
- Alias support

## Architecture Pattern

```
┌─────────────────────────────────────────────────────────────┐
│                    LanguageBehavior                          │
│  (Template Method Pattern)                                   │
│                                                             │
│  build_resolution_context()                                  │
│    ├── create_resolution_context()  ─────► Hook (override)  │
│    ├── get_imports_for_file()       ─────► Uses BehaviorState│
│    ├── resolve_import()             ─────► Calls DocumentIndex│
│    ├── is_resolvable_symbol()       ─────► Hook (override)  │
│    ├── is_symbol_visible_from_file()─────► Hook (override)  │
│    └── initialize_resolution_context()───► Hook (override)  │
└─────────────────────────────────────────────────────────────┘
                           │
                           │ returns
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                    ResolutionScope                           │
│  (Strategy Pattern - Language-specific impl)                 │
│                                                             │
│  TypeScriptResolutionContext / PythonResolutionContext / ...│
│                                                             │
│  resolve(name) -> Option<SymbolId>                          │
│  is_external_import(name) -> bool                           │
│  is_compatible_relationship(...) -> bool                    │
└─────────────────────────────────────────────────────────────┘
```

## Conclusions

1. **Most languages use defaults**: Only TypeScript, JavaScript, Go, and C# override the full `build_resolution_context` methods. Others rely on hook overrides.

2. **TypeScript is the most complex**: Three custom build methods, path alias enhancement, and type/value space separation.

3. **The behavior-scope split is clean**: Behavior handles data gathering and orchestration; scope handles resolution rules.

4. **Pipeline version avoids Tantivy**: `build_resolution_context_with_pipeline_cache` uses `PipelineSymbolCache` for parallel indexing without index locks.

5. **Path enhancement is TypeScript-only**: The `TypeScriptProjectEnhancer` and `ProjectResolutionEnhancer` trait exist specifically for tsconfig alias support.
