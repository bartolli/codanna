//! jsconfig.json parser for JavaScript path alias resolution
//!
//! Handles JSONC parsing, extends chain resolution, and path alias compilation
//! for JavaScript projects (Create React App, Next.js, Vite, etc.)

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

/// Path alias resolver for JavaScript import resolution
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct PathAliasResolver {
    /// Base URL for relative path resolution
    pub baseUrl: Option<String>,
    /// Compiled path rules in priority order
    pub rules: Vec<PathRule>,
}

/// JavaScript compiler options subset for path resolution
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

/// Minimal jsconfig.json representation for path resolution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_snake_case)]
#[derive(Default)]
pub struct JsConfig {
    /// Extends another configuration file
    pub extends: Option<String>,

    /// Compiler options
    #[serde(default)]
    pub compilerOptions: CompilerOptions,
}

/// JSONC parsing helper using serde_json5 for comment and trailing comma support
pub fn parse_jsonc_jsconfig(content: &str) -> ResolutionResult<JsConfig> {
    serde_json5::from_str(content).map_err(|e| {
        ResolutionError::invalid_cache(format!(
            "Failed to parse jsconfig.json: {e}\nSuggestion: Check JSON syntax, comments, and trailing commas"
        ))
    })
}

/// Read and parse a jsconfig.json file with JSONC support
pub fn read_jsconfig(path: &Path) -> ResolutionResult<JsConfig> {
    let content =
        std::fs::read_to_string(path).map_err(|e| ResolutionError::cache_io(path.to_path_buf(), e))?;

    parse_jsonc_jsconfig(&content)
}

/// Resolve extends chain and merge configurations
///
/// Follows jsconfig.json extends resolution rules:
/// 1. Relative paths are resolved relative to the extending config
/// 2. Configurations are merged with child overriding parent
/// 3. Cycle detection prevents infinite recursion
pub fn resolve_extends_chain(
    base_path: &Path,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> ResolutionResult<JsConfig> {
    let canonical_path = base_path
        .canonicalize()
        .map_err(|e| ResolutionError::cache_io(base_path.to_path_buf(), e))?;

    // Cycle detection
    if visited.contains(&canonical_path) {
        return Err(ResolutionError::invalid_cache(format!(
            "Circular extends chain detected: {}\nSuggestion: Remove circular references in jsconfig extends",
            canonical_path.display()
        )));
    }

    visited.insert(canonical_path.clone());

    let mut config = read_jsconfig(&canonical_path)?;

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
        config = merge_jsconfig(parent_config, config);
    }

    visited.remove(&canonical_path);
    Ok(config)
}

/// Merge two jsconfig objects, with child overriding parent
fn merge_jsconfig(parent: JsConfig, child: JsConfig) -> JsConfig {
    JsConfig {
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
        // Convert glob pattern to regex
        // "@components/*" becomes "^@components/(.*)$"
        let regex_pattern = pattern.replace("*", "(.*)");
        let regex_pattern = format!(
            "^{}$",
            regex::escape(&regex_pattern).replace("\\(\\.\\*\\)", "(.*)")
        );

        let regex = regex::Regex::new(&regex_pattern).map_err(|e| {
            ResolutionError::invalid_cache(format!(
                "Invalid path pattern '{pattern}': {e}\nSuggestion: Check jsconfig.json path patterns for valid syntax"
            ))
        })?;

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
    /// Create a resolver from jsconfig compiler options
    pub fn from_jsconfig(config: &JsConfig) -> ResolutionResult<Self> {
        let mut rules = Vec::new();

        // Compile path patterns in SPECIFICITY order (most specific first)
        // More specific patterns (longer, fewer wildcards) should match before catch-all patterns
        let mut paths: Vec<_> = config.compilerOptions.paths.iter().collect();
        paths.sort_by_key(|(pattern, _)| {
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

    /// Expand a candidate path with JavaScript file extensions
    pub fn expand_extensions(&self, path: &str) -> Vec<String> {
        let mut expanded = Vec::new();

        // Add the path as-is first
        expanded.push(path.to_string());

        // Add common JavaScript extensions
        for ext in &[".js", ".jsx", ".mjs", ".cjs"] {
            expanded.push(format!("{path}{ext}"));
        }

        // Add index file variants
        for ext in &[".js", ".jsx"] {
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
    fn parse_jsconfig_with_comments() {
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

        let config = parse_jsonc_jsconfig(content).expect("Should parse JSONC with comments");

        assert_eq!(config.compilerOptions.baseUrl, Some("./src".to_string()));
        assert_eq!(config.compilerOptions.paths.len(), 1);
    }

    #[test]
    fn parse_minimal_jsconfig() {
        let content = r#"{}"#;

        let config = parse_jsonc_jsconfig(content).expect("Should parse empty config");

        assert!(config.extends.is_none());
        assert!(config.compilerOptions.baseUrl.is_none());
        assert!(config.compilerOptions.paths.is_empty());
    }

    #[test]
    fn parse_jsconfig_with_extends() {
        let content = r#"{
            "extends": "./base.json",
            "compilerOptions": {
                "baseUrl": "./src"
            }
        }"#;

        let config = parse_jsonc_jsconfig(content).expect("Should parse config with extends");

        assert_eq!(config.extends, Some("./base.json".to_string()));
        assert_eq!(config.compilerOptions.baseUrl, Some("./src".to_string()));
    }

    #[test]
    fn invalid_json_returns_error() {
        let content = r#"{ invalid json }"#;

        let result = parse_jsonc_jsconfig(content);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Failed to parse jsconfig.json"));
        assert!(error_msg.contains("Suggestion:"));
    }

    #[test]
    fn merge_jsconfig_child_overrides_parent() {
        let parent = JsConfig {
            extends: Some("parent.json".to_string()),
            compilerOptions: CompilerOptions {
                baseUrl: Some("./parent".to_string()),
                paths: HashMap::from([
                    ("@parent/*".to_string(), vec!["parent/*".to_string()]),
                    ("@common/*".to_string(), vec!["parent/common/*".to_string()]),
                ]),
            },
        };

        let child = JsConfig {
            extends: Some("child.json".to_string()),
            compilerOptions: CompilerOptions {
                baseUrl: Some("./child".to_string()),
                paths: HashMap::from([
                    ("@child/*".to_string(), vec!["child/*".to_string()]),
                    ("@common/*".to_string(), vec!["child/common/*".to_string()]),
                ]),
            },
        };

        let merged = merge_jsconfig(parent, child);

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
    fn path_rule_resolves_wildcards() {
        let rule =
            PathRule::new("@components/*".to_string(), vec!["src/components/*".to_string()])
                .expect("Should create rule");

        let result = rule.try_resolve("@components/Button");
        assert_eq!(result, Some("src/components/Button".to_string()));

        let result = rule.try_resolve("@components/forms/Input");
        assert_eq!(result, Some("src/components/forms/Input".to_string()));

        let result = rule.try_resolve("@utils/format");
        assert!(result.is_none());
    }

    #[test]
    fn path_alias_resolver_with_base_url() {
        let config = JsConfig {
            extends: None,
            compilerOptions: CompilerOptions {
                baseUrl: Some("./src".to_string()),
                paths: HashMap::from([("@/*".to_string(), vec!["*".to_string()])]),
            },
        };

        let resolver = PathAliasResolver::from_jsconfig(&config).expect("Should create resolver");
        let resolved = resolver.resolve_import("@/components/Button");

        assert_eq!(resolved, vec!["./src/components/Button".to_string()]);
    }

    #[test]
    fn expand_javascript_extensions() {
        let resolver = PathAliasResolver {
            baseUrl: Some("./src".to_string()),
            rules: vec![],
        };

        let base_path = "components/Button";
        let expanded = resolver.expand_extensions(base_path);

        assert!(expanded.contains(&"components/Button".to_string()));
        assert!(expanded.contains(&"components/Button.js".to_string()));
        assert!(expanded.contains(&"components/Button.jsx".to_string()));
        assert!(expanded.contains(&"components/Button.mjs".to_string()));
        assert!(expanded.contains(&"components/Button/index.js".to_string()));
        assert!(expanded.contains(&"components/Button/index.jsx".to_string()));
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
