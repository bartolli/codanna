//! Go project configuration provider (go.mod)
//!
//! Resolves Go module paths from go.mod files.
//! Extracts module name and source root for full import path resolution.

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

/// Go-specific project configuration path (go.mod)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GoModPath(PathBuf);

impl GoModPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// Information extracted from go.mod file
#[derive(Debug, Clone, Default)]
pub struct GoModInfo {
    /// Module name from 'module' directive (e.g., "github.com/gin-gonic/gin")
    pub module_name: Option<String>,

    /// Go version from 'go' directive
    pub go_version: Option<String>,
}

/// Go project resolution provider
///
/// Handles go.mod parsing to determine module paths for import resolution.
/// Go modules use URL-like paths (e.g., github.com/user/repo/pkg/auth).
pub struct GoProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)]
    memo: ResolutionMemo<HashMap<GoModPath, Sha256Hash>>,
}

impl Default for GoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GoProvider {
    /// Create a new Go provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Get full module path for a Go source file
    ///
    /// Combines go.mod module name with relative file path.
    /// Example: github.com/gin-gonic/gin + pkg/render/html.go -> github.com/gin-gonic/gin/pkg/render
    pub fn module_path_for_file(&self, file_path: &Path) -> Option<String> {
        let codanna_dir = Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("go").ok()?;

        // Canonicalize file path to handle symlinks
        let canon_file = file_path.canonicalize().ok()?;

        // Find the config file (go.mod) for this source file
        let config_path = index.get_config_for_file(&canon_file)?;

        // Get the resolution rules for this config
        let rules = index.rules.get(config_path)?;

        // Get module name from baseUrl
        let module_name = rules.base_url.as_ref()?;

        // Get project root (directory containing go.mod)
        for root_path in rules.paths.keys() {
            let root = Path::new(root_path);
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

            if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                // Get directory path (Go packages are directories)
                let dir_path = relative.parent()?.to_string_lossy();

                if dir_path.is_empty() {
                    // Root package
                    return Some(module_name.clone());
                } else {
                    // Subpackage: module_name/relative/path
                    return Some(format!("{module_name}/{dir_path}"));
                }
            }
        }

        None
    }

    /// Parse go.mod file to extract module information
    fn parse_go_mod(&self, go_mod_path: &Path) -> ResolutionResult<GoModInfo> {
        use std::fs;

        let content = fs::read_to_string(go_mod_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: go_mod_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        let mut info = GoModInfo::default();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            // Parse module directive
            if let Some(module_name) = line.strip_prefix("module ") {
                info.module_name = Some(module_name.trim().to_string());
            }
            // Parse go directive
            else if let Some(go_version) = line.strip_prefix("go ") {
                info.go_version = Some(go_version.trim().to_string());
            }
        }

        Ok(info)
    }

    /// Build resolution rules from go.mod file
    fn build_rules_for_config(&self, config_path: &Path) -> ResolutionResult<ResolutionRules> {
        let go_mod_info = self.parse_go_mod(config_path)?;

        // Project root is the directory containing go.mod
        let project_root = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut paths = HashMap::new();
        paths.insert(project_root.to_string_lossy().to_string(), Vec::new());

        Ok(ResolutionRules {
            base_url: go_mod_info.module_name,
            paths,
        })
    }
}

impl ProjectResolutionProvider for GoProvider {
    fn language_id(&self) -> &'static str {
        "go"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        is_language_enabled(settings, "go")
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        extract_language_config_paths(settings, "go")
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

            // Map all .go files under project directory to this config
            let project_dir = config_path.parent().unwrap_or(Path::new("."));
            let pattern = format!("{}/**/*.go", project_dir.display());

            index.mappings.insert(pattern, config_path.clone());
            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        persistence.save("go", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When go.mod changes, all .go files need re-indexing
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_go_mod_extracts_module_name() {
        let temp_dir = TempDir::new().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");

        let go_mod_content = r#"module github.com/gin-gonic/gin

go 1.21

require (
    github.com/bytedance/sonic v1.9.1
    github.com/go-playground/validator/v10 v10.14.0
)
"#;

        fs::write(&go_mod_path, go_mod_content).unwrap();

        let provider = GoProvider::new();
        let info = provider.parse_go_mod(&go_mod_path).unwrap();

        assert_eq!(
            info.module_name,
            Some("github.com/gin-gonic/gin".to_string())
        );
        assert_eq!(info.go_version, Some("1.21".to_string()));
    }

    #[test]
    fn test_parse_go_mod_handles_minimal_file() {
        let temp_dir = TempDir::new().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");

        let go_mod_content = "module example.com/myproject\n";

        fs::write(&go_mod_path, go_mod_content).unwrap();

        let provider = GoProvider::new();
        let info = provider.parse_go_mod(&go_mod_path).unwrap();

        assert_eq!(info.module_name, Some("example.com/myproject".to_string()));
        assert_eq!(info.go_version, None);
    }

    #[test]
    fn test_build_rules_sets_base_url_to_module_name() {
        let temp_dir = TempDir::new().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");

        let go_mod_content = "module github.com/user/repo\ngo 1.21\n";
        fs::write(&go_mod_path, go_mod_content).unwrap();

        let provider = GoProvider::new();
        let rules = provider.build_rules_for_config(&go_mod_path).unwrap();

        assert_eq!(rules.base_url, Some("github.com/user/repo".to_string()));
        assert_eq!(rules.paths.len(), 1);
        // Project root should be in paths
        assert!(
            rules
                .paths
                .keys()
                .any(|k| k.contains(temp_dir.path().to_str().unwrap()))
        );
    }

    #[test]
    #[ignore = "Requires filesystem isolation (changes cwd, conflicts with parallel tests)"]
    fn test_rebuild_cache_creates_resolution_json() {
        let temp_dir = TempDir::new().unwrap();
        let go_mod_path = temp_dir.path().join("go.mod");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        let go_mod_content = "module github.com/test/project\ngo 1.21\n";
        fs::write(&go_mod_path, go_mod_content).unwrap();

        // Create settings with Go config
        let settings_content = format!(
            r#"
[languages.go]
enabled = true
config_files = ["{}"]
"#,
            go_mod_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory
        let original_dir = std::env::current_dir().unwrap();

        // Use temp .codanna directory
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        let provider = GoProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Verify cache file exists
        let cache_path = codanna_dir.join("index/resolvers/go_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should exist at {}",
            cache_path.display()
        );

        // Verify content
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("github.com/test/project"),
            "Cache should contain module name"
        );
    }

    #[test]
    fn test_provider_language_id() {
        let provider = GoProvider::new();
        assert_eq!(provider.language_id(), "go");
    }

    #[test]
    fn test_provider_uses_helpers_for_settings() {
        let provider = GoProvider::new();
        let settings = Settings::default();

        // Should use helper functions
        assert!(provider.is_enabled(&settings)); // Enabled by default
        assert!(provider.config_paths(&settings).is_empty()); // No config paths
    }
}
