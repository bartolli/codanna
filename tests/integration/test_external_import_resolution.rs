/// Integration tests for external import detection
///
/// These tests verify that ResolutionContext correctly identifies external imports
/// and prevents them from resolving to local symbols with the same name.
///
/// NOTE: These tests do NOT use Tantivy (to avoid lock conflicts).
/// They test the resolution layer logic only.
use codanna::parsing::resolution::{ImportBinding, ImportOrigin, ResolutionScope};
use codanna::parsing::rust::resolution::RustResolutionContext;
use codanna::parsing::{Import, ScopeLevel};
use codanna::{FileId, SymbolId};

fn register_binding(
    context: &mut RustResolutionContext,
    import: &Import,
    exposed_name: &str,
    origin: ImportOrigin,
    resolved: Option<SymbolId>,
) {
    context.register_import_binding(ImportBinding {
        import: import.clone(),
        exposed_name: exposed_name.to_string(),
        origin,
        resolved_symbol: resolved,
    });
}

#[test]
fn test_external_import_detection_prevents_local_resolution() {
    println!("\n=== Test: External Import Detection Prevents Local Resolution ===");

    // Scenario: File has external import AND local symbol with same name
    // use indicatif::ProgressBar;  // External
    // struct ProgressBar { ... }    // Local

    let file_id = FileId::new(1).unwrap();
    let mut context = RustResolutionContext::new(file_id);

    // Step 1: Add external import metadata
    let external_import = Import {
        path: "indicatif::ProgressBar".to_string(),
        name: None,
        alias: None,
        file_id,
        is_glob: false,
        is_type_only: false,
    };

    println!("\n1. Populating external import: {}", external_import.path);
    context.populate_imports(std::slice::from_ref(&external_import));
    register_binding(
        &mut context,
        &external_import,
        "ProgressBar",
        ImportOrigin::External,
        None,
    );
    register_binding(
        &mut context,
        &external_import,
        "indicatif::ProgressBar",
        ImportOrigin::External,
        None,
    );

    // Step 2: Add local symbol with same name
    let local_symbol_id = SymbolId::new(100).unwrap();
    println!("2. Adding local symbol 'ProgressBar' with id {local_symbol_id:?}");
    context.add_symbol(
        "ProgressBar".to_string(),
        local_symbol_id,
        ScopeLevel::Module,
    );

    // Step 3: Check external import detection
    println!("\n3. Checking is_external_import('ProgressBar')");
    let is_external = context.is_external_import("ProgressBar");
    println!("   Result: {is_external}");

    // CRITICAL: Should detect as external
    assert!(
        is_external,
        "ProgressBar should be detected as external import"
    );

    // Step 4: Verify resolution still works for local context
    println!("\n4. Testing resolution (should return local symbol)");
    let resolved = context.resolve("ProgressBar");
    println!("   Resolved to: {resolved:?}");

    // The resolution returns the local symbol, but the CALLER should check
    // is_external_import() BEFORE using the resolved symbol
    assert_eq!(
        resolved,
        Some(local_symbol_id),
        "Resolution returns local symbol (caller must check is_external_import)"
    );

    println!("\n✅ Test passed: is_external_import() correctly identifies external symbol");
    println!("   Caller code should skip resolution when is_external_import() returns true");
}

#[test]
fn test_internal_import_not_flagged_as_external() {
    println!("\n=== Test: Internal Import Not Flagged as External ===");

    let file_id = FileId::new(1).unwrap();
    let mut context = RustResolutionContext::new(file_id);

    // Internal import (crate::utils::helper)
    let internal_import = Import {
        path: "crate::utils::helper".to_string(),
        name: None,
        alias: None,
        file_id,
        is_glob: false,
        is_type_only: false,
    };

    println!("1. Populating internal import: {}", internal_import.path);
    context.populate_imports(std::slice::from_ref(&internal_import));

    // Add the internal symbol (this mimics what build_resolution_context does)
    let internal_symbol_id = SymbolId::new(200).unwrap();
    context.add_symbol("helper".to_string(), internal_symbol_id, ScopeLevel::Module);

    register_binding(
        &mut context,
        &internal_import,
        "helper",
        ImportOrigin::Internal,
        Some(internal_symbol_id),
    );
    register_binding(
        &mut context,
        &internal_import,
        "crate::utils::helper",
        ImportOrigin::Internal,
        Some(internal_symbol_id),
    );

    // Check it's NOT external
    println!("\n2. Checking is_external_import('helper')");
    let is_external = context.is_external_import("helper");
    println!("   Result: {is_external}");

    assert!(
        !is_external,
        "Internal symbols should NOT be flagged as external"
    );

    // Should resolve normally
    let resolved = context.resolve("helper");
    assert_eq!(
        resolved,
        Some(internal_symbol_id),
        "Internal symbols should resolve normally"
    );

    println!("\n✅ Test passed: Internal imports correctly distinguished from external");
}

#[test]
fn test_aliased_external_import_detection() {
    println!("\n=== Test: Aliased External Import Detection ===");

    let file_id = FileId::new(1).unwrap();
    let mut context = RustResolutionContext::new(file_id);

    // use indicatif::ProgressBar as PBar;
    let aliased_import = Import {
        path: "indicatif::ProgressBar".to_string(),
        name: None,
        alias: Some("PBar".to_string()),
        file_id,
        is_glob: false,
        is_type_only: false,
    };

    println!(
        "1. Populating aliased import: {} as PBar",
        aliased_import.path
    );
    context.populate_imports(std::slice::from_ref(&aliased_import));
    register_binding(
        &mut context,
        &aliased_import,
        "PBar",
        ImportOrigin::External,
        None,
    );
    register_binding(
        &mut context,
        &aliased_import,
        "ProgressBar",
        ImportOrigin::External,
        None,
    );
    register_binding(
        &mut context,
        &aliased_import,
        "indicatif::ProgressBar",
        ImportOrigin::External,
        None,
    );

    // Check alias is detected as external
    println!("\n2. Checking is_external_import('PBar')");
    let is_external_alias = context.is_external_import("PBar");
    println!("   Result: {is_external_alias}");

    assert!(
        is_external_alias,
        "Aliased external imports should be detected by alias"
    );

    // Original name should also be detected
    println!("\n3. Checking is_external_import('ProgressBar')");
    let is_external_original = context.is_external_import("ProgressBar");
    println!("   Result: {is_external_original}");

    assert!(
        is_external_original,
        "Aliased external imports should be detected by original name too"
    );

    println!("\n✅ Test passed: Aliased external imports correctly detected");
}

#[test]
fn test_multiple_external_imports() {
    println!("\n=== Test: Multiple External Imports ===");

    let file_id = FileId::new(1).unwrap();
    let mut context = RustResolutionContext::new(file_id);

    // Multiple external imports
    let imports = vec![
        Import {
            path: "indicatif::ProgressBar".to_string(),
            name: None,
            alias: None,
            file_id,
            is_glob: false,
            is_type_only: false,
        },
        Import {
            path: "serde::Serialize".to_string(),
            name: None,
            alias: None,
            file_id,
            is_glob: false,
            is_type_only: false,
        },
        Import {
            path: "tokio::sync::Mutex".to_string(),
            name: None,
            alias: None,
            file_id,
            is_glob: false,
            is_type_only: false,
        },
    ];

    println!("1. Populating {} external imports", imports.len());
    context.populate_imports(&imports);
    for import in &imports {
        // Register both the short name and full path
        let short_name = import
            .path
            .rsplit("::")
            .next()
            .unwrap_or(&import.path)
            .to_string();
        register_binding(
            &mut context,
            import,
            &short_name,
            ImportOrigin::External,
            None,
        );
        register_binding(
            &mut context,
            import,
            &import.path,
            ImportOrigin::External,
            None,
        );
    }

    // Check all are detected
    let external_symbols = ["ProgressBar", "Serialize", "Mutex"];
    for symbol_name in &external_symbols {
        println!("\n2. Checking is_external_import('{symbol_name}')");
        let is_external = context.is_external_import(symbol_name);
        println!("   Result: {is_external}");
        assert!(is_external, "{symbol_name} should be detected as external");
    }

    // Check a non-imported symbol is NOT external
    println!("\n3. Checking is_external_import('LocalStruct')");
    let is_local_external = context.is_external_import("LocalStruct");
    println!("   Result: {is_local_external}");
    assert!(
        !is_local_external,
        "Non-imported symbols should not be external"
    );

    println!("\n✅ Test passed: Multiple external imports handled correctly");
}

#[test]
fn test_external_import_same_name_as_local_symbol() {
    println!("\n=== Test: External Import with Same Name as Local Symbol ===");
    println!("This is the MAIN BUG we're fixing!");

    let file_id = FileId::new(1).unwrap();
    let mut context = RustResolutionContext::new(file_id);

    // The problematic scenario:
    // use indicatif::ProgressBar;  // External
    //
    // pub fn create_progress() -> ProgressBar {
    //     ProgressBar::new(100)  // Should NOT resolve
    // }
    //
    // struct ProgressBar;  // Local symbol with SAME NAME
    // impl ProgressBar {
    //     fn new() -> Self { Self }
    // }

    // Step 1: Add external import
    let external_import = Import {
        path: "indicatif::ProgressBar".to_string(),
        name: None,
        alias: None,
        file_id,
        is_glob: false,
        is_type_only: false,
    };

    println!("\n1. External import: {}", external_import.path);
    context.populate_imports(std::slice::from_ref(&external_import));
    register_binding(
        &mut context,
        &external_import,
        "ProgressBar",
        ImportOrigin::External,
        None,
    );
    register_binding(
        &mut context,
        &external_import,
        "indicatif::ProgressBar",
        ImportOrigin::External,
        None,
    );

    // Step 2: Add local ProgressBar struct
    let local_progressbar_id = SymbolId::new(100).unwrap();
    println!("2. Local symbol: ProgressBar (struct) id={local_progressbar_id:?}");
    context.add_symbol(
        "ProgressBar".to_string(),
        local_progressbar_id,
        ScopeLevel::Module,
    );

    // Step 3: Add local ProgressBar::new method
    let local_new_method_id = SymbolId::new(101).unwrap();
    println!("3. Local symbol: ProgressBar::new (method) id={local_new_method_id:?}");
    context.add_symbol("new".to_string(), local_new_method_id, ScopeLevel::Module);

    // Step 4: Simulate method call resolution
    println!("\n4. Simulating resolution of 'ProgressBar::new(100)'");

    // Check receiver (ProgressBar) is external
    let receiver_is_external = context.is_external_import("ProgressBar");
    println!("   is_external_import('ProgressBar') = {receiver_is_external}");

    // CRITICAL: Caller code should check this BEFORE resolving
    if receiver_is_external {
        println!("   ✅ CORRECT: Receiver is external, skip resolution");
        println!("   Should NOT create relationship to local ProgressBar::new");
    } else {
        println!("   ❌ BUG: Receiver not detected as external!");
        println!("   Would incorrectly resolve to local ProgressBar::new");
    }

    assert!(
        receiver_is_external,
        "CRITICAL: External import must be detected to prevent wrong resolution"
    );

    println!("\n✅ Test passed: External import detection prevents incorrect resolution");
    println!("   This is the key mechanism that fixes the bug!");
}

#[test]
fn test_no_imports_means_no_external_symbols() {
    println!("\n=== Test: No Imports Means No External Symbols ===");

    let file_id = FileId::new(1).unwrap();
    let context = RustResolutionContext::new(file_id);

    // No imports populated
    println!("1. Context has no imports");

    // Check that nothing is flagged as external
    let test_names = ["ProgressBar", "HashMap", "Vec", "String"];
    for name in &test_names {
        println!("\n2. Checking is_external_import('{name}')");
        let is_external = context.is_external_import(name);
        println!("   Result: {is_external}");
        assert!(!is_external, "Without imports, nothing should be external");
    }

    println!("\n✅ Test passed: Empty imports means no external symbols");
}
