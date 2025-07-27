# Code Guidelines

This document provides a strict and consolidated set of Rust development guidelines for this project, derived from a review of common implementation errors. Adherence to these rules is **mandatory** to ensure code quality, performance, and maintainability. These rules supersede any conflicting guidelines in other documents.

## 1. Function Signatures: Zero-Cost Abstractions are MANDATORY

This is the most critical principle. Violations require immediate fixing. The goal is to maximize caller flexibility and eliminate unnecessary memory allocations.

-   **Parameters**: For read-only data, **MUST** use borrowed types: `&str` over `String`, and `&[T]` over `&Vec<T>`.
-   **Return Values**:
    -   Prefer returning iterators (`impl Iterator`) over collecting into a `Vec`.
    -   Use `impl Trait` instead of heap-allocated trait objects (`Box<dyn Trait>`).
-   **Ownership Rule**:
    -   Take **owned types** (`String`, `Vec<T>`) **only** when you need to store or transform the data.
    -   Take **borrowed types** (`&str`, `&[T]`) when you only need to read or process the data.

```rust
// ✅ CORRECT: Flexible, zero-allocation for caller
fn process_data(data: &[u8]) -> impl Iterator<Item = &u8> { ... }

// ❌ INCORRECT: Forces ownership and unnecessary allocations
fn process_data(data: Vec<u8>) -> Vec<u8> { ... }
```

## 2. Type Safety: Enforce Strict Domain Modeling

-   **NO PRIMITIVE OBSESSION**: **MUST** create newtype wrappers for domain-specific concepts. Do not use raw primitives (`u32`, `String`, `PathBuf`) for IDs, file paths, or other special values. This prevents logic errors and makes the code self-documenting.

    ```rust
    // ✅ CORRECT: Type-safe, invalid states are impossible
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct VectorId(std::num::NonZeroU32);

    // ❌ INCORRECT: Prone to error, mixes different kinds of IDs
    let vector_id: u32 = 5;
    let cluster_id: u32 = 5; // Are these the same?
    ```

-   **Make Invalid States Unrepresentable**: Use the type system (e.g., `NonZeroU32`, enums) to prevent invalid data from ever being created.

## 3. Error Handling: Be Structured and Actionable

-   **Use `thiserror`**: All library code **MUST** use `thiserror` to create specific, structured error enums. `anyhow` is only acceptable at the top-level of binary application code.
-   **Actionable Suggestions**: Every error variant's message **MUST** include a concrete "Suggestion:" for the user on how to fix it.

    ```rust
    #[derive(Error, Debug)]
    pub enum AccuracyTestError {
        #[error("Search returned no results for query: {0}\nSuggestion: Check if test fixtures are properly indexed or try broader search terms.")]
        NoResults(String),

        #[error("Vector dimension mismatch: expected {expected}, got {actual}\nSuggestion: Ensure the query embedding model matches the document embedding model.")]
        DimensionMismatch { expected: usize, actual: usize },
    }
    ```

-   **No Panics**: Never `panic!`. Use `Result<T, E>`. Use `expect()` only for states that are truly impossible at runtime.

## 4. Code & API Structure: Be Idiomatic and Ergonomic

-   **Decompose Functions**: Keep functions small and focused on a single responsibility. A function that is longer than 20-30 lines or has more than 2 levels of nesting is a candidate for refactoring.
-   **Builder Pattern**: **MUST** use the Builder pattern for any struct with 3 or more fields in its constructor.
-   **Derive Standard Traits**:
    -   **MUST** derive `Debug` on ALL public types. No exceptions.
    -   Implement `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash` wherever it is logical to do so.
-   **Use `#[must_use]`**: **MUST** add `#[must_use]` to any function returning a `Result` or a value that would cause a logic error if ignored (e.g., validation functions, transaction commits).
-   **Obey Clippy**: Fix all clippy warnings (`cargo clippy -- -W clippy::all`). Do not ignore them.

```