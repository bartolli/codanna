//! PHP project configuration provider (composer.json)
//!
//! Resolves PHP namespaces from composer.json PSR-4 autoload configuration.
//! Maps namespace prefixes to source directories for proper FQN resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::project_resolver::{
    ResolutionResult, Sha256Hash,
    helpers::{compute_config_shas, extract_language_config_paths, is_language_enabled},
    memo::ResolutionMemo,
    persist::{ResolutionIndex, ResolutionPersistence, ResolutionRules},
    provider::ProjectResolutionProvider,
};

/// PHP-specific project configuration path (composer.json)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComposerJsonPath(PathBuf);

impl ComposerJsonPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// PSR-4 namespace mapping extracted from composer.json
#[derive(Debug, Clone)]
pub struct Psr4Mapping {
    /// Namespace prefix (e.g., "App\\")
    pub namespace_prefix: String,
    /// Source directories for this namespace (can be multiple)
    pub directories: Vec<PathBuf>,
}

/// Information extracted from composer.json autoload section
#[derive(Debug, Clone, Default)]
pub struct ComposerAutoloadInfo {
    /// PSR-4 mappings from autoload section
    pub psr4: Vec<Psr4Mapping>,
    /// PSR-4 mappings from autoload-dev section
    pub psr4_dev: Vec<Psr4Mapping>,
}

/// PHP project resolution provider
///
/// Handles composer.json parsing to determine namespace mappings for PSR-4 resolution.
/// PHP uses backslash-separated namespaces (e.g., App\Controllers\UserController).
pub struct PhpProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)]
    memo: ResolutionMemo<HashMap<ComposerJsonPath, Sha256Hash>>,
}

impl Default for PhpProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PhpProvider {
    /// Create a new PHP provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Get namespace for a PHP source file based on PSR-4 mappings
    ///
    /// Example: With mapping "App\\": "src/", file src/Controllers/User.php
    /// becomes namespace App\Controllers\User
    pub fn namespace_for_file(&self, file_path: &Path) -> Option<String> {
        let codanna_dir = Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("php").ok()?;

        // Canonicalize file path to handle symlinks
        let canon_file = file_path.canonicalize().ok()?;

        // Find the config file (composer.json) for this source file
        let config_path = index.get_config_for_file(&canon_file)?;

        // Get the resolution rules for this config
        let rules = index.rules.get(config_path)?;

        // Try each source root to find the matching namespace prefix
        for (source_root_str, namespace_prefixes) in &rules.paths {
            let source_root = Path::new(source_root_str);
            let canon_root = source_root
                .canonicalize()
                .unwrap_or_else(|_| source_root.to_path_buf());

            if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                // Remove .php extension
                let relative_str = relative.to_string_lossy();
                let without_ext = relative_str
                    .strip_suffix(".php")
                    .or_else(|| relative_str.strip_suffix(".class.php"))
                    .unwrap_or(&relative_str);

                // Convert path separators to namespace separators
                let namespace_suffix = without_ext.replace('/', "\\");

                // Get namespace prefix (first element in prefixes array)
                let namespace_prefix = namespace_prefixes.first().map(|s| s.as_str()).unwrap_or("");

                // Combine prefix + suffix
                if namespace_suffix.is_empty() {
                    // Root of source directory
                    let result = namespace_prefix.trim_end_matches('\\');
                    return Some(format!("\\{result}"));
                } else {
                    let prefix_trimmed = namespace_prefix.trim_end_matches('\\');
                    return Some(format!("\\{prefix_trimmed}\\{namespace_suffix}"));
                }
            }
        }

        None
    }

    /// Parse composer.json to extract PSR-4 autoload information
    fn parse_composer_json(&self, composer_path: &Path) -> ResolutionResult<ComposerAutoloadInfo> {
        use std::fs;

        let content = fs::read_to_string(composer_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: composer_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            crate::project_resolver::ResolutionError::ParseError {
                message: format!("Failed to parse {}: {e}", composer_path.display()),
            }
        })?;

        let mut info = ComposerAutoloadInfo::default();
        let project_root = composer_path.parent().unwrap_or(Path::new("."));

        // Parse autoload.psr-4
        if let Some(psr4) = json.get("autoload").and_then(|a| a.get("psr-4")) {
            info.psr4 = self.parse_psr4_section(psr4, project_root);
        }

        // Parse autoload-dev.psr-4
        if let Some(psr4_dev) = json.get("autoload-dev").and_then(|a| a.get("psr-4")) {
            info.psr4_dev = self.parse_psr4_section(psr4_dev, project_root);
        }

        Ok(info)
    }

    /// Parse a PSR-4 section from composer.json
    ///
    /// Handles both simple string values and array values:
    /// - `"App\\": "src/"` (simple)
    /// - `"App\\": ["src/", "lib/"]` (array)
    fn parse_psr4_section(
        &self,
        psr4: &serde_json::Value,
        project_root: &Path,
    ) -> Vec<Psr4Mapping> {
        let mut mappings = Vec::new();

        if let Some(obj) = psr4.as_object() {
            for (namespace_prefix, dirs) in obj {
                let directories: Vec<PathBuf> = match dirs {
                    // Array of directories
                    serde_json::Value::Array(arr) => arr
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|d| project_root.join(d))
                        .collect(),
                    // Single directory string
                    serde_json::Value::String(s) => {
                        vec![project_root.join(s)]
                    }
                    _ => continue,
                };

                if !directories.is_empty() {
                    mappings.push(Psr4Mapping {
                        namespace_prefix: namespace_prefix.clone(),
                        directories,
                    });
                }
            }
        }

        mappings
    }

    /// Build resolution rules from composer.json
    fn build_rules_for_config(&self, config_path: &Path) -> ResolutionResult<ResolutionRules> {
        let autoload_info = self.parse_composer_json(config_path)?;

        // Combine psr4 and psr4_dev mappings
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();

        // Process all PSR-4 mappings
        for mapping in autoload_info
            .psr4
            .iter()
            .chain(autoload_info.psr4_dev.iter())
        {
            for dir in &mapping.directories {
                let dir_str = dir.to_string_lossy().to_string();
                paths
                    .entry(dir_str)
                    .or_default()
                    .push(mapping.namespace_prefix.clone());
            }
        }

        Ok(ResolutionRules {
            base_url: None, // PHP doesn't have a single base URL
            paths,
        })
    }
}

impl ProjectResolutionProvider for PhpProvider {
    fn language_id(&self) -> &'static str {
        "php"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        is_language_enabled(settings, "php")
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        extract_language_config_paths(settings, "php")
    }

    fn compute_shas(&self, configs: &[PathBuf]) -> ResolutionResult<HashMap<PathBuf, Sha256Hash>> {
        compute_config_shas(configs)
    }

    fn rebuild_cache(&self, settings: &Settings) -> ResolutionResult<()> {
        let config_paths = self.config_paths(settings);
        if config_paths.is_empty() {
            return Ok(());
        }

        let persistence = ResolutionPersistence::new(Path::new(crate::init::local_dir_name()));
        let mut index = ResolutionIndex::new();

        for config_path in &config_paths {
            if !config_path.exists() {
                continue;
            }

            let rules = self.build_rules_for_config(config_path)?;

            // Map all .php files under each source directory to this config
            for source_dir in rules.paths.keys() {
                let pattern = format!("{source_dir}/**/*.php");
                index.mappings.insert(pattern, config_path.clone());
            }

            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        persistence.save("php", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When composer.json changes, all .php files need re-indexing
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_composer_json_simple_psr4() {
        let temp_dir = TempDir::new().unwrap();
        let composer_path = temp_dir.path().join("composer.json");

        let composer_content = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/"
        }
    }
}"#;

        fs::write(&composer_path, composer_content).unwrap();

        let provider = PhpProvider::new();
        let info = provider.parse_composer_json(&composer_path).unwrap();

        assert_eq!(info.psr4.len(), 1);
        assert_eq!(info.psr4[0].namespace_prefix, "App\\");
        assert_eq!(info.psr4[0].directories.len(), 1);
        assert!(info.psr4[0].directories[0].ends_with("src"));
    }

    #[test]
    fn test_parse_composer_json_array_syntax() {
        let temp_dir = TempDir::new().unwrap();
        let composer_path = temp_dir.path().join("composer.json");

        // Laravel framework uses array syntax
        let composer_content = r#"{
    "autoload": {
        "psr-4": {
            "Illuminate\\Support\\": [
                "src/Illuminate/Macroable/",
                "src/Illuminate/Collections/"
            ]
        }
    }
}"#;

        fs::write(&composer_path, composer_content).unwrap();

        let provider = PhpProvider::new();
        let info = provider.parse_composer_json(&composer_path).unwrap();

        assert_eq!(info.psr4.len(), 1);
        assert_eq!(info.psr4[0].namespace_prefix, "Illuminate\\Support\\");
        assert_eq!(info.psr4[0].directories.len(), 2);
    }

    #[test]
    fn test_parse_composer_json_multiple_mappings() {
        let temp_dir = TempDir::new().unwrap();
        let composer_path = temp_dir.path().join("composer.json");

        let composer_content = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Database\\": "database/"
        }
    },
    "autoload-dev": {
        "psr-4": {
            "Tests\\": "tests/"
        }
    }
}"#;

        fs::write(&composer_path, composer_content).unwrap();

        let provider = PhpProvider::new();
        let info = provider.parse_composer_json(&composer_path).unwrap();

        assert_eq!(info.psr4.len(), 2);
        assert_eq!(info.psr4_dev.len(), 1);
        assert_eq!(info.psr4_dev[0].namespace_prefix, "Tests\\");
    }

    #[test]
    fn test_build_rules_creates_path_mappings() {
        let temp_dir = TempDir::new().unwrap();
        let composer_path = temp_dir.path().join("composer.json");

        let composer_content = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Tests\\": "tests/"
        }
    }
}"#;

        fs::write(&composer_path, composer_content).unwrap();

        let provider = PhpProvider::new();
        let rules = provider.build_rules_for_config(&composer_path).unwrap();

        // Should have no baseUrl (PHP doesn't use it)
        assert!(rules.base_url.is_none());

        // Should have 2 paths
        assert_eq!(rules.paths.len(), 2);

        // Each path should have its namespace prefix
        for (path, prefixes) in &rules.paths {
            assert!(!prefixes.is_empty());
            if path.contains("src") {
                assert!(prefixes.contains(&"App\\".to_string()));
            } else if path.contains("tests") {
                assert!(prefixes.contains(&"Tests\\".to_string()));
            }
        }
    }

    #[test]
    fn test_parse_composer_json_with_empty_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let composer_path = temp_dir.path().join("composer.json");

        // Empty prefix is valid PSR-4 (fallback namespace)
        let composer_content = r#"{
    "autoload": {
        "psr-4": {
            "": "src/"
        }
    }
}"#;

        fs::write(&composer_path, composer_content).unwrap();

        let provider = PhpProvider::new();
        let info = provider.parse_composer_json(&composer_path).unwrap();

        assert_eq!(info.psr4.len(), 1);
        assert_eq!(info.psr4[0].namespace_prefix, "");
    }

    #[test]
    fn test_provider_language_id() {
        let provider = PhpProvider::new();
        assert_eq!(provider.language_id(), "php");
    }

    #[test]
    fn test_provider_uses_helpers_for_settings() {
        let provider = PhpProvider::new();
        let settings = Settings::default();

        // Should use helper functions
        assert!(provider.is_enabled(&settings)); // Enabled by default
        assert!(provider.config_paths(&settings).is_empty()); // No config paths
    }

    #[test]
    #[ignore = "Requires filesystem isolation (changes cwd, conflicts with parallel tests)"]
    fn test_rebuild_cache_creates_resolution_json() {
        let temp_dir = TempDir::new().unwrap();
        let composer_path = temp_dir.path().join("composer.json");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        let composer_content = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/"
        }
    }
}"#;
        fs::write(&composer_path, composer_content).unwrap();

        // Create settings with PHP config
        let settings_content = format!(
            r#"
[languages.php]
enabled = true
config_files = ["{}"]
"#,
            composer_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory
        let original_dir = std::env::current_dir().unwrap();

        // Use temp .codanna directory
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        let provider = PhpProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Verify cache file exists
        let cache_path = codanna_dir.join("index/resolvers/php_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should exist at {}",
            cache_path.display()
        );

        // Verify content
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("App\\\\"),
            "Cache should contain namespace prefix"
        );
    }
}
