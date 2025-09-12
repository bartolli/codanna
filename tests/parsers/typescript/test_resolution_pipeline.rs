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
fn test_typescript_resolution_pipeline() {
    // Step 1: Load settings (which now has tsconfig paths configured)
    let settings = Settings::load().expect("Failed to load settings");

    // Verify settings loaded our tsconfig paths
    let ts_config = settings
        .languages
        .get("typescript")
        .expect("TypeScript should be configured");
    assert_eq!(ts_config.config_files.len(), 3);
    assert!(
        ts_config.config_files[0]
            .to_str()
            .unwrap()
            .contains("examples/typescript/tsconfig.json")
    );
    assert!(
        ts_config.config_files[1]
            .to_str()
            .unwrap()
            .contains("packages/web/tsconfig.json")
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
    // Get rules for the root tsconfig
    let root_rules = index
        .rules
        .values()
        .find(|r| r.base_url.as_ref().is_some_and(|b| b.contains("./src")))
        .expect("Should find root tsconfig rules");

    // Create enhancer with the rules
    let enhancer = TypeScriptProjectEnhancer::new(root_rules.clone());
    let file_id = FileId::new(1).unwrap();

    // Test path alias resolution from root config
    // Note: baseUrl "./src" is prepended to the resolved paths
    assert_eq!(
        enhancer.enhance_import_path("@components/Button", file_id),
        Some("./src/components/Button".to_string()),
        "Root config: @components/* should resolve with baseUrl"
    );

    assert_eq!(
        enhancer.enhance_import_path("@utils/helpers", file_id),
        Some("./src/utils/helpers".to_string()),
        "Root config: @utils/* should resolve with baseUrl"
    );

    // Get rules for the web package tsconfig
    let web_rules = index
        .rules
        .values()
        .find(|r| r.paths.contains_key("@web/*"))
        .expect("Should find web package rules");

    let web_enhancer = TypeScriptProjectEnhancer::new(web_rules.clone());

    // Test path alias resolution from web config
    assert_eq!(
        web_enhancer.enhance_import_path("@web/components", file_id),
        Some("./src/web/components".to_string()),
        "Web config: @web/* should resolve with baseUrl"
    );

    assert_eq!(
        web_enhancer.enhance_import_path("@api/client", file_id),
        Some("./src/api/client".to_string()),
        "Web config: @api/* should resolve with baseUrl"
    );
}

#[test]
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
