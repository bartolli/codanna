//! Tests for I/O module functionality

#[cfg(test)]
mod tests {
    use crate::io::{ExitCode, JsonResponse, OutputFormat, OutputManager};
    use crate::symbol::Symbol;
    use crate::types::{FileId, Range, SymbolId, SymbolKind};

    #[test]
    fn test_symbol_json_output() {
        // Create a test symbol with realistic data
        let symbol = Symbol::new(
            SymbolId::new(42).unwrap(),
            "calculate_similarity",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(100, 4, 120, 5),
        )
        .with_signature("fn calculate_similarity(a: &[f32], b: &[f32]) -> f32")
        .with_doc("Calculate cosine similarity between two vectors");

        // Test JSON serialization via JsonResponse
        let response = JsonResponse::success(&symbol);
        let json_string = serde_json::to_string_pretty(&response).unwrap();

        // Verify the JSON contains expected fields
        assert!(json_string.contains(r#""status": "success""#));
        assert!(json_string.contains(r#""code": "OK""#));
        assert!(json_string.contains(r#""exit_code": 0"#));
        assert!(json_string.contains(r#""name": "calculate_similarity""#));
        assert!(json_string.contains(r#""kind": "Function""#));

        // Print the actual JSON for manual verification
        println!("JSON output for Symbol:");
        println!("{json_string}");
    }

    #[test]
    fn test_output_manager_simple() {
        // Create a simple test symbol
        let symbol = Symbol::new(
            SymbolId::new(1).unwrap(),
            "test_function",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(10, 0, 20, 0),
        );

        // Test JSON output
        let stdout = Vec::new();
        let stderr = Vec::new();
        let mut manager =
            OutputManager::new_with_writers(OutputFormat::Json, Box::new(stdout), Box::new(stderr));

        let exit_code = manager.success(&symbol).unwrap();
        assert_eq!(exit_code, ExitCode::Success);

        // We can't easily get the output back from the manager due to ownership,
        // but we've proven it compiles and runs without panic
        println!("OutputManager test completed successfully");
    }

    #[test]
    fn test_multiple_symbols_json() {
        // Create multiple symbols
        let symbols = vec![
            Symbol::new(
                SymbolId::new(1).unwrap(),
                "main",
                SymbolKind::Function,
                FileId::new(1).unwrap(),
                Range::new(10, 0, 15, 1),
            )
            .with_signature("fn main()"),
            Symbol::new(
                SymbolId::new(2).unwrap(),
                "Config",
                SymbolKind::Struct,
                FileId::new(1).unwrap(),
                Range::new(20, 0, 30, 1),
            ),
            Symbol::new(
                SymbolId::new(3).unwrap(),
                "parse",
                SymbolKind::Method,
                FileId::new(2).unwrap(),
                Range::new(40, 0, 50, 1),
            )
            .with_signature("fn parse(&self) -> Result<(), Error>"),
        ];

        // Test JSON serialization of a collection
        let response = JsonResponse::success(&symbols);
        let json = serde_json::to_string_pretty(&response).unwrap();

        println!("JSON output for multiple symbols:");
        println!("{json}");

        // Verify the JSON structure
        assert!(json.contains(r#""status": "success""#));
        assert!(json.contains(r#""main""#));
        assert!(json.contains(r#""Config""#));
        assert!(json.contains(r#""parse""#));
        assert!(json.contains(r#""Function""#));
        assert!(json.contains(r#""Struct""#));
        assert!(json.contains(r#""Method""#));
    }

    #[test]
    fn test_not_found_json() {
        let response = JsonResponse::not_found("Symbol", "undefined_function");
        let json = serde_json::to_string_pretty(&response).unwrap();

        println!("JSON output for not found:");
        println!("{json}");

        // Verify error response structure
        assert!(json.contains(r#""status": "error""#));
        assert!(json.contains(r#""code": "NOT_FOUND""#));
        assert!(json.contains(r#""exit_code": 3"#));
        assert!(json.contains("undefined_function"));
        assert!(json.contains("suggestions"));
    }
}
