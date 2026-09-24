//! Integration test for TypeScript resolution pipeline
//!
//! Tests the resolution enhancement during parsing WITHOUT indexing

use codanna::FileId;
use codanna::config::Settings;
use codanna::parsing::resolution::ProjectResolutionEnhancer;
use codanna::parsing::typescript::resolution::TypeScriptProjectEnhancer;
use codanna::project_resolver::persist::{ResolutionPersistence, ResolutionRules};
use codanna::project_resolver::providers::typescript::TypeScriptProvider;
use std::path::Path;

#[test]
#[ignore = "Requires local .codanna/settings.toml with TypeScript config_files and examples/typescript/ directory"]
fn test_typescript_resolution_pipeline() {
    // Step 1: Load settings (which now has tsconfig paths configured)
    let settings = Settings::load().expect("Failed to load settings");

    // Verify settings loaded our tsconfig paths
    let ts_config = settings
        .languages
        .get("typescript")
        .expect("TypeScript should be configured");

    println!(
        "Found {} TypeScript config files:",
        ts_config.config_files.len()
    );
    for (i, config_file) in ts_config.config_files.iter().enumerate() {
        println!("  [{}] {}", i, config_file.display());
    }

    assert!(
        !ts_config.config_files.is_empty(),
        "Should have at least one TypeScript config"
    );
    assert!(
        ts_config.config_files[0]
            .to_str()
            .unwrap()
            .contains("tsconfig.json"),
        "First config should be a tsconfig.json file"
    );

    // Step 2: Create provider
    let provider = TypeScriptProvider::new();

    // Step 3: Process and persist the resolution rules
    use codanna::project_resolver::provider::ProjectResolutionProvider;
    provider
        .rebuild_cache(&settings)
        .expect("Failed to process TypeScript configs");

    // Step 4: Load the persisted rules
    let persistence = ResolutionPersistence::new(Path::new(".codanna"));
    let index = persistence
        .load("typescript")
        .expect("Resolution index should be persisted");

    // Verify we have rules for both configs
    assert!(!index.rules.is_empty(), "Should have resolution rules");

    // Step 5: Test resolution enhancement with the rules
    // Get the first (and possibly only) tsconfig rules
    let root_rules = index
        .rules
        .values()
        .next()
        .expect("Should find at least one tsconfig rules");

    // Debug: Print the loaded rules
    println!("\n=== ROOT TSCONFIG RULES ===");
    println!("BaseUrl: {:?}", root_rules.base_url);
    println!("Paths:");
    for (alias, targets) in &root_rules.paths {
        println!("  {alias} -> {targets:?}");
    }

    // Create enhancer with the rules
    let enhancer = TypeScriptProjectEnhancer::new(root_rules.clone());
    let file_id = FileId::new(1).unwrap();

    // Test path alias resolution from root config
    // Note: baseUrl "./src" is prepended to the resolved paths

    println!("\n=== TESTING ALIAS RESOLUTION ===");

    // Test @/components/* pattern (from react tsconfig)
    let test_import = "@/components/Button";
    let result = enhancer.enhance_import_path(test_import, file_id);
    println!("Input: '{test_import}' -> Output: {result:?}");
    assert_eq!(
        result,
        Some("./src/components/Button".to_string()),
        "@/components/* should resolve to ./src/components/*"
    );

    // Test @/utils/* pattern (from react tsconfig)
    let test_import = "@/utils/helpers";
    let result = enhancer.enhance_import_path(test_import, file_id);
    println!("Input: '{test_import}' -> Output: {result:?}");
    assert_eq!(
        result,
        Some("./src/utils/helpers".to_string()),
        "@/utils/* should resolve to ./src/utils/*"
    );

    // Test @/* catch-all pattern (from react tsconfig)
    let test_import = "@/lib/api";
    let result = enhancer.enhance_import_path(test_import, file_id);
    println!("Input: '{test_import}' -> Output: {result:?}");
    assert_eq!(
        result,
        Some("./src/lib/api".to_string()),
        "@/* should resolve to ./src/*"
    );

    // Skip web package tests if we only have one config
    if index.rules.len() > 1 {
        // Get rules for the web package tsconfig
        if let Some(web_rules) = index
            .rules
            .values()
            .find(|r| r.paths.contains_key("@web/*"))
        {
            println!("\n=== WEB TSCONFIG RULES ===");
            println!("BaseUrl: {:?}", web_rules.base_url);
            println!("Paths:");
            for (alias, targets) in &web_rules.paths {
                println!("  {alias} -> {targets:?}");
            }

            let web_enhancer = TypeScriptProjectEnhancer::new(web_rules.clone());

            println!("\n=== TESTING WEB ALIAS RESOLUTION ===");

            // Test path alias resolution from web config
            let test_import = "@web/components";
            let result = web_enhancer.enhance_import_path(test_import, file_id);
            println!("Input: '{test_import}' -> Output: {result:?}");
            assert_eq!(
                result,
                Some("./src/web/components".to_string()),
                "Web config: @web/* should resolve with baseUrl"
            );

            let test_import = "@api/client";
            let result = web_enhancer.enhance_import_path(test_import, file_id);
            println!("Input: '{test_import}' -> Output: {result:?}");
            assert_eq!(
                result,
                Some("./src/api/client".to_string()),
                "Web config: @api/* should resolve with baseUrl"
            );
        }
    } else {
        println!("\n=== SKIPPING WEB TESTS (only one config file) ===");
    }
}

#[test]
#[ignore = "Requires local .codanna/settings.toml with TypeScript config_files and examples/typescript/ directory"]
fn test_typescript_extends_chain() {
    // Test that the extends chain is properly resolved
    let settings = Settings::load().expect("Failed to load settings");

    let provider = TypeScriptProvider::new();

    use codanna::project_resolver::provider::ProjectResolutionProvider;
    provider
        .rebuild_cache(&settings)
        .expect("Failed to process configs");

    let persistence = ResolutionPersistence::new(Path::new(".codanna"));
    let index = persistence.load("typescript").expect("Should load index");

    // Skip this test if we only have one config
    if index.rules.len() <= 1 {
        println!("Skipping extends chain test - only one config file found");
        return;
    }

    // Find the web package rules (which extends root)
    let web_rules = index
        .rules
        .values()
        .find(|r| r.paths.contains_key("@web/*"))
        .expect("Should find web rules");

    // Web config should have BOTH its own paths AND inherited paths from root
    // Child paths: @web/*, @api/*
    assert!(web_rules.paths.contains_key("@web/*"));
    assert!(web_rules.paths.contains_key("@api/*"));

    // Parent paths should also be inherited: @components/*, @utils/*, @types/*
    assert!(
        web_rules.paths.contains_key("@components/*"),
        "Should inherit @components/* from parent"
    );
    assert!(
        web_rules.paths.contains_key("@utils/*"),
        "Should inherit @utils/* from parent"
    );

    // BaseUrl should be from child (./src), not parent
    assert_eq!(
        web_rules.base_url,
        Some("./src".to_string()),
        "Child baseUrl should override parent"
    );
}

#[test]
fn test_resolution_without_config() {
    // Test that resolution works gracefully when no config exists
    let rules = ResolutionRules {
        base_url: None,
        paths: std::collections::HashMap::new(),

        relative_specifiers_redirected: false,
    };

    let enhancer = TypeScriptProjectEnhancer::new(rules);
    let file_id = FileId::new(1).unwrap();

    // Should return None for any alias when no rules
    assert_eq!(
        enhancer.enhance_import_path("@anything/file", file_id),
        None,
        "Should not enhance without rules"
    );

    // Relative imports should still return None (no enhancement needed)
    assert_eq!(
        enhancer.enhance_import_path("./local", file_id),
        None,
        "Relative imports not enhanced"
    );
}
