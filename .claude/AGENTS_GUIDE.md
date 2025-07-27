# Claude Code Agents Guide

A practical guide to using AI sub-agents for Rust development. Following the Rust philosophy: simple, type-safe, and efficient.

## What Are Agents?

Sub-agents are specialized AI assistants that handle specific tasks. They run in their own context, preventing clutter in your main conversation while providing focused expertise.

## Current Agents

### 1. **code-documenter** 📚
*Generates concise documentation optimized for code intelligence indexing*

**When to use:**
- Before indexing new code for your code intelligence system
- When public APIs lack documentation
- After creating new modules or complex functions

**Usage:**
```
/doc-file src/parser/rust.rs
```

**What it does:**
- Adds 2-4 line doc comments to all public items
- Includes performance characteristics when relevant
- Skips code examples (avoids linter issues)
- Focuses on "what" and "why", not implementation details

---

### 2. **test-runner** ✅
*Runs tests and automatically fixes failures*

**When to use:**
- After refactoring or API changes
- When you see "cargo test" failures
- During rapid prototyping to keep tests green

**Usage:**
```
Use the test-runner agent to fix failing tests
```

**What it does:**
- Runs `cargo test` and parses failures
- Updates test code to match new APIs
- Fixes import errors in tests
- Updates expected values when behavior changes

---

### 3. **import-fixer** 🔧
*Resolves missing imports and use statements*

**When to use:**
- When you see "cannot find type/function" errors
- After moving code between modules
- When adding new dependencies

**Usage:**
```
Use import-fixer to resolve the missing imports
```

**What it does:**
- Runs `cargo check` and finds import errors
- Adds appropriate `use` statements
- Removes unused imports
- Groups and organizes imports properly

---

### 4. **quality-reviewer** 🔍
*Reviews Rust code for project coding principles*

**When to use:**
- After implementing new features
- Before committing significant changes
- When you want feedback on API design

**Usage:**
```
Use quality-reviewer to check my recent changes
```

**What it does:**
- Checks function signatures follow project guidelines
- Reviews error handling patterns
- Validates type design decisions
- Suggests performance improvements

---

### 5. **vector-engineer** 🔮
*Specialist for vector search and IVFFlat implementation*

**When to use:**
- Implementing vector search features
- Working with Tantivy integration
- Optimizing similarity search performance

**Usage:**
```
Use vector-engineer to implement IVFFlat indexing
```

**What it does:**
- Designs vector indexing pipelines
- Implements IVFFlat with Tantivy
- Follows TDD practices for search features
- Optimizes vector search performance

---

### 6. **tdd-architect** 🏗️
*Designs features through test-driven development*

**When to use:**
- Before implementing new features
- When designing new APIs
- During POC to production migration
- When you need to define behavior through tests

**Usage:**
```
Use tdd-architect to design the vector search API through tests
```

**What it does:**
- Creates tests that define production APIs
- Ensures tests import from real production modules
- Guides POC → Production migration path
- Designs error handling and performance requirements
- Follows test-first development methodology

## Good to Have Agents

These agents would align with Rust development style and save significant time:

### 1. **cargo-dep-resolver** 📦
*Manages Cargo dependencies intelligently*

Would handle:
- Finding the right crate for a task
- Checking for security advisories
- Updating dependency versions safely
- Resolving version conflicts
- Adding feature flags correctly

Example: "Use cargo-dep-resolver to add async HTTP client support"

---

### 2. **unsafe-auditor** 🛡️
*Reviews and documents unsafe code blocks*

Would handle:
- Auditing unsafe blocks for soundness
- Documenting safety invariants
- Suggesting safe alternatives
- Adding proper safety comments
- Checking for undefined behavior

Example: "Use unsafe-auditor to review the FFI bindings"

---

### 3. **trait-implementor** 🔗
*Implements standard traits correctly*

Would handle:
- Implementing Display, Debug, Error traits
- Deriving vs manual implementation decisions
- Handling trait coherence rules
- Adding proper trait bounds
- Implementing Iterator patterns

Example: "Use trait-implementor to add Display and Error to our types"

---

### 4. **bench-optimizer** ⚡
*Optimizes performance bottlenecks*

Would handle:
- Running benchmarks with criterion
- Identifying hot paths
- Suggesting optimization strategies
- Avoiding premature optimization
- Measuring before/after performance

Example: "Use bench-optimizer to improve parse_file performance"

---

### 5. **lifetime-wizard** 🧙
*Resolves complex lifetime issues*

Would handle:
- Fixing lifetime compiler errors
- Simplifying lifetime annotations
- Suggesting ownership patterns
- Converting to Arc/Rc when appropriate
- Explaining lifetime relationships

Example: "Use lifetime-wizard to fix the borrow checker errors"

## Agent Best Practices

### DO:
- Use agents for mechanical, repetitive tasks
- Let agents handle the boring parts
- Chain agents for complex workflows
- Trust the Rust compiler's guidance

### DON'T:
- Over-engineer agent responsibilities  
- Use agents for creative decisions
- Fight the borrow checker with agents
- Skip code review after agent changes

## Creating New Agents

When creating agents, follow Rust principles:

1. **Single Responsibility** - One agent, one job
2. **Type Safety First** - Let the compiler guide fixes
3. **Zero Cost** - Don't add runtime overhead
4. **Explicit Over Implicit** - Clear about what changes are made
5. **Minimal Dependencies** - Use std library when possible

## Example Workflow

A typical rapid development session:

```bash
# 1. Design new feature with TDD
"Use tdd-architect to design the new search API"

# 2. Make your implementation
vim src/search/vector_index.rs

# 3. Let import-fixer handle missing imports
"Use import-fixer to resolve errors"

# 4. Run test-runner to fix broken tests  
"Use test-runner to update failing tests"

# 5. Document new public APIs
/doc-file src/search/vector_index.rs

# 6. Get code review
"Use quality-reviewer to check changes"
```

Or for POC → Production migration:

```bash
# 1. Create POC test that defines ideal API
"Use tdd-architect to create poc_vector_search_test.rs"

# 2. Implement in production location
vim src/search/mod.rs

# 3. Migrate POC code gradually
"Move working code from POC to production, keeping tests green"

# 4. Clean up when complete
rm tests/poc_vector_search_test.rs
```

## Tips

- **Proactive Agents**: Some agents work best when used immediately (import-fixer after errors)
- **Batch Operations**: Run agents on multiple files when patterns repeat
- **Trust But Verify**: Agents save time but always review their changes
- **Compiler Is King**: When in doubt, `cargo check` has the final say

Remember: Agents are tools to eliminate toil, not replace thinking. Use them to handle the mechanical parts so you can focus on design and architecture.