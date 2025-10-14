use codanna::SimpleIndexer;
use codanna::config::Settings;
use std::sync::Arc;
use tempfile::TempDir;

/// Test that object property names matching function names are correctly tracked
///
/// Bug: When an object property name matches a function name and calls that function,
/// the relationship tracking fails. For example:
///
/// ```typescript
/// function submitForm(data: any) { ... }
///
/// const actions = {
///     submitForm: async (request: any) => {
///         return submitForm({ input: request.body });
///     }
/// };
/// ```
///
/// In this case, the anonymous function assigned to `actions.submitForm` should be
/// tracked as calling the `submitForm` function, but the relationship is not being recorded.
#[test]
fn test_object_property_calls_same_named_function() {
    println!("\n=== Testing Object Property Calling Same-Named Function ===");

    let code = r#"// Bug reproduction: object property name matches function name
// codanna should detect that actions.submitForm calls submitForm()

function submitForm(data: any) {
    return { success: true, data };
}

const actions = {
    // Property name 'submitForm' matches function name
    // This call should be detected as a caller
    submitForm: async (request: any) => {
        return submitForm({ input: request.body });
    }
};
"#;

    // Create a temporary directory and file
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test_action_caller.ts");
    std::fs::write(&test_file, code).unwrap();

    // Initialize indexer with custom settings pointing to temp directory
    let settings = Settings {
        workspace_root: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };
    let mut indexer = SimpleIndexer::with_settings(Arc::new(settings));

    // Index the file
    println!("\nIndexing file: {test_file:?}");
    indexer
        .index_file(test_file.to_str().unwrap())
        .expect("Failed to index file");

    println!("\n=== Verifying Symbol Detection ===");

    // Find the submitForm function
    let submit_form_symbols = indexer.find_symbols_by_name("submitForm", Some("typescript"));
    println!("Found {} 'submitForm' symbols", submit_form_symbols.len());
    assert!(
        !submit_form_symbols.is_empty(),
        "submitForm function should be indexed"
    );

    let submit_form = &submit_form_symbols[0];
    println!(
        "  - submitForm: kind={:?}, line={}, id={:?}",
        submit_form.kind, submit_form.range.start_line, submit_form.id
    );

    // Find the actions constant
    let actions_symbols = indexer.find_symbols_by_name("actions", Some("typescript"));
    println!("\nFound {} 'actions' symbols", actions_symbols.len());
    assert!(
        !actions_symbols.is_empty(),
        "actions constant should be indexed"
    );

    let actions = &actions_symbols[0];
    println!(
        "  - actions: kind={:?}, line={}, id={:?}",
        actions.kind, actions.range.start_line, actions.id
    );

    println!("\n=== Testing Relationship Tracking ===");

    // Check if the property method is tracked as calling submitForm
    // Note: The property method might be indexed as a separate symbol or as part of the actions object
    let callers = indexer.get_calling_functions_with_metadata(submit_form.id);
    println!("\nCallers of submitForm: {}", callers.len());
    for (caller, metadata) in &callers {
        println!(
            "  - {}: kind={:?}, line={}, metadata={:?}",
            caller.name, caller.kind, caller.range.start_line, metadata
        );
    }

    // The bug: This assertion will fail because the relationship is not tracked
    assert!(
        !callers.is_empty(),
        "BUG: submitForm should have at least one caller (the property method in actions object)"
    );

    // Verify the caller is from the actions object (line 11 in the test code)
    let caller = &callers[0].0;
    println!("\nVerifying caller details:");
    println!("  - Caller name: {}", caller.name);
    println!("  - Caller line: {}", caller.range.start_line);

    // The property method should be on line 11 (0-indexed: line 10)
    // or be associated with the actions object on line 8
    assert!(
        caller.range.start_line >= 7 && caller.range.start_line <= 11,
        "Caller should be within the actions object definition (lines 8-12)"
    );
}

/// Test a simpler case: regular function calling another function
/// This should work correctly and serves as a baseline comparison
#[test]
fn test_regular_function_call_works() {
    println!("\n=== Testing Regular Function Call (Baseline) ===");

    let code = r#"
function submitForm(data: any) {
    return { success: true, data };
}

function handleRequest(request: any) {
    return submitForm({ input: request.body });
}
"#;

    // Create a temporary directory and file
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test_regular_call.ts");
    std::fs::write(&test_file, code).unwrap();

    // Initialize indexer with custom settings pointing to temp directory
    let settings = Settings {
        workspace_root: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };
    let mut indexer = SimpleIndexer::with_settings(Arc::new(settings));

    // Index the file
    indexer
        .index_file(test_file.to_str().unwrap())
        .expect("Failed to index file");

    // Find the submitForm function
    let submit_form_symbols = indexer.find_symbols_by_name("submitForm", Some("typescript"));
    assert!(
        !submit_form_symbols.is_empty(),
        "submitForm function should be indexed"
    );

    let submit_form = &submit_form_symbols[0];

    // Check callers
    let callers = indexer.get_calling_functions_with_metadata(submit_form.id);
    println!("Callers of submitForm: {}", callers.len());
    for (caller, metadata) in &callers {
        println!(
            "  - {}: kind={:?}, metadata={:?}",
            caller.name, caller.kind, metadata
        );
    }

    // This should work correctly
    assert_eq!(
        callers.len(),
        1,
        "submitForm should have exactly one caller (handleRequest)"
    );
    assert_eq!(callers[0].0.name.as_ref(), "handleRequest");
}
