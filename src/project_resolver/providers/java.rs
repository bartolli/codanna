//! Java project configuration provider (Maven/Gradle)
//!
//! Resolves Java package paths from source roots defined in pom.xml or build.gradle.
//! Similar to TypeScriptProvider but for Java project structures.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::project_resolver::{
    ResolutionResult, Sha256Hash,
    memo::ResolutionMemo,
    persist::{ResolutionPersistence, ResolutionRules},
    provider::ProjectResolutionProvider,
    sha::compute_file_sha,
};

/// Java-specific project configuration path (pom.xml or build.gradle)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JavaProjectPath(PathBuf);

impl JavaProjectPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// Java project resolution provider
///
/// Handles Maven (pom.xml) and Gradle (build.gradle) project configurations
/// to determine source roots for package path resolution.
pub struct JavaProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)] // Used for future caching optimizations
    memo: ResolutionMemo<HashMap<JavaProjectPath, Sha256Hash>>,
}

impl Default for JavaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaProvider {
    /// Create a new Java provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Extract project config paths from Java language settings
    fn extract_config_paths(&self, settings: &Settings) -> Vec<JavaProjectPath> {
        settings
            .languages
            .get("java")
            .map(|config| {
                config
                    .config_files
                    .iter()
                    .map(|path| JavaProjectPath::new(path.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if Java is enabled in language settings
    fn is_java_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("java")
            .map(|config| config.enabled)
            .unwrap_or(true) // Default to enabled
    }

    /// Get package path for a Java source file
    ///
    /// Converts file path to package notation by stripping source root prefix.
    /// Example: /project/src/main/java/com/example/Foo.java → com.example
    pub fn package_for_file(&self, file_path: &Path) -> Option<String> {
        // Load cached resolution rules
        let codanna_dir = Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("java").ok()?;

        // Canonicalize file path early to handle symlinks (e.g., /var → /private/var on macOS)
        let canon_file = file_path.canonicalize().ok()?;

        // Find the config file for this source file
        let config_path = index.get_config_for_file(&canon_file)?;

        // Get the resolution rules for this config
        let rules = index.rules.get(config_path)?;

        // Extract source roots from rules.paths

        for root_path in rules.paths.keys() {
            let root = Path::new(root_path);

            // Canonicalize root path if it exists (runtime resolution)
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

            // Try to strip prefix (both canonicalized now)
            if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                // Convert path to package: com/example/Foo.java → com.example
                let package_path = relative
                    .parent()? // Remove Foo.java
                    .to_string_lossy()
                    .replace(['/', '\\'], ".");

                return Some(package_path);
            }
        }

        None
    }

    /// Parse Maven pom.xml to extract source roots
    fn parse_maven_config(&self, pom_path: &Path) -> ResolutionResult<Vec<PathBuf>> {
        use std::fs;

        let content = fs::read_to_string(pom_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: pom_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        let mut source_roots = Vec::new();
        let project_dir = pom_path.parent().unwrap_or(Path::new("."));

        // Simple XML parsing for <sourceDirectory> tags
        // Default Maven source root if not specified
        if !content.contains("<sourceDirectory>") {
            source_roots.push(project_dir.join("src/main/java"));
        } else {
            // Extract custom source directory
            // This is a simplified parser - production code would use xml-rs
            if let Some(start) = content.find("<sourceDirectory>") {
                if let Some(end) = content[start..].find("</sourceDirectory>") {
                    let src_dir = &content[start + 17..start + end];
                    source_roots.push(project_dir.join(src_dir.trim()));
                }
            }
        }

        // Add test sources
        if !content.contains("<testSourceDirectory>") {
            source_roots.push(project_dir.join("src/test/java"));
        }

        Ok(source_roots)
    }

    /// Parse Gradle build.gradle to extract source roots
    fn parse_gradle_config(&self, gradle_path: &Path) -> ResolutionResult<Vec<PathBuf>> {
        use std::fs;

        let content = fs::read_to_string(gradle_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: gradle_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        let mut source_roots = Vec::new();
        let project_dir = gradle_path.parent().unwrap_or(Path::new("."));

        // Default Gradle source roots
        if !content.contains("srcDirs") {
            source_roots.push(project_dir.join("src/main/java"));
            source_roots.push(project_dir.join("src/test/java"));
        }
        // TODO: Parse custom srcDirs from build.gradle

        Ok(source_roots)
    }

    /// Build resolution rules from project config file
    fn build_rules_for_config(&self, config_path: &Path) -> ResolutionResult<ResolutionRules> {
        let source_roots = if config_path.file_name().unwrap_or_default() == "pom.xml" {
            self.parse_maven_config(config_path)?
        } else if config_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .contains("build.gradle")
        {
            self.parse_gradle_config(config_path)?
        } else {
            return Err(crate::project_resolver::ResolutionError::ParseError {
                message: format!("Unknown Java config file: {}", config_path.display()),
            });
        };

        // Convert source roots to paths HashMap
        // Don't canonicalize to avoid symlink inconsistencies (per TypeScript pattern)
        // Canonicalization happens at runtime in package_for_file() and module_path_from_file()
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

impl ProjectResolutionProvider for JavaProvider {
    fn language_id(&self) -> &'static str {
        "java"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        self.is_java_enabled(settings)
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        self.extract_config_paths(settings)
            .into_iter()
            .map(|p| p.0)
            .collect()
    }

    fn compute_shas(&self, configs: &[PathBuf]) -> ResolutionResult<HashMap<PathBuf, Sha256Hash>> {
        let mut shas = HashMap::with_capacity(configs.len());
        for config in configs {
            let sha = compute_file_sha(config)?;
            shas.insert(config.clone(), sha);
        }
        Ok(shas)
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
            // Skip non-existent config files (graceful handling like TypeScript)
            if !config_path.exists() {
                continue;
            }

            let rules = self.build_rules_for_config(config_path)?;

            // Create file pattern mappings for this config
            // Map all .java files under project directory to this config
            // Don't canonicalize to avoid symlink inconsistencies (per TypeScript pattern)
            let project_dir = config_path.parent().unwrap_or(Path::new("."));
            let pattern = format!("{}/**/*.java", project_dir.display());

            index.mappings.insert(pattern, config_path.clone());
            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        // Save to disk
        persistence.save("java", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When Java config changes, all .java files need re-indexing
        // This is called when pom.xml or build.gradle changes
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_maven_default_source_roots() {
        // TDD: Given a minimal pom.xml without custom source directories
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        let pom_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>
</project>"#;

        fs::write(&pom_path, pom_content).unwrap();

        // When parsing the pom.xml
        let provider = JavaProvider::new();
        let roots = provider.parse_maven_config(&pom_path).unwrap();

        // Then it should return default Maven source roots
        assert_eq!(roots.len(), 2, "Should have main and test source roots");
        assert!(
            roots.iter().any(|r| r.ends_with("src/main/java")),
            "Should have src/main/java"
        );
        assert!(
            roots.iter().any(|r| r.ends_with("src/test/java")),
            "Should have src/test/java"
        );
    }

    #[test]
    fn test_parse_maven_custom_source_directory() {
        // TDD: Given a pom.xml with custom <sourceDirectory>
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        let pom_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <build>
        <sourceDirectory>custom/src/main</sourceDirectory>
    </build>
</project>"#;

        fs::write(&pom_path, pom_content).unwrap();

        // When parsing the pom.xml
        let provider = JavaProvider::new();
        let roots = provider.parse_maven_config(&pom_path).unwrap();

        // Then it should use the custom source directory
        assert!(
            roots.iter().any(|r| r.ends_with("custom/src/main")),
            "Should have custom source directory"
        );
    }

    #[test]
    fn test_rebuild_cache_creates_resolution_json() {
        // TDD: Given a pom.xml and settings
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        let pom_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
</project>"#;

        fs::write(&pom_path, pom_content).unwrap();

        // Create settings with Java config
        let settings_content = format!(
            r#"
[languages.java]
enabled = true
config_files = ["{}"]
"#,
            pom_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory to restore later
        let original_dir = std::env::current_dir().unwrap();

        // When rebuilding cache
        let provider = JavaProvider::new();

        // Use temp .codanna directory for this test
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        provider.rebuild_cache(&settings).unwrap();

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Then java_resolution.json should exist
        let cache_path = codanna_dir.join("index/resolvers/java_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should be created at {}",
            cache_path.display()
        );

        // And it should contain source roots
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("src/main/java")
                || cache_content.contains("src\\\\main\\\\java"),
            "Cache should contain source root path"
        );
    }

    #[test]
    fn test_package_for_file_converts_path_to_package() {
        // TDD: Given a Java file under src/main/java with cached project config
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        let pom_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
</project>"#;

        fs::write(&pom_path, pom_content).unwrap();

        // Create settings and build cache
        let settings_content = format!(
            r#"
[languages.java]
enabled = true
config_files = ["{}"]
"#,
            pom_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory to restore later
        let original_dir = std::env::current_dir().unwrap();

        // Build cache
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(temp_dir.path().join(crate::init::local_dir_name())).unwrap();

        let provider = JavaProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        // Create test Java file: src/main/java/com/example/owner/Owner.java
        let java_file = temp_dir
            .path()
            .join("src/main/java/com/example/owner/Owner.java");
        fs::create_dir_all(java_file.parent().unwrap()).unwrap();
        fs::write(
            &java_file,
            "package com.example.owner; public class Owner {}",
        )
        .unwrap();

        // Debug: Check what was cached
        let cache_path = temp_dir.path().join(format!(
            "{}/index/resolvers/java_resolution.json",
            crate::init::local_dir_name()
        ));
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        eprintln!("Cache content: {cache_content}");
        eprintln!("Java file path: {}", java_file.display());

        // When calling package_for_file() (must run while cwd is still temp_dir)
        let package = provider.package_for_file(&java_file);

        // Then it should return the package path
        assert_eq!(
            package,
            Some("com.example.owner".to_string()),
            "Should convert file path to package notation"
        );

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();
    }

    #[test]
    fn test_provider_language_id() {
        let provider = JavaProvider::new();
        assert_eq!(provider.language_id(), "java");
    }

    #[test]
    #[ignore] // Run with: cargo test test_package_for_owner_file -- --ignored --nocapture
    fn test_package_for_owner_file() {
        // Test that package_for_file works with the cached resolution
        let provider = JavaProvider::new();

        let owner_file = std::path::Path::new(
            "/Users/bartolli/Projects/codanna/test_monorepos/spring-petclinic/src/main/java/org/springframework/samples/petclinic/owner/Owner.java",
        );

        let package = provider.package_for_file(owner_file);

        println!("package_for_file result: {package:?}");

        assert_eq!(
            package,
            Some("org.springframework.samples.petclinic.owner".to_string()),
            "Should extract package path from file path"
        );
    }
}
