//! Swift project configuration provider (Swift Package Manager)
//!
//! Resolves Swift module paths from source roots defined in Package.swift.
//! Similar to JavaProvider but for Swift Package Manager project structures.

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

/// Swift-specific project configuration path (Package.swift)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SwiftPackagePath(PathBuf);

impl SwiftPackagePath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// Swift project resolution provider
///
/// Handles Swift Package Manager (Package.swift) project configurations
/// to determine source roots for module path resolution.
pub struct SwiftProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)] // Used for future caching optimizations
    memo: ResolutionMemo<HashMap<SwiftPackagePath, Sha256Hash>>,
}

impl Default for SwiftProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SwiftProvider {
    /// Create a new Swift provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Extract project config paths from Swift language settings
    fn extract_config_paths(&self, settings: &Settings) -> Vec<SwiftPackagePath> {
        settings
            .languages
            .get("swift")
            .map(|config| {
                config
                    .config_files
                    .iter()
                    .map(|path| SwiftPackagePath::new(path.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if Swift is enabled in language settings
    fn is_swift_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("swift")
            .map(|config| config.enabled)
            .unwrap_or(true) // Default to enabled
    }

    /// Get module path for a Swift source file
    ///
    /// Converts file path to module notation by stripping source root prefix.
    /// Example: /project/Sources/MyModule/Types/User.swift -> MyModule.Types
    pub fn module_for_file(&self, file_path: &Path) -> Option<String> {
        // Load cached resolution rules
        let codanna_dir = Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("swift").ok()?;

        // Canonicalize file path early to handle symlinks
        let canon_file = file_path.canonicalize().ok()?;

        // Find the config file for this source file
        let config_path = index.get_config_for_file(&canon_file)?;

        // Get the resolution rules for this config
        let rules = index.rules.get(config_path)?;

        // Extract module path from file path using source roots
        for root_path in rules.paths.keys() {
            let root = Path::new(root_path);

            // Canonicalize root path if it exists (runtime resolution)
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

            // Try to strip prefix (both canonicalized now)
            if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                // Convert path to module: MyModule/Types/User.swift -> MyModule.Types
                let module_path = relative
                    .parent()? // Remove User.swift
                    .to_string_lossy()
                    .replace(['/', '\\'], ".");

                return Some(module_path);
            }
        }

        None
    }

    /// Parse Package.swift to extract source roots
    ///
    /// Swift Package Manager uses convention-based source directories:
    /// - Sources/<ModuleName>/ for library/executable targets
    /// - Tests/<ModuleName>Tests/ for test targets
    fn parse_package_swift(&self, package_path: &Path) -> ResolutionResult<Vec<PathBuf>> {
        use std::fs;

        let content = fs::read_to_string(package_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: package_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        let mut source_roots = Vec::new();
        let project_dir = package_path.parent().unwrap_or(Path::new("."));

        // Check for custom path specifications in targets
        // .target(name: "MyLib", path: "CustomSources/MyLib")
        let mut found_custom_paths = false;
        for line in content.lines() {
            if let Some(path_start) = line.find("path:") {
                if let Some(quote_start) = line[path_start..].find('"') {
                    let after_quote = &line[path_start + quote_start + 1..];
                    if let Some(quote_end) = after_quote.find('"') {
                        let custom_path = &after_quote[..quote_end];
                        source_roots.push(project_dir.join(custom_path));
                        found_custom_paths = true;
                    }
                }
            }
        }

        // If no custom paths found, use SPM conventions
        if !found_custom_paths {
            // Default SPM source directories
            let sources_dir = project_dir.join("Sources");
            let tests_dir = project_dir.join("Tests");

            if sources_dir.exists() {
                source_roots.push(sources_dir);
            } else {
                // Fallback: project root as source
                source_roots.push(project_dir.to_path_buf());
            }

            if tests_dir.exists() {
                source_roots.push(tests_dir);
            }
        }

        Ok(source_roots)
    }

    /// Build resolution rules from Package.swift
    fn build_rules_for_config(&self, config_path: &Path) -> ResolutionResult<ResolutionRules> {
        let source_roots = self.parse_package_swift(config_path)?;

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

impl ProjectResolutionProvider for SwiftProvider {
    fn language_id(&self) -> &'static str {
        "swift"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        self.is_swift_enabled(settings)
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
            // Skip non-existent config files
            if !config_path.exists() {
                continue;
            }

            let rules = self.build_rules_for_config(config_path)?;

            // Create file pattern mappings for this config
            // Map all .swift files under project directory to this config
            let project_dir = config_path.parent().unwrap_or(Path::new("."));
            let pattern = format!("{}/**/*.swift", project_dir.display());

            index.mappings.insert(pattern, config_path.clone());
            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        // Save to disk
        persistence.save("swift", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When Package.swift changes, all .swift files need re-indexing
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_package_swift_default_sources() {
        // Given a minimal Package.swift without custom paths
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path().join("Package.swift");

        let package_content = r#"// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "MyPackage",
    targets: [
        .target(name: "MyLib"),
        .testTarget(name: "MyLibTests", dependencies: ["MyLib"]),
    ]
)
"#;

        fs::write(&package_path, package_content).unwrap();

        // Create default directories
        fs::create_dir_all(temp_dir.path().join("Sources")).unwrap();
        fs::create_dir_all(temp_dir.path().join("Tests")).unwrap();

        // When parsing the Package.swift
        let provider = SwiftProvider::new();
        let roots = provider.parse_package_swift(&package_path).unwrap();

        // Then it should return default SPM source roots
        assert!(!roots.is_empty(), "Should have at least Sources directory");
        assert!(
            roots.iter().any(|r| r.ends_with("Sources")),
            "Should have Sources directory"
        );
    }

    #[test]
    fn test_parse_package_swift_custom_path() {
        // Given a Package.swift with custom path
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path().join("Package.swift");

        let package_content = r#"// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "MyPackage",
    targets: [
        .target(name: "MyLib", path: "CustomSources/MyLib"),
    ]
)
"#;

        fs::write(&package_path, package_content).unwrap();

        // When parsing the Package.swift
        let provider = SwiftProvider::new();
        let roots = provider.parse_package_swift(&package_path).unwrap();

        // Then it should use the custom path
        assert!(
            roots
                .iter()
                .any(|r| r.to_string_lossy().contains("CustomSources/MyLib")),
            "Should have custom source path"
        );
    }

    #[test]
    fn test_rebuild_cache_creates_resolution_json() {
        // Given a Package.swift and settings
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path().join("Package.swift");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        let package_content = r#"// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "MyPackage",
    targets: [
        .target(name: "MyLib"),
    ]
)
"#;

        fs::write(&package_path, package_content).unwrap();
        fs::create_dir_all(temp_dir.path().join("Sources")).unwrap();

        // Create settings with Swift config
        let settings_content = format!(
            r#"
[languages.swift]
enabled = true
config_files = ["{}"]
"#,
            package_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory to restore later
        let original_dir = std::env::current_dir().unwrap();

        // When rebuilding cache
        let provider = SwiftProvider::new();

        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        provider.rebuild_cache(&settings).unwrap();

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Then swift_resolution.json should exist
        let cache_path = codanna_dir.join("index/resolvers/swift_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should be created at {}",
            cache_path.display()
        );

        // And it should contain source roots
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("Sources") || cache_content.contains("sources"),
            "Cache should contain source root path"
        );
    }

    #[test]
    fn test_provider_language_id() {
        let provider = SwiftProvider::new();
        assert_eq!(provider.language_id(), "swift");
    }
}
