# Code Guidelines

This document provides a strict and consolidated set of Rust development guidelines for this project, derived from implementation experience and common errors. Adherence to these rules is **mandatory** to ensure code quality, performance, and maintainability. These rules supersede any conflicting guidelines in other documents.

## 1. Function Signatures: Zero-Cost Abstractions are MANDATORY

This is the most critical principle. Violations require immediate fixing. The goal is to maximize caller flexibility and eliminate unnecessary memory allocations.

### Parameters
- **MUST** use borrowed types for read-only data: `&str` over `String`, `&[T]` over `Vec<T>`
- **MUST** use owned types **only** when you need to store or transform the data
- **MUST** use `impl Trait` instead of heap-allocated trait objects (`Box<dyn Trait>`)

### Return Values
- **SHOULD** return iterators (`impl Iterator`) when it avoids allocation
- **MUST NOT** return iterators if the caller always needs a collected result
- **MUST** use `Cow<'_, str>` for conditional ownership scenarios

```rust
// ✅ CORRECT: Zero allocation, flexible for caller
fn parse_config(input: &str) -> Result<Config, Error> { ... }
fn find_symbols<'a>(code: &'a str) -> impl Iterator<Item = &'a str> { ... }

// ❌ INCORRECT: Forces allocation or ownership transfer
fn parse_config(input: String) -> Result<Config, Error> { ... }
fn find_symbols(code: &str) -> Vec<String> { ... }

// ✅ CORRECT: Iterator that avoids allocation in the common case
fn extract_words(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter(|w| !w.is_empty())
}

// ❌ INCORRECT: Returns iterator when result is always collected
fn get_all_ids(&self) -> impl Iterator<Item = UserId> {
    // Bad: Caller always does .collect(), should return Vec<UserId>
}
```

## 2. Performance: Measure, Don't Guess

Performance optimizations **MUST** be justified with measurements. Different rules apply to different code paths.

### Hot Path Rules (>1000 calls/second)
- **MUST NOT** allocate memory - use iterators and borrowing
- **MUST NOT** use `.clone()` unless measured and justified
- **MUST** use `&[T]` over `Vec<T>`, `&str` over `String`
- **MUST** pre-allocate collections when size is known: `Vec::with_capacity()`

### Setup/Configuration Code
- **MAY** allocate memory for clarity
- **SHOULD** optimize for readability over micro-performance
- **MUST** still avoid unnecessary allocations

### Performance Targets (Project-Specific)
- Indexing: **MUST** achieve 10,000+ files/second
- Search latency: **MUST** be <10ms for semantic search
- Memory: **MUST** use ~100 bytes per symbol
- Vector operations: **MUST** achieve <1μs per vector access

```rust
// ✅ CORRECT: Hot path with zero allocations
fn calculate_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

// ✅ CORRECT: Setup code prioritizes clarity
fn load_config() -> Config {
    let paths = vec![
        home_dir().join(".config/app.toml"),
        PathBuf::from("/etc/app.toml"),
    ];
    // Allocation is fine here - runs once at startup
}
```

## 3. Type Safety: NO Primitive Obsession

**MUST** create newtype wrappers for ALL domain-specific concepts. Raw primitives for IDs, scores, or domain values are **forbidden**.

### Required Newtypes
- **ALL** IDs: `UserId(NonZeroU32)`, `ClusterId(NonZeroU32)`, `VectorId(NonZeroU32)`
- **ALL** domain values: `Score(f32)`, `Distance(f32)`, `Confidence(f32)`
- **ALL** file paths with special meaning: `IndexPath(PathBuf)`, `ConfigPath(PathBuf)`

### Type Safety Rules
- **MUST** use `NonZeroU32` for IDs that cannot be zero
- **MUST** validate constraints in newtype constructors
- **MUST** make invalid states unrepresentable at compile time

```rust
// ✅ CORRECT: Type-safe, self-documenting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClusterId(NonZeroU32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelevanceScore(f32);

impl RelevanceScore {
    pub fn new(score: f32) -> Result<Self, ValidationError> {
        if (0.0..=1.0).contains(&score) {
            Ok(Self(score))
        } else {
            Err(ValidationError::ScoreOutOfRange { score })
        }
    }
}

// ❌ INCORRECT: Primitive obsession, error-prone
fn calculate_relevance(query: &str, doc: &str) -> f32 { ... }
fn get_cluster(id: u32) -> Option<Cluster> { ... }
```

## 4. Error Handling: Structured and Actionable

### Library Code Requirements
- **MUST** use `thiserror` for all error types
- **MUST** include "Suggestion:" in every error message
- **MUST** provide actionable recovery steps
- **MUST NOT** use `anyhow` in library code

### Application Code Requirements  
- **MAY** use `anyhow` at the binary level only
- **MUST** add context when crossing module boundaries
- **MUST** use `Result<T, E>` - never `panic!` or `unwrap()`
- **MAY** use `expect()` only for truly impossible states with clear messages

```rust
#[derive(Error, Debug)]
pub enum VectorError {
    #[error("Vector dimension mismatch: expected {expected}, got {actual}\nSuggestion: Ensure all vectors are generated with the same embedding model (dimension {expected})")]
    DimensionMismatch { expected: usize, actual: usize },
    
    #[error("No vectors found in cluster {0}\nSuggestion: Check if clustering has been performed or if minimum cluster size is too high")]
    EmptyCluster(ClusterId),
    
    #[error("Search returned no results for query: {query}\nSuggestion: Try broader search terms or check if the index is properly built")]
    NoResults { query: String },
}
```

## 5. Function Design: Single Responsibility is MANDATORY

### Size Limits
- **MUST NOT** exceed 30 lines (excluding comments and whitespace)
- **SHOULD** be under 20 lines for optimal readability
- **MUST** split functions that do multiple things

### Complexity Limits
- **MUST NOT** have more than 2 levels of nesting
- **MUST NOT** mix different responsibilities (parsing + validation + transformation)
- **MUST** extract complex conditions into named predicates

```rust
// ✅ CORRECT: Each function has one clear responsibility
pub fn process_file(path: &Path) -> Result<Symbols, ProcessError> {
    let content = read_file(path)?;          // I/O responsibility
    let tokens = tokenize(&content)?;        // Lexing responsibility
    let ast = parse_tokens(&tokens)?;        // Parsing responsibility
    let symbols = extract_symbols(&ast)?;    // Extraction responsibility
    validate_symbols(&symbols)?;             // Validation responsibility
    Ok(symbols)
}

// ❌ INCORRECT: Mixed responsibilities, too long
pub fn process_file(path: &Path) -> Result<Symbols, Error> {
    let content = std::fs::read_to_string(path)?;
    let mut tokens = Vec::new();
    let mut chars = content.chars();
    while let Some(ch) = chars.next() {
        // 50+ lines of mixed tokenizing, parsing, validating...
    }
}
```

## 6. API Design: Ergonomics are MANDATORY

### Builder Pattern
- **MUST** use builder pattern for any struct with ≥3 constructor parameters
- **MUST** make builders infallible until `build()` is called
- **SHOULD** provide sensible defaults via `Default` trait

### Standard Traits
- **MUST** derive `Debug` on ALL public types (exception: types containing secrets)
- **MUST** implement `Clone` where logical (not for resources like file handles)
- **MUST** implement `PartialEq`/`Eq` for types used as keys
- **MUST** add `#[must_use]` to validation methods and builder finishers

### Method Naming
- **MUST** use `into_*` for methods that consume `self`
- **MUST** use `as_*` for methods that borrow `self`
- **MUST** use `to_*` for methods that clone/allocate
- **MUST** use `with_*` for builder methods

```rust
// ✅ CORRECT: Ergonomic builder with proper traits
#[derive(Debug, Clone)]
pub struct VectorIndex { ... }

#[derive(Debug, Default)]
pub struct VectorIndexBuilder { ... }

impl VectorIndexBuilder {
    pub fn with_dimensions(mut self, dims: usize) -> Self {
        self.dimensions = Some(dims);
        self
    }
    
    #[must_use = "Building the index may fail, check the Result"]
    pub fn build(self) -> Result<VectorIndex, BuildError> {
        // Validation happens here, not in individual setters
    }
}
```

## 7. Code Quality: Standards are NOT Optional

### Clippy Compliance
- **MUST** fix all warnings from `cargo clippy -- -W clippy::all`
- **MUST** address clippy lints before merging
- **SHOULD** enable additional lints for common mistakes

### Documentation
- **MUST** document all public APIs with examples
- **MUST** include panic conditions in doc comments
- **MUST** document performance characteristics for algorithms

### Testing
- **MUST** follow @tests/TEST_TEMPLATE.md structure
- **MUST** test error conditions, not just happy paths
- **MUST** include performance tests for critical paths

## 8. Integration Patterns (Project-Specific)

### Working with DocumentIndex
- **MUST** use batch operations when indexing multiple files
- **MUST** handle transaction rollback properly
- **MUST** warm caches after bulk operations

### Vector Search Integration
- **MUST** use `VectorUpdateCoordinator` for incremental updates
- **MUST** detect symbol-level changes before re-embedding
- **MUST** maintain consistency between text and vector indices

```rust
// ✅ CORRECT: Proper integration with existing systems
impl VectorSearchEngine {
    pub fn index_file(&self, path: &Path) -> Result<(), IndexError> {
        let symbols = self.extract_symbols(path)?;
        let changes = self.change_detector.detect_changes(&symbols)?;
        
        // Only re-embed actually changed symbols
        for changed in changes.modified {
            let embedding = self.generate_embedding(&changed)?;
            self.update_vector(changed.id, embedding)?;
        }
        
        Ok(())
    }
}
```

## 9. Development Workflow

### Progress Tracking
- **MUST** use TodoWrite tool for task tracking in complex features
- **MUST** update progress before moving to next task
- **SHOULD** break large tasks into trackable subtasks

### Quality Reviews
- **MUST** pass quality-reviewer agent checks before integration
- **MUST** address all "MUST FIX" issues before proceeding
- **SHOULD** explain any guideline violations with clear justification

## Enforcement

These guidelines are enforced through:
1. Automated clippy checks in CI
2. Quality reviewer agent validation
3. Code review requirements
4. Performance benchmarks that must pass

Violations of **MUST** rules block merging. Violations of **SHOULD** rules require justification.