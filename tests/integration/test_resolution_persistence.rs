#[cfg(test)]
mod tests {
    use codanna::project_resolver::persist::{
        ResolutionIndex, ResolutionPersistence, ResolutionRules,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_resolution_persistence_save_and_load() {
        // Use a temp directory for testing
        let temp_dir = std::env::temp_dir().join("codanna_test_persist");
        fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

        let persistence = ResolutionPersistence::new(&temp_dir);

        let mut index = ResolutionIndex::new();
        let tsconfig_path = Path::new("examples/typescript/tsconfig.json");

        // Add some test data
        index.update_sha(
            tsconfig_path,
            &codanna::project_resolver::Sha256Hash::from_bytes(&[42; 32]),
        );
        index.add_mapping("src/**/*.ts", tsconfig_path);
        index.set_rules(
            tsconfig_path,
            ResolutionRules {
                base_url: Some("./".to_string()),
                paths: HashMap::from([
                    (
                        "@components/*".to_string(),
                        vec!["src/components/*".to_string()],
                    ),
                    ("@utils/*".to_string(), vec!["src/utils/*".to_string()]),
                ]),

                relative_specifiers_redirected: false,
            },
        );

        // Save it
        persistence
            .save("typescript", &index)
            .expect("Failed to save");

        // Load it back
        let loaded_index = persistence.load("typescript").expect("Failed to load");

        // Verify the data
        assert!(
            loaded_index.needs_rebuild(
                tsconfig_path,
                &codanna::project_resolver::Sha256Hash::from_bytes(&[43; 32])
            ),
            "Should need rebuild with different SHA"
        );
        assert!(
            !loaded_index.needs_rebuild(
                tsconfig_path,
                &codanna::project_resolver::Sha256Hash::from_bytes(&[42; 32])
            ),
            "Should not need rebuild with same SHA"
        );

        // Clean up
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_resolution_persistence_file_structure() {
        let temp_dir = std::env::temp_dir().join("codanna_test_persist_structure");
        fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

        let persistence = ResolutionPersistence::new(&temp_dir);

        let mut index = ResolutionIndex::new();
        let tsconfig_path = Path::new("examples/typescript/tsconfig.json");

        // Add minimal data
        index.update_sha(
            tsconfig_path,
            &codanna::project_resolver::Sha256Hash::from_bytes(&[1; 32]),
        );

        // Save it
        persistence
            .save("typescript", &index)
            .expect("Failed to save");

        // Check file was created in the right place
        let expected_file = temp_dir.join("index/resolvers/typescript_resolution.json");
        assert!(
            expected_file.exists(),
            "Resolution file should exist at {expected_file:?}"
        );

        // Clean up
        fs::remove_dir_all(&temp_dir).ok();
    }
}
