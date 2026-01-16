//! C# project configuration provider (.csproj)
//!
//! Resolves C# namespaces from .csproj files.
//! Extracts RootNamespace (or derives from project filename) for module path resolution.

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

/// C#-specific project configuration path (.csproj)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CsprojPath(PathBuf);

impl CsprojPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// Information extracted from .csproj file
#[derive(Debug, Clone, Default)]
pub struct CsprojInfo {
    /// RootNamespace from <RootNamespace> element (if specified)
    pub root_namespace: Option<String>,

    /// AssemblyName from <AssemblyName> element (if specified)
    pub assembly_name: Option<String>,

    /// Whether this is an SDK-style project (has Sdk attribute on Project element)
    pub is_sdk_style: bool,
}

/// C# project resolution provider
///
/// Handles .csproj parsing to determine namespace mappings.
/// C# uses dot-separated namespaces (e.g., Microsoft.EntityFrameworkCore.Internal).
pub struct CSharpProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)]
    memo: ResolutionMemo<HashMap<CsprojPath, Sha256Hash>>,
}

impl Default for CSharpProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CSharpProvider {
    /// Create a new C# provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Get namespace for a C# source file based on project configuration
    ///
    /// Combines RootNamespace with relative folder path.
    /// Example: RootNamespace=MyApp, file Controllers/UserController.cs
    /// becomes namespace MyApp.Controllers
    pub fn namespace_for_file(&self, file_path: &Path) -> Option<String> {
        let codanna_dir = Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("csharp").ok()?;

        // Canonicalize file path to handle symlinks
        let canon_file = file_path.canonicalize().ok()?;

        // Find the config file (.csproj) for this source file
        let config_path = index.get_config_for_file(&canon_file)?;

        // Get the resolution rules for this config
        let rules = index.rules.get(config_path)?;

        // Get RootNamespace from baseUrl
        let root_namespace = rules.base_url.as_ref()?;

        // Get project root (directory containing .csproj)
        for root_path in rules.paths.keys() {
            let root = Path::new(root_path);
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

            if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                // Get directory path (C# namespaces follow folder structure by convention)
                let dir_path = relative.parent()?.to_string_lossy();

                if dir_path.is_empty() {
                    // Root of project
                    return Some(root_namespace.clone());
                } else {
                    // Subfolder: RootNamespace.Folder.SubFolder
                    let namespace_suffix = dir_path.replace(['/', '\\'], ".");
                    return Some(format!("{root_namespace}.{namespace_suffix}"));
                }
            }
        }

        None
    }

    /// Parse .csproj file to extract project information
    fn parse_csproj(&self, csproj_path: &Path) -> ResolutionResult<CsprojInfo> {
        use std::fs;

        let content = fs::read_to_string(csproj_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: csproj_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        // Check for SDK-style project (has Sdk attribute on Project element)
        // Pattern: <Project Sdk="Microsoft.NET.Sdk">
        let is_sdk_style = content.contains("Sdk=\"Microsoft.NET.Sdk")
            || content.contains("Sdk=\"Microsoft.NET.Sdk.Web")
            || content.contains("Sdk=\"Microsoft.NET.Sdk.Razor")
            || content.contains("Sdk=\"Microsoft.NET.Sdk.Worker")
            || content.contains("<Sdk Name=\"Microsoft.NET.Sdk");

        // Extract RootNamespace - pattern: <RootNamespace>...</RootNamespace>
        let root_namespace = Self::extract_xml_element(&content, "RootNamespace");

        // Extract AssemblyName - pattern: <AssemblyName>...</AssemblyName>
        let assembly_name = Self::extract_xml_element(&content, "AssemblyName");

        Ok(CsprojInfo {
            root_namespace,
            assembly_name,
            is_sdk_style,
        })
    }

    /// Extract content from a simple XML element
    fn extract_xml_element(content: &str, element_name: &str) -> Option<String> {
        let open_tag = format!("<{element_name}>");
        let close_tag = format!("</{element_name}>");

        let start = content.find(&open_tag)?;
        let start = start + open_tag.len();
        let end = content[start..].find(&close_tag)?;
        let value = content[start..start + end].trim();

        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    /// Derive root namespace from project file path
    ///
    /// For SDK-style projects without explicit RootNamespace,
    /// the namespace defaults to the project filename (without .csproj extension).
    fn derive_root_namespace(&self, csproj_path: &Path, info: &CsprojInfo) -> String {
        // Priority:
        // 1. Explicit RootNamespace
        // 2. AssemblyName
        // 3. Project filename

        if let Some(ref ns) = info.root_namespace {
            return ns.clone();
        }

        if let Some(ref asm) = info.assembly_name {
            return asm.clone();
        }

        // Fallback to project filename
        csproj_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    }

    /// Build resolution rules from .csproj file
    fn build_rules_for_config(&self, config_path: &Path) -> ResolutionResult<ResolutionRules> {
        let csproj_info = self.parse_csproj(config_path)?;

        // Project root is the directory containing .csproj
        let project_root = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        // Derive root namespace
        let root_namespace = self.derive_root_namespace(config_path, &csproj_info);

        // Map project root to empty paths list (C# doesn't need prefix mappings like PHP)
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        paths.insert(project_root.to_string_lossy().to_string(), vec![]);

        Ok(ResolutionRules {
            base_url: Some(root_namespace),
            paths,
        })
    }
}

impl ProjectResolutionProvider for CSharpProvider {
    fn language_id(&self) -> &'static str {
        "csharp"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        is_language_enabled(settings, "csharp")
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        extract_language_config_paths(settings, "csharp")
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

            // Map all .cs files under project directory to this config
            for source_dir in rules.paths.keys() {
                let pattern = format!("{source_dir}/**/*.cs");
                index.mappings.insert(pattern, config_path.clone());
            }

            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        persistence.save("csharp", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When .csproj changes, all .cs files need re-indexing
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_csproj_sdk_style_with_root_namespace() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("MyProject.csproj");

        let csproj_content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <RootNamespace>MyCompany.MyProject</RootNamespace>
  </PropertyGroup>
</Project>"#;

        fs::write(&csproj_path, csproj_content).unwrap();

        let provider = CSharpProvider::new();
        let info = provider.parse_csproj(&csproj_path).unwrap();

        assert!(info.is_sdk_style);
        assert_eq!(info.root_namespace, Some("MyCompany.MyProject".to_string()));
        assert!(info.assembly_name.is_none());
    }

    #[test]
    fn test_parse_csproj_sdk_style_implicit_namespace() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("MyCompany.MyApp.csproj");

        // Minimal SDK-style project - no explicit RootNamespace
        let csproj_content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
  </PropertyGroup>
</Project>"#;

        fs::write(&csproj_path, csproj_content).unwrap();

        let provider = CSharpProvider::new();
        let info = provider.parse_csproj(&csproj_path).unwrap();

        assert!(info.is_sdk_style);
        assert!(info.root_namespace.is_none()); // Not specified
        assert!(info.assembly_name.is_none());

        // Should derive from filename
        let namespace = provider.derive_root_namespace(&csproj_path, &info);
        assert_eq!(namespace, "MyCompany.MyApp");
    }

    #[test]
    fn test_parse_csproj_with_assembly_name() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("EFCore.Abstractions.csproj");

        // EF Core pattern: AssemblyName includes suffix, RootNamespace doesn't
        let csproj_content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <AssemblyName>Microsoft.EntityFrameworkCore.Abstractions</AssemblyName>
    <RootNamespace>Microsoft.EntityFrameworkCore</RootNamespace>
  </PropertyGroup>
</Project>"#;

        fs::write(&csproj_path, csproj_content).unwrap();

        let provider = CSharpProvider::new();
        let info = provider.parse_csproj(&csproj_path).unwrap();

        assert!(info.is_sdk_style);
        assert_eq!(
            info.root_namespace,
            Some("Microsoft.EntityFrameworkCore".to_string())
        );
        assert_eq!(
            info.assembly_name,
            Some("Microsoft.EntityFrameworkCore.Abstractions".to_string())
        );

        // RootNamespace takes priority
        let namespace = provider.derive_root_namespace(&csproj_path, &info);
        assert_eq!(namespace, "Microsoft.EntityFrameworkCore");
    }

    #[test]
    fn test_parse_csproj_web_sdk() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("MyWebApp.csproj");

        let csproj_content = r#"<Project Sdk="Microsoft.NET.Sdk.Web">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
  </PropertyGroup>
</Project>"#;

        fs::write(&csproj_path, csproj_content).unwrap();

        let provider = CSharpProvider::new();
        let info = provider.parse_csproj(&csproj_path).unwrap();

        assert!(info.is_sdk_style);
    }

    #[test]
    fn test_derive_namespace_priority() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("ProjectName.csproj");

        let provider = CSharpProvider::new();

        // Case 1: RootNamespace specified - use it
        let info1 = CsprojInfo {
            root_namespace: Some("Custom.Namespace".to_string()),
            assembly_name: Some("Custom.Assembly".to_string()),
            is_sdk_style: true,
        };
        assert_eq!(
            provider.derive_root_namespace(&csproj_path, &info1),
            "Custom.Namespace"
        );

        // Case 2: Only AssemblyName - use it
        let info2 = CsprojInfo {
            root_namespace: None,
            assembly_name: Some("Custom.Assembly".to_string()),
            is_sdk_style: true,
        };
        assert_eq!(
            provider.derive_root_namespace(&csproj_path, &info2),
            "Custom.Assembly"
        );

        // Case 3: Nothing specified - use filename
        let info3 = CsprojInfo {
            root_namespace: None,
            assembly_name: None,
            is_sdk_style: true,
        };
        assert_eq!(
            provider.derive_root_namespace(&csproj_path, &info3),
            "ProjectName"
        );
    }

    #[test]
    fn test_build_rules_creates_correct_structure() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("MyApp.csproj");

        let csproj_content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <RootNamespace>MyCompany.MyApp</RootNamespace>
  </PropertyGroup>
</Project>"#;

        fs::write(&csproj_path, csproj_content).unwrap();

        let provider = CSharpProvider::new();
        let rules = provider.build_rules_for_config(&csproj_path).unwrap();

        // Should have baseUrl set to RootNamespace
        assert_eq!(rules.base_url, Some("MyCompany.MyApp".to_string()));

        // Should have one path entry (project root)
        assert_eq!(rules.paths.len(), 1);
    }

    #[test]
    fn test_provider_language_id() {
        let provider = CSharpProvider::new();
        assert_eq!(provider.language_id(), "csharp");
    }

    #[test]
    fn test_provider_uses_helpers_for_settings() {
        let provider = CSharpProvider::new();
        let settings = Settings::default();

        // Should use helper functions
        assert!(provider.is_enabled(&settings)); // Enabled by default
        assert!(provider.config_paths(&settings).is_empty()); // No config paths
    }

    #[test]
    #[ignore = "Requires filesystem isolation (changes cwd, conflicts with parallel tests)"]
    fn test_rebuild_cache_creates_resolution_json() {
        let temp_dir = TempDir::new().unwrap();
        let csproj_path = temp_dir.path().join("MyApp.csproj");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        let csproj_content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <RootNamespace>MyCompany.MyApp</RootNamespace>
  </PropertyGroup>
</Project>"#;
        fs::write(&csproj_path, csproj_content).unwrap();

        // Create settings with C# config
        let settings_content = format!(
            r#"
[languages.csharp]
enabled = true
config_files = ["{}"]
"#,
            csproj_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory
        let original_dir = std::env::current_dir().unwrap();

        // Use temp .codanna directory
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        let provider = CSharpProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Verify cache file exists
        let cache_path = codanna_dir.join("index/resolvers/csharp_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should exist at {}",
            cache_path.display()
        );

        // Verify content
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("MyCompany.MyApp"),
            "Cache should contain namespace"
        );
    }
}
