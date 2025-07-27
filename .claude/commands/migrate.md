---
allowed-tools: Task
description: Perform systematic code migrations using the migration-engineer
argument-hint: <pattern> [options] | function-sig <old> -> <new> | type <old> -> <new> | async <module>
---

# 🔄 Rust Code Migration

Use the migration-engineer to perform systematic code migrations across the codebase.

## Usage Examples

### Function signature change:
`/migrate function-sig process(path: &str) -> process(path: &Path, config: &Config)`

### Type migration:
`/migrate type Result<T, String> -> Result<T, IndexError>`

### Async conversion:
`/migrate async src/storage`

### Pattern update:
`/migrate pattern unwrap() -> proper-errors`

### Error handling:
`/migrate errors string -> thiserror`

## Available Migrations

1. **function-sig** - Update function signatures and all call sites
2. **type** - Migrate type usage across codebase  
3. **async** - Convert sync code to async
4. **pattern** - Modernize code patterns
5. **errors** - Improve error handling
6. **api** - Stabilize public APIs
7. **perf** - Apply performance optimizations

## Task

Use the migration-engineer agent to perform the following migration:

**Migration requested**: $ARGUMENTS

The agent will:
1. Analyze the scope of changes needed
2. Update code incrementally
3. Fix compiler errors mechanically
4. Ensure tests continue passing
5. Apply clippy suggestions

This saves significant time on mechanical refactoring while Rust's type system ensures correctness.