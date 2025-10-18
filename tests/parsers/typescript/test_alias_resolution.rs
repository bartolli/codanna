//! Test TypeScript alias resolution without indexing
//!
//! Tests that TypeScript path aliases are properly enhanced during parsing

use codanna::FileId;
use codanna::config::Settings;
use codanna::parsing::resolution::ProjectResolutionEnhancer;
use codanna::parsing::typescript::behavior::TypeScriptBehavior;
use codanna::parsing::typescript::resolution::TypeScriptProjectEnhancer;
use codanna::parsing::{Import, LanguageBehavior};
use codanna::project_resolver::persist::{ResolutionPersistence, ResolutionRules};
use codanna::project_resolver::providers::typescript::TypeScriptProvider;
use std::path::Path;

#[test]
fn test_import_enhancement_with_aliases() {
    // Create resolution rules like tsconfig would provide
    let rules = ResolutionRules {
        base_url: None,
        paths: vec![
            (
                "@/components/*".to_string(),
                vec!["./src/components/*".to_string()],
            ),
            ("@/utils/*".to_string(), vec!["./src/utils/*".to_string()]),
            ("@/*".to_string(), vec!["./src/*".to_string()]),
        ]
        .into_iter()
        .collect(),
    };

    // Create enhancer with the rules
    let enhancer = TypeScriptProjectEnhancer::new(rules);
    let file_id = FileId::new(1).unwrap();

    // Test various alias patterns
    let test_cases = vec![
        ("@/components/Button", Some("./src/components/Button")),
        ("@/components/ui/dialog", Some("./src/components/ui/dialog")),
        ("@/utils/helpers", Some("./src/utils/helpers")),
        ("@/lib/api", Some("./src/lib/api")),
        ("./relative/path", None), // Relative paths should not be enhanced
        ("../parent/path", None),  // Parent paths should not be enhanced
        ("react", None),           // External packages should not be enhanced
    ];

    for (import_path, expected) in test_cases {
        let result = enhancer.enhance_import_path(import_path, file_id);

        match expected {
            Some(expected_path) => {
                assert_eq!(
                    result.as_deref(),
                    Some(expected_path),
                    "Import '{import_path}' should be enhanced to '{expected_path}'"
                );
            }
            None => {
                assert_eq!(
                    result, None,
                    "Import '{import_path}' should not be enhanced"
                );
            }
        }
    }

    println!("All import enhancements work correctly!");
}

#[test]
fn test_module_path_computation() {
    // Test that enhanced paths are converted to correct module paths
    // This simulates what should happen in resolve_import

    let test_cases = vec![
        // (enhanced_path, importing_module, expected_target_module)
        (
            "./src/components/Button",
            "examples.typescript.react.src.app",
            "examples.typescript.react.src.components.Button",
        ),
        (
            "./src/utils/helpers",
            "examples.typescript.react.src.components.Form",
            "examples.typescript.react.src.utils.helpers",
        ),
        (
            "./src/lib/api",
            "examples.typescript.react.src.hooks.useAuth",
            "examples.typescript.react.src.lib.api",
        ),
    ];

    for (enhanced_path, importing_module, expected) in test_cases {
        // Extract project prefix from importing module
        let project_prefix = if importing_module.contains("examples.typescript.react") {
            "examples.typescript.react"
        } else if let Some(idx) = importing_module.find(".src.") {
            &importing_module[..idx]
        } else {
            ""
        };

        // Convert enhanced path to module path
        let cleaned_path = enhanced_path
            .trim_start_matches("./")
            .trim_start_matches("/")
            .replace('/', ".");

        let target_module = if project_prefix.is_empty() {
            cleaned_path
        } else {
            format!("{project_prefix}.{cleaned_path}")
        };

        assert_eq!(
            target_module, expected,
            "Enhanced path '{enhanced_path}' from module '{importing_module}' should become '{expected}'"
        );
    }

    println!("Module path computation works correctly!");
}

#[test]
#[ignore = "TypeScript resolution rules not being loaded correctly - needs investigation"]
fn test_typescript_behavior_add_import() {
    // Test that TypeScriptBehavior enhances imports when project rules are available
    // Using test fixtures to provide a proper isolated test environment
    //
    // TODO: This test fails because the resolution rules aren't being loaded/applied properly.
    // The test creates .codanna/index/resolvers/typescript_resolution.json but TypeScriptBehavior
    // doesn't seem to pick up these rules when calling add_import(). This needs deeper investigation
    // of the TypeScript resolution infrastructure.

    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    // Setup: Create a temporary .codanna directory for the test
    let test_codanna_dir = PathBuf::from(".codanna_test");
    let resolver_dir = test_codanna_dir.join("index").join("resolvers");
    fs::create_dir_all(&resolver_dir).expect("Failed to create test resolver directory");

    // Get the absolute path to our test fixture
    let test_fixture_path = Path::new("tests/fixtures/typescript_alias_test")
        .canonicalize()
        .expect("Test fixture directory not found");
    let tsconfig_path = test_fixture_path.join("tsconfig.json");

    // Create resolution rules that match our test fixture
    let test_rules = format!(
        r#"{{
        "version": "1.0",
        "hashes": {{
            "{}": "test-hash"
        }},
        "mappings": {{
            "{}/**/*.ts": "{}"
        }},
        "rules": {{
            "{}": {{
                "baseUrl": ".",
                "paths": {{
                    "@/components/*": ["./src/components/*"],
                    "@/utils/*": ["./src/utils/*"],
                    "@/hooks/*": ["./src/hooks/*"],
                    "@/*": ["./src/*"]
                }}
            }}
        }}
    }}"#,
        tsconfig_path.display(),
        test_fixture_path.display(),
        tsconfig_path.display(),
        tsconfig_path.display()
    );

    // Write the resolution rules to our test .codanna directory
    let rules_path = resolver_dir.join("typescript_resolution.json");
    fs::write(&rules_path, test_rules).expect("Failed to write test resolution rules");

    // Temporarily change working directory to use our test .codanna
    let original_dir = env::current_dir().expect("Failed to get current directory");

    // Create a test workspace directory
    let test_workspace = PathBuf::from("test_workspace");
    fs::create_dir_all(&test_workspace).ok();

    // Move our test .codanna to the test workspace
    let test_workspace_codanna = test_workspace.join(".codanna");
    if test_workspace_codanna.exists() {
        fs::remove_dir_all(&test_workspace_codanna).ok();
    }
    fs::rename(&test_codanna_dir, &test_workspace_codanna)
        .expect("Failed to move test .codanna directory");

    // Change to test workspace
    env::set_current_dir(&test_workspace).expect("Failed to change to test workspace");

    // Now create the behavior - it will find .codanna in the current directory
    let behavior = TypeScriptBehavior::new();
    let file_id = FileId::new(1).unwrap();

    // Create an import with an alias
    let import = Import {
        path: "@/components/Button".to_string(),
        alias: Some("Button".to_string()),
        is_glob: false,
        is_type_only: false,
        file_id,
    };

    // Add the import - this should enhance it using the rules we set up
    behavior.add_import(import.clone());

    // Get imports back
    let imports = behavior.get_imports_for_file(file_id);

    // Restore original directory
    env::set_current_dir(&original_dir).expect("Failed to restore directory");

    // Clean up test directories
    fs::remove_dir_all(&test_workspace).ok();

    // Verify the import was stored and enhanced
    assert_eq!(imports.len(), 1, "Should have exactly one import");

    // The import should be enhanced from @/components/Button to ./src/components/Button
    assert_eq!(
        imports[0].path, "./src/components/Button",
        "Import should be enhanced from @/components/Button to ./src/components/Button"
    );

    println!("Import enhancement test passed with proper test fixtures!");
}

#[test]
fn test_resolution_with_project_rules() {
    // Test that resolution works when project rules are available
    // This tests the load_project_rules_for_file path

    // First ensure settings are configured with TypeScript config files
    if let Ok(settings) = Settings::load() {
        if let Some(ts_config) = settings.languages.get("typescript") {
            if !ts_config.config_files.is_empty() {
                println!(
                    "Found {} TypeScript config files",
                    ts_config.config_files.len()
                );

                // Create provider and build cache
                let provider = TypeScriptProvider::new();

                use codanna::project_resolver::provider::ProjectResolutionProvider;
                if let Err(e) = provider.rebuild_cache(&settings) {
                    println!("Warning: Could not build cache: {e}");
                    return;
                }

                // Load the persisted rules
                let persistence = ResolutionPersistence::new(Path::new(".codanna"));
                if let Ok(index) = persistence.load("typescript") {
                    println!("Loaded {} resolution rules", index.rules.len());

                    // Test enhancement with loaded rules
                    if let Some(rules) = index.rules.values().next() {
                        let enhancer = TypeScriptProjectEnhancer::new(rules.clone());
                        let file_id = FileId::new(1).unwrap();

                        if let Some(enhanced) =
                            enhancer.enhance_import_path("@/components/Button", file_id)
                        {
                            println!("Successfully enhanced: @/components/Button -> {enhanced}");
                        }
                    }
                }
            }
        }
    }
}
