//! JavaScript jsconfig.json project resolution provider
//!
//! Resolves JavaScript path aliases using jsconfig.json baseUrl and paths configuration.
//! Supports popular React stacks: Create React App, Next.js, Vite.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Settings;
use crate::project_resolver::{
    ResolutionResult, Sha256Hash,
    memo::ResolutionMemo,
    persist::{ResolutionIndex, ResolutionPersistence, ResolutionRules},
    provider::ProjectResolutionProvider,
    sha::compute_file_sha,
};

/// JavaScript-specific configuration path newtype for type safety
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsConfigPath(PathBuf);

impl JsConfigPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// JavaScript project resolution provider
///
/// Handles jsconfig.json parsing, path alias resolution, and SHA-based invalidation
/// for JavaScript projects using path aliases.
pub struct JavaScriptProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)] // Will be used in future iterations
    memo: ResolutionMemo<HashMap<JsConfigPath, Sha256Hash>>,
}

impl Default for JavaScriptProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaScriptProvider {
    /// Create a new JavaScript provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Extract config file paths from JavaScript language settings
    fn extract_config_paths(&self, settings: &Settings) -> Vec<JsConfigPath> {
        settings
            .languages
            .get("javascript")
            .map(|config| {
                config
                    .config_files
                    .iter()
                    .map(|path| JsConfigPath::new(path.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if JavaScript is enabled in language settings
    fn is_javascript_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("javascript")
            .map(|config| config.enabled)
            .unwrap_or(true) // Default to enabled if not configured
    }

    /// Get resolution rules for a specific source file
    ///
    /// Returns the jsconfig resolution rules that apply to the given file path
    pub fn get_resolution_rules_for_file(
        &self,
        file_path: &std::path::Path,
    ) -> Option<ResolutionRules> {
        // Load the resolution index
        let codanna_dir = std::path::Path::new(".codanna");
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("javascript").ok()?;

        // Find the config file for this source file
        let config_path = index.get_config_for_file(file_path)?;

        // Get the resolution rules for this config
        index.rules.get(config_path).cloned()
    }
}

impl ProjectResolutionProvider for JavaScriptProvider {
    fn language_id(&self) -> &'static str {
        "javascript"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        self.is_javascript_enabled(settings)
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        // Convert typed paths back to PathBuf for trait compatibility
        self.extract_config_paths(settings)
            .into_iter()
            .map(|js_path| js_path.0)
            .collect()
    }

    fn compute_shas(&self, configs: &[PathBuf]) -> ResolutionResult<HashMap<PathBuf, Sha256Hash>> {
        let mut shas = HashMap::with_capacity(configs.len());

        for config_path in configs {
            if config_path.exists() {
                let sha = compute_file_sha(config_path)?;
                shas.insert(config_path.clone(), sha);
            }
        }

        Ok(shas)
    }

    fn rebuild_cache(&self, settings: &Settings) -> ResolutionResult<()> {
        let config_paths = self.config_paths(settings);

        // Create persistence manager
        let codanna_dir = std::path::Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        // Load or create resolution index (graceful fallback if cache doesn't exist yet)
        let mut index = persistence
            .load("javascript")
            .unwrap_or_else(|_| ResolutionIndex::new());

        // Process each config file
        for config_path in &config_paths {
            if config_path.exists() {
                // Compute SHA for invalidation detection
                let sha = compute_file_sha(config_path)?;

                // Check if rebuild needed
                if index.needs_rebuild(config_path, &sha) {
                    // Parse jsconfig and resolve extends chain to get effective config
                    let mut visited = std::collections::HashSet::new();
                    let jsconfig = crate::parsing::javascript::jsconfig::resolve_extends_chain(
                        config_path,
                        &mut visited,
                    )?;

                    // Update index with new SHA
                    index.update_sha(config_path, &sha);

                    // Set resolution rules from jsconfig
                    index.set_rules(
                        config_path,
                        ResolutionRules {
                            base_url: jsconfig.compilerOptions.baseUrl,
                            paths: jsconfig.compilerOptions.paths,
                        },
                    );

                    // Add file mappings for JavaScript files
                    if let Some(parent) = config_path.parent() {
                        let pattern = format!("{}/**/*.js", parent.display());
                        index.add_mapping(&pattern, config_path);
                        let pattern_jsx = format!("{}/**/*.jsx", parent.display());
                        index.add_mapping(&pattern_jsx, config_path);
                        let pattern_mjs = format!("{}/**/*.mjs", parent.display());
                        index.add_mapping(&pattern_mjs, config_path);
                        let pattern_cjs = format!("{}/**/*.cjs", parent.display());
                        index.add_mapping(&pattern_cjs, config_path);
                    }
                }
            }
        }

        // Save updated index to disk
        persistence.save("javascript", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, settings: &Settings) -> Vec<PathBuf> {
        let config_paths = self.extract_config_paths(settings);
        let mut affected = Vec::new();

        for config in config_paths {
            let config_path = config.as_path();

            // Root jsconfig affects src directory
            if config_path == &PathBuf::from("jsconfig.json") {
                affected.extend([
                    PathBuf::from("src"),
                    PathBuf::from("lib"),
                    PathBuf::from("index.js"),
                ]);
            }
            // Package-specific jsconfig affects package directory
            else if let Some(parent) = config_path.parent() {
                affected.push(parent.to_path_buf());
            }
        }

        affected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LanguageConfig;

    fn create_test_settings_with_js_config(config_files: Vec<PathBuf>) -> Settings {
        let mut settings = Settings::default();
        let js_config = LanguageConfig {
            enabled: true,
            extensions: vec![
                "js".to_string(),
                "jsx".to_string(),
                "mjs".to_string(),
                "cjs".to_string(),
            ],
            parser_options: HashMap::new(),
            config_files,
        };
        settings
            .languages
            .insert("javascript".to_string(), js_config);
        settings
    }

    #[test]
    fn javascript_provider_has_correct_language_id() {
        let provider = JavaScriptProvider::new();
        assert_eq!(provider.language_id(), "javascript");
    }

    #[test]
    fn javascript_provider_enabled_by_default() {
        let provider = JavaScriptProvider::new();
        let settings = Settings::default();

        assert!(
            provider.is_enabled(&settings),
            "JavaScript should be enabled by default"
        );
    }

    #[test]
    fn javascript_provider_respects_enabled_flag() {
        let provider = JavaScriptProvider::new();
        let mut settings = Settings::default();

        // Explicitly disable JavaScript
        let js_config = LanguageConfig {
            enabled: false,
            extensions: vec!["js".to_string(), "jsx".to_string()],
            parser_options: HashMap::new(),
            config_files: vec![],
        };
        settings
            .languages
            .insert("javascript".to_string(), js_config);

        assert!(
            !provider.is_enabled(&settings),
            "JavaScript should be disabled when explicitly set"
        );
    }

    #[test]
    fn extracts_config_paths_from_settings() {
        let provider = JavaScriptProvider::new();
        let config_files = vec![
            PathBuf::from("jsconfig.json"),
            PathBuf::from("packages/app/jsconfig.json"),
        ];
        let settings = create_test_settings_with_js_config(config_files.clone());

        let paths = provider.config_paths(&settings);

        assert_eq!(paths.len(), 2, "Should extract all config paths");
        assert!(paths.contains(&PathBuf::from("jsconfig.json")));
        assert!(paths.contains(&PathBuf::from("packages/app/jsconfig.json")));
    }

    #[test]
    fn returns_empty_paths_when_no_javascript_config() {
        let provider = JavaScriptProvider::new();
        let settings = Settings::default();

        let paths = provider.config_paths(&settings);

        assert!(
            paths.is_empty(),
            "Should return empty paths when JavaScript not configured"
        );
    }

    #[test]
    fn computes_shas_for_existing_files() {
        use std::fs;
        use std::io::Write;

        let provider = JavaScriptProvider::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("jsconfig.json");

        // Create a real jsconfig file
        let config_content = r#"{"compilerOptions": {"baseUrl": "."}}"#;
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let paths = vec![config_path.clone()];
        let result = provider.compute_shas(&paths);

        assert!(result.is_ok(), "Should compute SHA for existing file");
        let shas = result.unwrap();
        assert_eq!(shas.len(), 1, "Should have one SHA");
        assert!(
            shas.contains_key(&config_path),
            "Should contain SHA for config file"
        );
    }

    #[test]
    fn skips_non_existent_files_in_sha_computation() {
        let provider = JavaScriptProvider::new();
        let non_existent = PathBuf::from("/definitely/does/not/exist/jsconfig.json");
        let paths = vec![non_existent.clone()];

        let result = provider.compute_shas(&paths);

        assert!(
            result.is_ok(),
            "Should handle non-existent files gracefully"
        );
        let shas = result.unwrap();
        assert!(
            shas.is_empty(),
            "Should not include SHAs for non-existent files"
        );
    }

    #[test]
    fn select_affected_files_returns_reasonable_defaults() {
        let provider = JavaScriptProvider::new();
        let settings = create_test_settings_with_js_config(vec![
            PathBuf::from("jsconfig.json"),
            PathBuf::from("packages/app/jsconfig.json"),
        ]);

        let affected = provider.select_affected_files(&settings);

        assert!(!affected.is_empty(), "Should return some affected files");
        assert!(
            affected.iter().any(|p| p.to_str().unwrap().contains("src")),
            "Should include src directory for root jsconfig"
        );
    }
}
