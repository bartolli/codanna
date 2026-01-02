//! Output management for CLI commands.
//!
//! Handles formatting and display for different output formats,
//! providing a unified interface for text and JSON output.

use crate::error::IndexError;
use crate::io::exit_code::ExitCode;
use crate::io::format::{JsonResponse, OutputFormat};
use crate::io::schema::{OutputData, OutputStatus, UnifiedOutput};
use serde::Serialize;
use std::fmt::Display;
use std::io::{self, Write};

/// Manages output formatting and display.
///
/// Provides methods for outputting success results, collections,
/// and errors in either text or JSON format based on configuration.
pub struct OutputManager {
    format: OutputFormat,
    stdout: Box<dyn Write>,
    stderr: Box<dyn Write>,
}

impl OutputManager {
    /// Create a new output manager with the specified format.
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            stdout: Box::new(io::stdout()),
            stderr: Box::new(io::stderr()),
        }
    }

    /// Helper to write to a stream, ignoring broken pipe errors.
    ///
    /// Broken pipes occur when the reader closes before we finish writing
    /// (e.g., when piping to `head`). This is normal behavior and should not
    /// be treated as an error. The exit code should reflect the operation's
    /// success, not the pipe status.
    fn write_ignoring_broken_pipe(stream: &mut dyn Write, content: &str) -> io::Result<()> {
        if let Err(e) = writeln!(stream, "{content}") {
            // Only propagate non-broken-pipe errors
            if e.kind() != io::ErrorKind::BrokenPipe {
                return Err(e);
            }
            // Silently ignore broken pipe - this is expected when piping to head, grep, etc.
        }
        Ok(())
    }

    /// Create an output manager for testing with custom writers.
    #[doc(hidden)]
    pub fn new_with_writers(
        format: OutputFormat,
        stdout: Box<dyn Write>,
        stderr: Box<dyn Write>,
    ) -> Self {
        Self {
            format,
            stdout,
            stderr,
        }
    }

    /// Output a successful result.
    ///
    /// In JSON mode, wraps the data in a success response.
    /// In text mode, displays the data using its Display implementation.
    /// Broken pipe errors are silently ignored to support piping to commands like `head`.
    pub fn success<T>(&mut self, data: T) -> io::Result<ExitCode>
    where
        T: Serialize + Display,
    {
        match self.format {
            OutputFormat::Json => {
                let response = JsonResponse::success(&data);
                let json_str = serde_json::to_string_pretty(&response)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &json_str)?;
            }
            OutputFormat::Text => {
                let text = format!("{data}");
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &text)?;
            }
        }
        Ok(ExitCode::Success)
    }

    /// Output a single item or indicate not found.
    ///
    /// If the item is Some, outputs it as success.
    /// If None, outputs a not found message.
    pub fn item<T>(&mut self, item: Option<T>, entity: &str, name: &str) -> io::Result<ExitCode>
    where
        T: Serialize + Display,
    {
        match item {
            Some(data) => self.success(data),
            None => self.not_found(entity, name),
        }
    }

    /// Output a not found result.
    /// Returns ExitCode::NotFound (3) to indicate the entity was not found.
    /// Broken pipe errors are silently ignored.
    pub fn not_found(&mut self, entity: &str, name: &str) -> io::Result<ExitCode> {
        match self.format {
            OutputFormat::Json => {
                let response = JsonResponse::not_found(entity, name);
                let json_str = serde_json::to_string_pretty(&response)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &json_str)?;
            }
            OutputFormat::Text => {
                let text = format!("{entity} '{name}' not found");
                Self::write_ignoring_broken_pipe(&mut *self.stderr, &text)?;
            }
        }
        Ok(ExitCode::NotFound)
    }

    /// Output a collection with proper formatting.
    ///
    /// Empty collections are treated as not found (returns ExitCode::NotFound).
    /// Non-empty collections are displayed as a list (returns ExitCode::Success).
    /// Broken pipe errors are silently ignored.
    pub fn collection<T, I>(&mut self, items: I, entity_name: &str) -> io::Result<ExitCode>
    where
        T: Serialize + Display,
        I: IntoIterator<Item = T>,
    {
        let items: Vec<T> = items.into_iter().collect();

        if items.is_empty() {
            return self.not_found(entity_name, "any");
        }

        match self.format {
            OutputFormat::Json => {
                let response = JsonResponse::success(&items);
                let json_str = serde_json::to_string_pretty(&response)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &json_str)?;
            }
            OutputFormat::Text => {
                let header = format!("Found {} {entity_name}:", items.len());
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &header)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &"=".repeat(40))?;
                for item in items {
                    let item_str = format!("{item}");
                    Self::write_ignoring_broken_pipe(&mut *self.stdout, &item_str)?;
                }
            }
        }
        Ok(ExitCode::Success)
    }

    /// Output an error with suggestions.
    /// Returns the appropriate ExitCode based on the error type.
    /// Broken pipe errors are silently ignored.
    pub fn error(&mut self, error: &IndexError) -> io::Result<ExitCode> {
        match self.format {
            OutputFormat::Json => {
                let response = JsonResponse::from_error(error);
                let json_str = serde_json::to_string_pretty(&response)?;
                Self::write_ignoring_broken_pipe(&mut *self.stderr, &json_str)?;
            }
            OutputFormat::Text => {
                let error_msg = format!("Error: {error}");
                Self::write_ignoring_broken_pipe(&mut *self.stderr, &error_msg)?;
                for suggestion in error.recovery_suggestions() {
                    let suggestion_msg = format!("  Suggestion: {suggestion}");
                    Self::write_ignoring_broken_pipe(&mut *self.stderr, &suggestion_msg)?;
                }
            }
        }
        Ok(ExitCode::from_error(error))
    }

    /// Output progress information (text mode only).
    ///
    /// In JSON mode, progress messages are suppressed to avoid
    /// polluting the JSON output.
    /// Broken pipe errors are silently ignored.
    pub fn progress(&mut self, message: &str) -> io::Result<()> {
        if matches!(self.format, OutputFormat::Text) {
            Self::write_ignoring_broken_pipe(&mut *self.stderr, message)?;
        }
        Ok(())
    }

    /// Output informational message (text mode only).
    /// Broken pipe errors are silently ignored.
    pub fn info(&mut self, message: &str) -> io::Result<()> {
        if matches!(self.format, OutputFormat::Text) {
            Self::write_ignoring_broken_pipe(&mut *self.stdout, message)?;
        }
        Ok(())
    }

    /// Output a collection of SymbolContext items.
    ///
    /// This method is specifically designed for SymbolContext to ensure
    /// consistent formatting across all retrieve commands.
    ///
    /// # Returns
    /// - `ExitCode::Success` - When contexts are found and output successfully
    /// - `ExitCode::NotFound` - When the collection is empty
    ///
    /// # Performance
    /// Collects the iterator once to check for empty and get count.
    /// This is necessary for proper error handling and text formatting.
    pub fn symbol_contexts(
        &mut self,
        contexts: impl IntoIterator<Item = crate::symbol::context::SymbolContext>,
        entity_name: &str,
    ) -> io::Result<ExitCode> {
        let contexts: Vec<_> = contexts.into_iter().collect();

        if contexts.is_empty() {
            return self.not_found(entity_name, "any");
        }

        match self.format {
            OutputFormat::Json => {
                let response = JsonResponse::success(&contexts);
                let json_str = serde_json::to_string_pretty(&response)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &json_str)?;
            }
            OutputFormat::Text => {
                let header = format!("Found {} {}:", contexts.len(), entity_name);
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &header)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &"=".repeat(40))?;

                for context in contexts {
                    let formatted = format!("{context}");
                    Self::write_ignoring_broken_pipe(&mut *self.stdout, &formatted)?;
                }
            }
        }
        Ok(ExitCode::Success)
    }

    /// Output a UnifiedOutput structure with dynamic data handling.
    ///
    /// This method handles all OutputData variants appropriately:
    /// - Items: Simple list display
    /// - Grouped: Hierarchical display by category
    /// - Contextual: Rich nested structure with relationships
    /// - Ranked: Sorted display with scores
    /// - Single: Single item display
    /// - Empty: Not found handling
    ///
    /// # Performance
    /// Uses zero-cost abstractions from UnifiedOutput.
    /// Display formatting relies on the UnifiedOutput's Display implementation.
    ///
    /// # Returns
    /// The exit code from the UnifiedOutput structure
    pub fn unified<T>(&mut self, output: UnifiedOutput<'_, T>) -> io::Result<ExitCode>
    where
        T: Serialize + Display,
    {
        let exit_code = output.exit_code;

        match self.format {
            OutputFormat::Json => {
                // For JSON, serialize the UnifiedOutput directly
                // This preserves the structured data with metadata and guidance
                let json_str = serde_json::to_string_pretty(&output)?;
                Self::write_ignoring_broken_pipe(&mut *self.stdout, &json_str)?;
            }
            OutputFormat::Text => {
                // For text, check if we have special handling needs
                match (&output.data, &output.status) {
                    // Only show "not found" when status is NotFound (symbol doesn't exist)
                    (OutputData::Empty, OutputStatus::NotFound) => {
                        let entity = format!("{:?}", output.entity_type);
                        let msg = format!("{} not found", entity.to_lowercase());
                        Self::write_ignoring_broken_pipe(&mut *self.stderr, &msg)?;
                    }
                    (OutputData::Items { items }, OutputStatus::NotFound) if items.is_empty() => {
                        let entity = format!("{:?}", output.entity_type);
                        let msg = format!("{} not found", entity.to_lowercase());
                        Self::write_ignoring_broken_pipe(&mut *self.stderr, &msg)?;
                    }
                    // Empty with Success status: symbol exists but no results (handled by guidance)
                    (OutputData::Empty, OutputStatus::Success) => {
                        // Don't output "not found" - guidance will explain
                    }
                    (OutputData::Items { items }, OutputStatus::Success) if items.is_empty() => {
                        // Don't output "not found" - guidance will explain
                    }
                    _ => {
                        // For all other cases, use the Display implementation
                        let formatted = format!("{output}");
                        Self::write_ignoring_broken_pipe(&mut *self.stdout, &formatted)?;
                    }
                }

                // Add guidance message if present (text mode only, to stderr)
                if let Some(guidance) = &output.guidance {
                    Self::write_ignoring_broken_pipe(&mut *self.stderr, "")?;
                    Self::write_ignoring_broken_pipe(&mut *self.stderr, guidance)?;
                }
            }
        }

        Ok(exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A writer that always returns broken pipe error
    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe broken"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe broken"))
        }
    }

    #[test]
    fn test_output_manager_text_success() {
        let stdout = Vec::new();
        let stderr = Vec::new();

        let mut manager =
            OutputManager::new_with_writers(OutputFormat::Text, Box::new(stdout), Box::new(stderr));

        let code = manager.success("Test output").unwrap();
        assert_eq!(code, ExitCode::Success);
    }

    #[test]
    fn test_broken_pipe_returns_success_exit_code() {
        let mut manager = OutputManager::new_with_writers(
            OutputFormat::Json,
            Box::new(BrokenPipeWriter),
            Box::new(Vec::new()),
        );

        // Should return Success exit code even with broken pipe
        let code = manager.success("test data").unwrap();
        assert_eq!(code, ExitCode::Success);
    }

    #[test]
    fn test_broken_pipe_returns_not_found_exit_code() {
        let mut manager = OutputManager::new_with_writers(
            OutputFormat::Json,
            Box::new(BrokenPipeWriter),
            Box::new(Vec::new()),
        );

        // Should return NotFound exit code even with broken pipe
        let code = manager.not_found("Symbol", "missing").unwrap();
        assert_eq!(code, ExitCode::NotFound);
    }

    #[test]
    fn test_broken_pipe_in_text_mode() {
        let mut manager = OutputManager::new_with_writers(
            OutputFormat::Text,
            Box::new(BrokenPipeWriter),
            Box::new(Vec::new()),
        );

        // Should handle broken pipe gracefully in text mode
        let code = manager.success("test output").unwrap();
        assert_eq!(code, ExitCode::Success);
    }

    #[test]
    fn test_broken_pipe_stderr() {
        let mut manager = OutputManager::new_with_writers(
            OutputFormat::Text,
            Box::new(Vec::new()),
            Box::new(BrokenPipeWriter),
        );

        // progress writes to stderr
        let result = manager.progress("Processing...");
        assert!(result.is_ok());

        // not_found text mode writes to stderr
        let code = manager.not_found("Entity", "name").unwrap();
        assert_eq!(code, ExitCode::NotFound);
    }

    #[test]
    fn test_output_manager_json_success() {
        let stdout = Vec::new();
        let stderr = Vec::new();

        let mut manager =
            OutputManager::new_with_writers(OutputFormat::Json, Box::new(stdout), Box::new(stderr));

        #[derive(Serialize)]
        struct TestData {
            value: i32,
        }

        impl Display for TestData {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "TestData({})", self.value)
            }
        }

        let data = TestData { value: 42 };
        let code = manager.success(data).unwrap();
        assert_eq!(code, ExitCode::Success);
    }

    #[test]
    fn test_symbol_contexts_collection() {
        use crate::symbol::Symbol;
        use crate::symbol::context::{SymbolContext, SymbolRelationships};
        use crate::types::{FileId, Range, SymbolId, SymbolKind};

        // Helper to create test context
        fn create_context(id: u32, name: &str) -> SymbolContext {
            let symbol = Symbol::new(
                SymbolId::new(id).unwrap(),
                name,
                SymbolKind::Function,
                FileId::new(1).unwrap(),
                Range::new(10, 0, 20, 0),
            );

            SymbolContext {
                symbol,
                file_path: format!("src/{name}.rs:11"),
                relationships: SymbolRelationships::default(),
            }
        }

        // Test with multiple items
        let stdout = Vec::new();
        let stderr = Vec::new();
        let mut manager =
            OutputManager::new_with_writers(OutputFormat::Json, Box::new(stdout), Box::new(stderr));

        let contexts = vec![create_context(1, "main"), create_context(2, "process")];

        let code = manager.symbol_contexts(contexts, "functions").unwrap();
        assert_eq!(code, ExitCode::Success);
    }

    #[test]
    fn test_symbol_contexts_empty() {
        use crate::symbol::context::SymbolContext;

        let stdout = Vec::new();
        let stderr = Vec::new();
        let mut manager =
            OutputManager::new_with_writers(OutputFormat::Json, Box::new(stdout), Box::new(stderr));

        let contexts: Vec<SymbolContext> = vec![];
        let code = manager.symbol_contexts(contexts, "symbols").unwrap();
        assert_eq!(code, ExitCode::NotFound);
    }

    #[test]
    fn test_symbol_contexts_text_format() {
        use crate::symbol::Symbol;
        use crate::symbol::context::{SymbolContext, SymbolRelationships};
        use crate::types::{FileId, Range, SymbolId, SymbolKind};

        let symbol = Symbol::new(
            SymbolId::new(1).unwrap(),
            "test_function",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(42, 0, 50, 0),
        );

        let context = SymbolContext {
            symbol,
            file_path: "src/test.rs:43".to_string(),
            relationships: SymbolRelationships::default(),
        };

        let stdout = Vec::new();
        let stderr = Vec::new();
        let mut manager =
            OutputManager::new_with_writers(OutputFormat::Text, Box::new(stdout), Box::new(stderr));

        let code = manager.symbol_contexts(vec![context], "function").unwrap();
        assert_eq!(code, ExitCode::Success);

        // Since we can't extract the output easily from a Box<dyn Write>,
        // we'll trust that if the method runs without error and returns Success,
        // it's working correctly. More detailed verification would require
        // a different test approach.
    }

    #[test]
    fn test_symbol_contexts_broken_pipe() {
        use crate::symbol::Symbol;
        use crate::symbol::context::{SymbolContext, SymbolRelationships};
        use crate::types::{FileId, Range, SymbolId, SymbolKind};

        let symbol = Symbol::new(
            SymbolId::new(1).unwrap(),
            "test",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(1, 0, 2, 0),
        );

        let context = SymbolContext {
            symbol,
            file_path: "test.rs:1".to_string(),
            relationships: SymbolRelationships::default(),
        };

        // Test with broken pipe on stdout
        let mut manager = OutputManager::new_with_writers(
            OutputFormat::Json,
            Box::new(BrokenPipeWriter),
            Box::new(Vec::new()),
        );

        // Should succeed despite broken pipe
        let code = manager
            .symbol_contexts(vec![context.clone()], "symbols")
            .unwrap();
        assert_eq!(code, ExitCode::Success);

        // Test empty collection with broken pipe
        let code = manager
            .symbol_contexts(Vec::<SymbolContext>::new(), "symbols")
            .unwrap();
        assert_eq!(code, ExitCode::NotFound);
    }
}
