//! Kotlin project configuration provider (Gradle Kotlin DSL)
//!
//! Resolves Kotlin package paths from source roots defined in build.gradle.kts.
//! Uses shared Gradle parsing helpers with Java provider.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{Settings, SourceLayout};
use crate::project_resolver::{
    ResolutionResult, Sha256Hash,
    helpers::{
        compute_config_shas, extract_language_config_paths, get_layout_for_config,
        is_language_enabled, module_for_file_generic, parse_gradle_source_roots,
    },
    memo::ResolutionMemo,
    persist::{ResolutionPersistence, ResolutionRules},
    provider::ProjectResolutionProvider,
};

/// Kotlin-specific project configuration path (build.gradle.kts)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KotlinProjectPath(PathBuf);

impl KotlinProjectPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// Kotlin project resolution provider
///
/// Handles Gradle Kotlin DSL (build.gradle.kts) project configurations
/// to determine source roots for package path resolution.
pub struct KotlinProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)] // Used for future caching optimizations
    memo: ResolutionMemo<HashMap<KotlinProjectPath, Sha256Hash>>,
}

impl Default for KotlinProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl KotlinProvider {
    /// Create a new Kotlin provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Get module path for a Kotlin source file
    ///
    /// Converts file path to package notation by stripping source root prefix.
    /// Example: /project/src/main/kotlin/com/example/Foo.kt → com.example
    pub fn module_path_for_file(&self, file_path: &Path) -> Option<String> {
        module_for_file_generic(file_path, "kotlin", ".")
    }

    /// Parse Gradle build.gradle.kts to extract source roots
    ///
    /// # Arguments
    /// * `gradle_path` - Path to the Gradle build file
    /// * `layout` - Optional explicit source layout. When None, auto-detects.
    fn parse_gradle_config(
        &self,
        gradle_path: &Path,
        layout: Option<SourceLayout>,
    ) -> ResolutionResult<Vec<PathBuf>> {
        parse_gradle_source_roots(gradle_path, "kotlin", layout)
    }

    /// Build resolution rules from project config file
    ///
    /// # Arguments
    /// * `config_path` - Path to the build.gradle.kts file
    /// * `layout` - Optional explicit source layout from settings
    fn build_rules_for_config(
        &self,
        config_path: &Path,
        layout: Option<SourceLayout>,
    ) -> ResolutionResult<ResolutionRules> {
        let file_name = config_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        // Accept both build.gradle.kts and build.gradle (for mixed projects)
        if !file_name.contains("build.gradle") {
            return Err(crate::project_resolver::ResolutionError::ParseError {
                message: format!("Unknown Kotlin config file: {}", config_path.display()),
            });
        }

        let source_roots = self.parse_gradle_config(config_path, layout)?;

        // Convert source roots to paths HashMap
        let mut paths = HashMap::new();
        for root in source_roots {
            paths.insert(root.to_string_lossy().to_string(), Vec::new());
        }

        Ok(ResolutionRules {
            base_url: None,
            paths,
        })
    }
}

impl ProjectResolutionProvider for KotlinProvider {
    fn language_id(&self) -> &'static str {
        "kotlin"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        is_language_enabled(settings, "kotlin")
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        extract_language_config_paths(settings, "kotlin")
    }

    fn compute_shas(&self, configs: &[PathBuf]) -> ResolutionResult<HashMap<PathBuf, Sha256Hash>> {
        compute_config_shas(configs)
    }

    fn rebuild_cache(&self, settings: &Settings) -> ResolutionResult<()> {
        use crate::project_resolver::persist::ResolutionIndex;

        let config_paths = self.config_paths(settings);
        if config_paths.is_empty() {
            return Ok(());
        }

        let persistence = ResolutionPersistence::new(Path::new(crate::init::local_dir_name()));
        let mut index = ResolutionIndex::new();

        // Build rules for each config file
        for config_path in &config_paths {
            // Skip non-existent config files
            if !config_path.exists() {
                continue;
            }

            // Look up explicit layout from settings, or use auto-detect (None)
            let layout = get_layout_for_config(settings, "kotlin", config_path);
            let rules = self.build_rules_for_config(config_path, layout)?;

            // Create file pattern mappings for this config
            // Map all .kt files under project directory to this config
            let project_dir = config_path.parent().unwrap_or(Path::new("."));
            let pattern = format!("{}/**/*.kt", project_dir.display());

            index.mappings.insert(pattern, config_path.clone());
            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        // Save to disk
        persistence.save("kotlin", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When Kotlin config changes, all .kt files need re-indexing
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_provider_language_id() {
        let provider = KotlinProvider::new();
        assert_eq!(provider.language_id(), "kotlin");
    }

    #[test]
    fn test_parse_gradle_default_source_roots() {
        let temp_dir = TempDir::new().unwrap();
        let gradle_path = temp_dir.path().join("build.gradle.kts");

        let gradle_content = r#"
plugins {
    kotlin("jvm") version "1.9.0"
}
"#;
        fs::write(&gradle_path, gradle_content).unwrap();

        let provider = KotlinProvider::new();
        let roots = provider.parse_gradle_config(&gradle_path, None).unwrap();

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|r| r.ends_with("src/main/kotlin")));
        assert!(roots.iter().any(|r| r.ends_with("src/test/kotlin")));
    }

    #[test]
    fn test_parse_gradle_custom_source_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let gradle_path = temp_dir.path().join("build.gradle.kts");

        let gradle_content = r#"
plugins {
    kotlin("jvm") version "1.9.0"
}

sourceSets {
    main {
        kotlin.setSrcDirs(listOf("src/custom/kotlin", "src/generated/kotlin"))
    }
}
"#;
        fs::write(&gradle_path, gradle_content).unwrap();

        let provider = KotlinProvider::new();
        let roots = provider.parse_gradle_config(&gradle_path, None).unwrap();

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|r| r.ends_with("src/custom/kotlin")));
        assert!(roots.iter().any(|r| r.ends_with("src/generated/kotlin")));
    }

    #[test]
    fn test_build_rules_for_config() {
        let temp_dir = TempDir::new().unwrap();
        let gradle_path = temp_dir.path().join("build.gradle.kts");

        fs::write(&gradle_path, "plugins { kotlin(\"jvm\") }").unwrap();

        let provider = KotlinProvider::new();
        let rules = provider.build_rules_for_config(&gradle_path, None).unwrap();

        assert!(rules.base_url.is_none());
        assert_eq!(rules.paths.len(), 2);
    }

    #[test]
    fn test_build_rules_rejects_unknown_config() {
        let temp_dir = TempDir::new().unwrap();
        let unknown_path = temp_dir.path().join("unknown.xml");

        fs::write(&unknown_path, "<project/>").unwrap();

        let provider = KotlinProvider::new();
        let result = provider.build_rules_for_config(&unknown_path, None);

        assert!(result.is_err());
    }

    #[test]
    #[ignore = "Requires filesystem isolation (changes cwd, conflicts with parallel tests)"]
    fn test_rebuild_cache_creates_resolution_json() {
        let temp_dir = TempDir::new().unwrap();
        let gradle_path = temp_dir.path().join("build.gradle.kts");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        fs::write(&gradle_path, "plugins { kotlin(\"jvm\") }").unwrap();

        let settings_content = format!(
            r#"
[languages.kotlin]
enabled = true
config_files = ["{}"]
"#,
            gradle_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        let provider = KotlinProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        std::env::set_current_dir(&original_dir).unwrap();

        let cache_path = codanna_dir.join("index/resolvers/kotlin_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should be created at {}",
            cache_path.display()
        );

        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("src/main/kotlin")
                || cache_content.contains("src\\\\main\\\\kotlin"),
            "Cache should contain Kotlin source root"
        );
    }
}
