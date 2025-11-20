//! TypeScript tsconfig.json project resolution provider
//!
//! Resolves TypeScript path aliases using tsconfig.json baseUrl and paths configuration.
//! Implements Sprint 1 requirements for basic path alias resolution.

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

/// TypeScript-specific configuration path newtype for type safety
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TsConfigPath(PathBuf);

impl TsConfigPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// TypeScript project resolution provider
///
/// Handles tsconfig.json parsing, path alias resolution, and SHA-based invalidation
/// following Sprint 1 requirements for basic TypeScript support.
pub struct TypeScriptProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)] // Will be used in future iterations
    memo: ResolutionMemo<HashMap<TsConfigPath, Sha256Hash>>,
}

impl Default for TypeScriptProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeScriptProvider {
    /// Create a new TypeScript provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Extract config file paths from TypeScript language settings
    ///
    /// Uses zero-cost abstraction pattern with borrowed settings
    fn extract_config_paths(&self, settings: &Settings) -> Vec<TsConfigPath> {
        settings
            .languages
            .get("typescript")
            .map(|config| {
                config
                    .config_files
                    .iter()
                    .map(|path| TsConfigPath::new(path.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if TypeScript is enabled in language settings
    fn is_typescript_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("typescript")
            .map(|config| config.enabled)
            .unwrap_or(true) // Default to enabled if not configured
    }

    /// Get resolution rules for a specific source file
    ///
    /// Returns the tsconfig resolution rules that apply to the given file path
    pub fn get_resolution_rules_for_file(
        &self,
        file_path: &std::path::Path,
    ) -> Option<ResolutionRules> {
        // Load the resolution index
        let codanna_dir = std::path::Path::new(".codanna");
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("typescript").ok()?;

        // Find the config file for this source file
        let config_path = index.get_config_for_file(file_path)?;

        // Get the resolution rules for this config
        index.rules.get(config_path).cloned()
    }
}

impl ProjectResolutionProvider for TypeScriptProvider {
    fn language_id(&self) -> &'static str {
        "typescript"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        self.is_typescript_enabled(settings)
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        // Convert typed paths back to PathBuf for trait compatibility
        self.extract_config_paths(settings)
            .into_iter()
            .map(|ts_path| ts_path.0)
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
            .load("typescript")
            .unwrap_or_else(|_| ResolutionIndex::new());

        // Process each config file
        for config_path in &config_paths {
            if config_path.exists() {
                // Compute SHA for invalidation detection
                let sha = compute_file_sha(config_path)?;

                // Check if rebuild needed
                if index.needs_rebuild(config_path, &sha) {
                    // Parse tsconfig and resolve extends chain to get effective config
                    let mut visited = std::collections::HashSet::new();
                    let tsconfig = crate::parsing::typescript::tsconfig::resolve_extends_chain(
                        config_path,
                        &mut visited,
                    )?;

                    // Update index with new SHA
                    index.update_sha(config_path, &sha);

                    // Set resolution rules from tsconfig
                    index.set_rules(
                        config_path,
                        ResolutionRules {
                            base_url: tsconfig.compilerOptions.baseUrl,
                            paths: tsconfig.compilerOptions.paths,
                        },
                    );

                    // Add file mappings (basic heuristic for Sprint 1)
                    if let Some(parent) = config_path.parent() {
                        let pattern = format!("{}/**/*.ts", parent.display());
                        index.add_mapping(&pattern, config_path);
                        let pattern_tsx = format!("{}/**/*.tsx", parent.display());
                        index.add_mapping(&pattern_tsx, config_path);
                    }
                }
            }
        }

        // Save updated index to disk
        persistence.save("typescript", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, settings: &Settings) -> Vec<PathBuf> {
        // Sprint 1: Basic file selection based on config presence
        // Future: Implement longest-prefix matching logic

        let config_paths = self.extract_config_paths(settings);
        let mut affected = Vec::new();

        for config in config_paths {
            let config_path = config.as_path();

            // Basic heuristic: root tsconfig affects src/**/*.ts
            if config_path == &PathBuf::from("tsconfig.json") {
                affected.extend([
                    PathBuf::from("src"),
                    PathBuf::from("lib"),
                    PathBuf::from("index.ts"),
                ]);
            }
            // Package-specific tsconfig affects package directory
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

    fn create_test_settings_with_ts_config(config_files: Vec<PathBuf>) -> Settings {
        let mut settings = Settings::default();
        let ts_config = LanguageConfig {
            enabled: true,
            extensions: vec!["ts".to_string(), "tsx".to_string()],
            parser_options: HashMap::new(),
            config_files,
        };
        settings
            .languages
            .insert("typescript".to_string(), ts_config);
        settings
    }

    #[test]
    fn typescript_provider_has_correct_language_id() {
        let provider = TypeScriptProvider::new();
        assert_eq!(provider.language_id(), "typescript");
    }

    #[test]
    fn typescript_provider_enabled_by_default() {
        let provider = TypeScriptProvider::new();
        let settings = Settings::default();

        assert!(
            provider.is_enabled(&settings),
            "TypeScript should be enabled by default"
        );
    }

    #[test]
    fn typescript_provider_respects_enabled_flag() {
        let provider = TypeScriptProvider::new();
        let mut settings = Settings::default();

        // Explicitly disable TypeScript
        let ts_config = LanguageConfig {
            enabled: false,
            extensions: vec!["ts".to_string(), "tsx".to_string()],
            parser_options: HashMap::new(),
            config_files: vec![],
        };
        settings
            .languages
            .insert("typescript".to_string(), ts_config);

        assert!(
            !provider.is_enabled(&settings),
            "TypeScript should be disabled when explicitly set"
        );
    }

    #[test]
    fn extracts_config_paths_from_settings() {
        let provider = TypeScriptProvider::new();
        let config_files = vec![
            PathBuf::from("tsconfig.json"),
            PathBuf::from("packages/web/tsconfig.json"),
        ];
        let settings = create_test_settings_with_ts_config(config_files.clone());

        let paths = provider.config_paths(&settings);

        assert_eq!(paths.len(), 2, "Should extract all config paths");
        assert!(paths.contains(&PathBuf::from("tsconfig.json")));
        assert!(paths.contains(&PathBuf::from("packages/web/tsconfig.json")));
    }

    #[test]
    fn returns_empty_paths_when_no_typescript_config() {
        let provider = TypeScriptProvider::new();
        let settings = Settings::default();

        let paths = provider.config_paths(&settings);

        assert!(
            paths.is_empty(),
            "Should return empty paths when TypeScript not configured"
        );
    }

    #[test]
    fn computes_shas_for_existing_files() {
        use std::fs;
        use std::io::Write;

        let provider = TypeScriptProvider::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("tsconfig.json");

        // Create a real tsconfig file
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
        let provider = TypeScriptProvider::new();
        let non_existent = PathBuf::from("/definitely/does/not/exist/tsconfig.json");
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
    fn rebuild_cache_succeeds_without_actual_files() {
        let provider = TypeScriptProvider::new();
        let settings = create_test_settings_with_ts_config(vec![PathBuf::from("tsconfig.json")]);

        let result = provider.rebuild_cache(&settings);

        assert!(
            result.is_ok(),
            "Should handle missing config files gracefully"
        );
    }

    #[test]
    fn select_affected_files_returns_reasonable_defaults() {
        let provider = TypeScriptProvider::new();
        let settings = create_test_settings_with_ts_config(vec![
            PathBuf::from("tsconfig.json"),
            PathBuf::from("packages/web/tsconfig.json"),
        ]);

        let affected = provider.select_affected_files(&settings);

        assert!(!affected.is_empty(), "Should return some affected files");
        assert!(
            affected.iter().any(|p| p.to_str().unwrap().contains("src")),
            "Should include src directory for root tsconfig"
        );
    }
}
