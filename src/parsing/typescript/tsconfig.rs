//! TypeScript tsconfig.json parsing and resolution logic
//!
//! Handles JSONC parsing, extends chain resolution, and path alias compilation
//! following Sprint 1 requirements for basic TypeScript support.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::project_resolver::{ResolutionError, ResolutionResult};

/// Compiled path rule for efficient pattern matching
#[derive(Debug)]
pub struct PathRule {
    /// Original pattern (e.g., "@components/*")
    pub pattern: String,
    /// Target paths (e.g., ["src/components/*"])
    pub targets: Vec<String>,
    /// Compiled regex for pattern matching
    regex: regex::Regex,
    /// Substitution template for replacements
    substitution_template: String,
}

/// Path alias resolver for TypeScript import resolution
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct PathAliasResolver {
    /// Base URL for relative path resolution
    pub baseUrl: Option<String>,
    /// Compiled path rules in priority order
    pub rules: Vec<PathRule>,
}

/// TypeScript compiler options subset for path resolution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_snake_case)]
#[derive(Default)]
pub struct CompilerOptions {
    /// Base URL for module resolution
    #[serde(rename = "baseUrl")]
    pub baseUrl: Option<String>,

    /// Path mapping for module resolution
    #[serde(default)]
    pub paths: HashMap<String, Vec<String>>,
}

/// Minimal tsconfig.json representation for path resolution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_snake_case)]
#[derive(Default)]
pub struct TsConfig {
    /// Extends another configuration file
    pub extends: Option<String>,

    /// Compiler options
    #[serde(default)]
    pub compilerOptions: CompilerOptions,
}

/// JSONC parsing helper using serde_json5 for comment and trailing comma support
pub fn parse_jsonc_tsconfig(content: &str) -> ResolutionResult<TsConfig> {
    serde_json5::from_str(content)
        .map_err(|e| ResolutionError::invalid_cache(
            format!("Failed to parse tsconfig.json: {e}\nSuggestion: Check JSON syntax, comments, and trailing commas")
        ))
}

/// Read and parse a tsconfig.json file with JSONC support
pub fn read_tsconfig(path: &Path) -> ResolutionResult<TsConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ResolutionError::cache_io(path.to_path_buf(), e))?;

    parse_jsonc_tsconfig(&content)
}

/// Resolve extends chain and merge configurations
///
/// Follows TypeScript's extends resolution rules:
/// 1. Relative paths are resolved relative to the extending config
/// 2. Configurations are merged with child overriding parent
/// 3. Cycle detection prevents infinite recursion
pub fn resolve_extends_chain(
    base_path: &Path,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> ResolutionResult<TsConfig> {
    let canonical_path = base_path
        .canonicalize()
        .map_err(|e| ResolutionError::cache_io(base_path.to_path_buf(), e))?;

    // Cycle detection
    if visited.contains(&canonical_path) {
        return Err(ResolutionError::invalid_cache(format!(
            "Circular extends chain detected: {}\nSuggestion: Remove circular references in tsconfig extends",
            canonical_path.display()
        )));
    }

    visited.insert(canonical_path.clone());

    let mut config = read_tsconfig(&canonical_path)?;

    // If this config extends another, resolve the parent first
    if let Some(extends_path) = &config.extends {
        let parent_path = if Path::new(extends_path).is_absolute() {
            PathBuf::from(extends_path)
        } else {
            canonical_path
                .parent()
                .ok_or_else(|| {
                    ResolutionError::invalid_cache(format!(
                        "Cannot resolve parent directory for: {}",
                        canonical_path.display()
                    ))
                })?
                .join(extends_path)
        };

        // Add .json extension if not present
        let parent_path = if parent_path.extension().is_none() {
            parent_path.with_extension("json")
        } else {
            parent_path
        };

        // Recursively resolve parent
        let parent_config = resolve_extends_chain(&parent_path, visited)?;

        // Merge parent into child (child overrides parent)
        config = merge_tsconfig(parent_config, config);
    }

    visited.remove(&canonical_path);
    Ok(config)
}

/// Merge two tsconfig objects, with child overriding parent
fn merge_tsconfig(parent: TsConfig, child: TsConfig) -> TsConfig {
    TsConfig {
        // Child extends takes precedence (but we don't chain extends)
        extends: child.extends,
        compilerOptions: CompilerOptions {
            // Child baseUrl overrides parent
            baseUrl: child
                .compilerOptions
                .baseUrl
                .or(parent.compilerOptions.baseUrl),
            // Merge paths with child taking precedence
            paths: {
                let mut merged = parent.compilerOptions.paths;
                merged.extend(child.compilerOptions.paths);
                merged
            },
        },
    }
}

impl PathRule {
    /// Create a new path rule from pattern and targets
    pub fn new(pattern: String, targets: Vec<String>) -> ResolutionResult<Self> {
        // Convert TypeScript glob pattern to regex
        // "@components/*" becomes "^@components/(.*)$"
        let regex_pattern = pattern.replace("*", "(.*)");
        let regex_pattern = format!(
            "^{}$",
            regex::escape(&regex_pattern).replace("\\(\\.\\*\\)", "(.*)")
        );

        let regex = regex::Regex::new(&regex_pattern)
            .map_err(|e| ResolutionError::invalid_cache(
                format!("Invalid path pattern '{pattern}': {e}\nSuggestion: Check tsconfig.json path patterns for valid syntax")
            ))?;

        // Create substitution template
        // "src/components/*" becomes "src/components/$1"
        let substitution_template = targets
            .first()
            .ok_or_else(|| {
                ResolutionError::invalid_cache(format!(
                    "Path pattern '{pattern}' has no targets\nSuggestion: Add at least one target path"
                ))
            })?
            .replace("*", "$1");

        Ok(Self {
            pattern,
            targets,
            regex,
            substitution_template,
        })
    }

    /// Try to match an import specifier against this rule
    pub fn try_resolve(&self, specifier: &str) -> Option<String> {
        if let Some(captures) = self.regex.captures(specifier) {
            let mut result = self.substitution_template.clone();
            if let Some(captured) = captures.get(1) {
                result = result.replace("$1", captured.as_str());
            }
            Some(result)
        } else {
            None
        }
    }
}

impl PathAliasResolver {
    /// Create a resolver from tsconfig compiler options
    pub fn from_tsconfig(config: &TsConfig) -> ResolutionResult<Self> {
        let mut rules = Vec::new();

        // Compile path patterns in SPECIFICITY order (most specific first)
        // More specific patterns (longer, fewer wildcards) should match before catch-all patterns
        // Example: "@/registry/*" should be tried before "@/*"
        let mut paths: Vec<_> = config.compilerOptions.paths.iter().collect();
        paths.sort_by_key(|(pattern, _)| {
            // Sort by:
            // 1. Length (descending) - longer patterns are more specific
            // 2. Number of wildcards (ascending) - fewer wildcards are more specific
            let wildcard_count = pattern.matches('*').count();
            (-(pattern.len() as isize), wildcard_count)
        });

        for (pattern, targets) in paths {
            let rule = PathRule::new(pattern.clone(), targets.clone())?;
            rules.push(rule);
        }

        Ok(Self {
            baseUrl: config.compilerOptions.baseUrl.clone(),
            rules,
        })
    }

    /// Resolve an import specifier to possible file paths
    pub fn resolve_import(&self, specifier: &str) -> Vec<String> {
        let mut candidates = Vec::new();

        // Try each rule in order
        for rule in &self.rules {
            if let Some(resolved) = rule.try_resolve(specifier) {
                // Apply baseUrl if present
                let final_path = if let Some(ref base) = self.baseUrl {
                    if base == "." {
                        resolved
                    } else {
                        format!("{}/{}", base.trim_end_matches('/'), resolved)
                    }
                } else {
                    resolved
                };
                candidates.push(final_path);
            }
        }

        candidates
    }

    /// Expand a candidate path with TypeScript file extensions
    pub fn expand_extensions(&self, path: &str) -> Vec<String> {
        let mut expanded = Vec::new();

        // Add the path as-is first
        expanded.push(path.to_string());

        // Add common TypeScript extensions
        for ext in &[".ts", ".tsx", ".d.ts"] {
            expanded.push(format!("{path}{ext}"));
        }

        // Add index file variants
        for ext in &[".ts", ".tsx"] {
            expanded.push(format!("{path}/index{ext}"));
        }

        expanded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_real_project_root_tsconfig() {
        // Test with the actual project root tsconfig.json
        let tsconfig_path = Path::new("tsconfig.json");

        if !tsconfig_path.exists() {
            println!("Skipping test - no tsconfig.json in project root");
            return;
        }

        let config = read_tsconfig(tsconfig_path).expect("Should parse project root tsconfig.json");

        // Debug: print what we actually parsed
        println!("DEBUG: Parsed config: {config:#?}");

        // Verify the actual content from the real file
        assert_eq!(config.compilerOptions.baseUrl, Some(".".to_string()));
        assert_eq!(config.compilerOptions.paths.len(), 2);

        println!("✓ Parsed project root tsconfig.json:");
        println!("  baseUrl: {:?}", config.compilerOptions.baseUrl);
        println!("  paths: {:#?}", config.compilerOptions.paths);
    }

    #[test]
    fn parse_tsconfig_with_comments() {
        let content = r#"{
            // Base configuration
            "compilerOptions": {
                "baseUrl": "./src", // Source directory
                "paths": {
                    /* Path mappings */
                    "@utils/*": ["utils/*"], // Utility modules
                }
            }
        }"#;

        let config = parse_jsonc_tsconfig(content).expect("Should parse JSONC with comments");

        assert_eq!(config.compilerOptions.baseUrl, Some("./src".to_string()));
        assert_eq!(config.compilerOptions.paths.len(), 1);
    }

    #[test]
    fn parse_minimal_tsconfig() {
        let content = r#"{}"#;

        let config = parse_jsonc_tsconfig(content).expect("Should parse empty config");

        assert!(config.extends.is_none());
        assert!(config.compilerOptions.baseUrl.is_none());
        assert!(config.compilerOptions.paths.is_empty());
    }

    #[test]
    fn parse_tsconfig_with_extends() {
        let content = r#"{
            "extends": "./base.json",
            "compilerOptions": {
                "baseUrl": "./src"
            }
        }"#;

        let config = parse_jsonc_tsconfig(content).expect("Should parse config with extends");

        assert_eq!(config.extends, Some("./base.json".to_string()));
        assert_eq!(config.compilerOptions.baseUrl, Some("./src".to_string()));
    }

    #[test]
    fn invalid_json_returns_error() {
        let content = r#"{ invalid json }"#;

        let result = parse_jsonc_tsconfig(content);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Failed to parse tsconfig.json"));
        assert!(error_msg.contains("Suggestion:"));
    }

    #[test]
    fn read_example_tsconfig_from_file() {
        // Test with our example TypeScript configuration
        let tsconfig_path = Path::new("examples/typescript/tsconfig.json");

        if !tsconfig_path.exists() {
            println!("Skipping test - no example tsconfig.json");
            return;
        }

        let config = read_tsconfig(tsconfig_path).expect("Should read example tsconfig from file");

        // Verify the example file content
        assert_eq!(config.compilerOptions.baseUrl, Some("./src".to_string()));
        assert_eq!(config.compilerOptions.paths.len(), 3);
        assert!(config.compilerOptions.paths.contains_key("@components/*"));
        assert!(config.compilerOptions.paths.contains_key("@utils/*"));
        assert!(config.compilerOptions.paths.contains_key("@types/*"));

        println!("✓ Parsed examples/typescript/tsconfig.json:");
        println!("  baseUrl: {:?}", config.compilerOptions.baseUrl);
        println!("  paths: {:#?}", config.compilerOptions.paths);
    }

    #[test]
    fn read_nonexistent_file_returns_error() {
        let nonexistent = Path::new("/does/not/exist/tsconfig.json");

        let result = read_tsconfig(nonexistent);

        assert!(result.is_err());
        // Should be a cache IO error
        let error = result.unwrap_err();
        matches!(error, ResolutionError::CacheIo { .. });
    }

    #[test]
    fn resolve_real_extends_chain() {
        // Test with our example extends chain: packages/web extends base
        let child_path = Path::new("examples/typescript/packages/web/tsconfig.json");
        let parent_path = Path::new("examples/typescript/tsconfig.json");

        if !child_path.exists() || !parent_path.exists() {
            println!("Skipping test - example extends chain files don't exist");
            return;
        }

        let mut visited = std::collections::HashSet::new();
        let merged = resolve_extends_chain(child_path, &mut visited)
            .expect("Should resolve real extends chain");

        // Child baseUrl should override parent
        assert_eq!(merged.compilerOptions.baseUrl, Some("./src".to_string()));

        // Should have paths from both parent and child
        assert!(merged.compilerOptions.paths.len() >= 2);

        // Parent paths should be present
        assert!(merged.compilerOptions.paths.contains_key("@components/*"));
        assert!(merged.compilerOptions.paths.contains_key("@utils/*"));
        assert!(merged.compilerOptions.paths.contains_key("@types/*"));

        // Child paths should be present
        assert!(merged.compilerOptions.paths.contains_key("@web/*"));
        assert!(merged.compilerOptions.paths.contains_key("@api/*"));

        println!("✓ Resolved real extends chain:");
        println!("  baseUrl: {:?}", merged.compilerOptions.baseUrl);
        println!("  merged paths: {:#?}", merged.compilerOptions.paths);
    }

    #[test]
    fn merge_tsconfig_child_overrides_parent() {
        let parent = TsConfig {
            extends: Some("parent.json".to_string()),
            compilerOptions: CompilerOptions {
                baseUrl: Some("./parent".to_string()),
                paths: HashMap::from([
                    ("@parent/*".to_string(), vec!["parent/*".to_string()]),
                    ("@common/*".to_string(), vec!["parent/common/*".to_string()]),
                ]),
            },
        };

        let child = TsConfig {
            extends: Some("child.json".to_string()),
            compilerOptions: CompilerOptions {
                baseUrl: Some("./child".to_string()),
                paths: HashMap::from([
                    ("@child/*".to_string(), vec!["child/*".to_string()]),
                    ("@common/*".to_string(), vec!["child/common/*".to_string()]),
                ]),
            },
        };

        let merged = merge_tsconfig(parent, child);

        // Child values should take precedence
        assert_eq!(merged.extends, Some("child.json".to_string()));
        assert_eq!(merged.compilerOptions.baseUrl, Some("./child".to_string()));

        // Child should override parent for @common/*
        assert_eq!(
            merged.compilerOptions.paths.get("@common/*"),
            Some(&vec!["child/common/*".to_string()])
        );

        // Parent-only paths should be preserved
        assert!(merged.compilerOptions.paths.contains_key("@parent/*"));

        // Child-only paths should be present
        assert!(merged.compilerOptions.paths.contains_key("@child/*"));
    }

    #[test]
    fn resolve_path_aliases_with_real_tsconfig() {
        // Test path alias resolution with our real example tsconfig
        let tsconfig_path = Path::new("examples/typescript/tsconfig.json");

        if !tsconfig_path.exists() {
            println!("Skipping test - no example tsconfig.json");
            return;
        }

        let config = read_tsconfig(tsconfig_path).expect("Should read example tsconfig");
        let resolver = PathAliasResolver::from_tsconfig(&config).expect("Should create resolver");

        println!(
            "DEBUG: Created resolver with {} rules",
            resolver.rules.len()
        );
        println!("DEBUG: Base URL: {:?}", resolver.baseUrl);
        for rule in &resolver.rules {
            println!("DEBUG: Rule: {} → {:?}", rule.pattern, rule.targets);
        }

        // Test real patterns from our example file
        let test_cases = [
            ("@components/Button", "components/Button"), // Should match @components/*
            ("@utils/format", "utils/format"),           // Should match @utils/*
            ("@types/User", "types/User"),               // Should match @types/*
            ("regular/import", ""),                      // Should not match any pattern
        ];

        for (import_specifier, expected_path) in test_cases {
            let resolved = resolver.resolve_import(import_specifier);

            println!("Testing: {import_specifier} → Expected: {expected_path} → Got: {resolved:?}");

            if expected_path.is_empty() {
                assert!(
                    resolved.is_empty(),
                    "Should not resolve regular imports: {import_specifier}"
                );
            } else {
                assert!(
                    !resolved.is_empty(),
                    "Should resolve alias: {import_specifier}"
                );

                // With baseUrl "./src", expect "./src/components/Button"
                let expected_full = format!("./src/{expected_path}");
                assert!(
                    resolved.contains(&expected_full),
                    "Should contain expected path: {expected_full} in {resolved:?}"
                );
            }
        }
    }

    #[test]
    fn resolve_with_extends_chain_real_files() {
        // Test with the child config that extends the parent
        let child_path = Path::new("examples/typescript/packages/web/tsconfig.json");

        if !child_path.exists() {
            println!("Skipping test - no example extends chain");
            return;
        }

        let mut visited = std::collections::HashSet::new();
        let merged_config =
            resolve_extends_chain(child_path, &mut visited).expect("Should resolve extends chain");

        let resolver = PathAliasResolver::from_tsconfig(&merged_config)
            .expect("Should create resolver from merged config");

        println!("DEBUG: Merged resolver has {} rules", resolver.rules.len());

        // Should have paths from both parent and child
        let test_cases = [
            ("@components/Header", "components/Header"), // From parent
            ("@utils/api", "utils/api"),                 // From parent
            ("@web/Layout", "web/Layout"),               // From child
            ("@api/client", "api/client"),               // From child
        ];

        for (import_specifier, expected_path) in test_cases {
            let resolved = resolver.resolve_import(import_specifier);

            println!(
                "Extends test: {import_specifier} → Expected: {expected_path} → Got: {resolved:?}"
            );

            assert!(
                !resolved.is_empty(),
                "Should resolve merged alias: {import_specifier}"
            );

            // With baseUrl "./src", expect "./src/components/Header"
            let expected_full = format!("./src/{expected_path}");
            assert!(
                resolved.contains(&expected_full),
                "Should contain merged path: {expected_full} in {resolved:?}"
            );
        }
    }

    #[test]
    fn expand_typescript_extensions() {
        let resolver = PathAliasResolver {
            baseUrl: Some("./src".to_string()),
            rules: vec![],
        };

        let base_path = "components/Button";
        let expanded = resolver.expand_extensions(base_path);

        println!("Extension expansion: {base_path} → {expanded:?}");

        // Should include the base path and common TypeScript extensions
        assert!(expanded.contains(&"components/Button".to_string()));
        assert!(expanded.contains(&"components/Button.ts".to_string()));
        assert!(expanded.contains(&"components/Button.tsx".to_string()));
        assert!(expanded.contains(&"components/Button.d.ts".to_string()));
        assert!(expanded.contains(&"components/Button/index.ts".to_string()));
        assert!(expanded.contains(&"components/Button/index.tsx".to_string()));

        println!("✓ Extension expansion working correctly");
    }

    #[test]
    fn detect_circular_extends() {
        let temp_dir = TempDir::new().unwrap();

        // Create circular reference: a -> b -> a
        let a_content = r#"{ "extends": "./b.json" }"#;
        let b_content = r#"{ "extends": "./a.json" }"#;

        let a_path = temp_dir.path().join("a.json");
        let b_path = temp_dir.path().join("b.json");

        fs::write(&a_path, a_content).unwrap();
        fs::write(&b_path, b_content).unwrap();

        let mut visited = std::collections::HashSet::new();
        let result = resolve_extends_chain(&a_path, &mut visited);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Circular extends chain detected"));
        assert!(error_msg.contains("Suggestion:"));
    }
}
