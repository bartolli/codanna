//! Shared helper utilities for project resolution providers
//!
//! Extracts common patterns from language-specific providers to reduce duplication.
//! New providers should use these helpers instead of reimplementing the same logic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::project_resolver::{
    ResolutionResult, Sha256Hash, persist::ResolutionPersistence, sha::compute_file_sha,
};

/// Extract config file paths from settings for a specific language.
///
/// Returns the list of config files defined in `[languages.<language_id>].config_files`.
/// Returns empty vec if language is not configured.
///
/// # Example
/// ```ignore
/// let paths = extract_language_config_paths(&settings, "go");
/// // Returns paths from [languages.go].config_files
/// ```
pub fn extract_language_config_paths(settings: &Settings, language_id: &str) -> Vec<PathBuf> {
    settings
        .languages
        .get(language_id)
        .map(|config| config.config_files.clone())
        .unwrap_or_default()
}

/// Check if a language is enabled in settings.
///
/// Returns true if:
/// - Language is not configured (enabled by default)
/// - Language is configured with `enabled = true`
///
/// Returns false only if explicitly set to `enabled = false`.
pub fn is_language_enabled(settings: &Settings, language_id: &str) -> bool {
    settings
        .languages
        .get(language_id)
        .map(|config| config.enabled)
        .unwrap_or(true)
}

/// Compute SHA-256 hashes for a list of config files.
///
/// Skips non-existent files gracefully (does not error).
/// Used for cache invalidation detection.
pub fn compute_config_shas(configs: &[PathBuf]) -> ResolutionResult<HashMap<PathBuf, Sha256Hash>> {
    let mut shas = HashMap::with_capacity(configs.len());
    for config in configs {
        if config.exists() {
            let sha = compute_file_sha(config)?;
            shas.insert(config.clone(), sha);
        }
    }
    Ok(shas)
}

/// Extract module path from a file path using cached resolution rules.
///
/// This is the generic implementation used by Java, Swift, Go, etc.
/// Each language provides its language_id and module path separator.
///
/// # Arguments
/// * `file_path` - Path to the source file
/// * `language_id` - Language identifier (e.g., "java", "swift", "go")
/// * `separator` - Module path separator (e.g., "." for Java/Swift, "/" for Go)
///
/// # Returns
/// Module path string if file is under a configured source root, None otherwise.
///
/// # Example
/// ```ignore
/// // Java: /project/src/main/java/com/example/Foo.java -> "com.example"
/// let module = module_for_file_generic(path, "java", ".");
///
/// // Go: /project/pkg/auth/handler.go -> "pkg/auth"
/// let module = module_for_file_generic(path, "go", "/");
/// ```
pub fn module_for_file_generic(
    file_path: &Path,
    language_id: &str,
    separator: &str,
) -> Option<String> {
    // Load cached resolution rules
    let codanna_dir = Path::new(crate::init::local_dir_name());
    let persistence = ResolutionPersistence::new(codanna_dir);

    let index = persistence.load(language_id).ok()?;

    // Canonicalize file path early to handle symlinks (e.g., /var -> /private/var on macOS)
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
            // Convert path to module notation
            // Remove filename, keep directory structure
            let module_path = relative
                .parent()?
                .to_string_lossy()
                .replace(['/', '\\'], separator);

            return Some(module_path);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LanguageConfig;

    fn create_test_settings_with_language(
        language_id: &str,
        enabled: bool,
        config_files: Vec<PathBuf>,
    ) -> Settings {
        let mut settings = Settings::default();
        let config = LanguageConfig {
            enabled,
            extensions: vec![],
            parser_options: HashMap::new(),
            config_files,
        };
        settings.languages.insert(language_id.to_string(), config);
        settings
    }

    #[test]
    fn test_extract_language_config_paths_returns_configured_paths() {
        let paths = vec![
            PathBuf::from("/project/go.mod"),
            PathBuf::from("/other/go.mod"),
        ];
        let settings = create_test_settings_with_language("go", true, paths.clone());

        let result = extract_language_config_paths(&settings, "go");

        assert_eq!(result, paths);
    }

    #[test]
    fn test_extract_language_config_paths_returns_empty_for_unconfigured() {
        let settings = Settings::default();

        let result = extract_language_config_paths(&settings, "go");

        assert!(result.is_empty());
    }

    #[test]
    fn test_is_language_enabled_returns_true_by_default() {
        let settings = Settings::default();

        assert!(is_language_enabled(&settings, "go"));
    }

    #[test]
    fn test_is_language_enabled_respects_explicit_false() {
        let settings = create_test_settings_with_language("go", false, vec![]);

        assert!(!is_language_enabled(&settings, "go"));
    }

    #[test]
    fn test_is_language_enabled_respects_explicit_true() {
        let settings = create_test_settings_with_language("go", true, vec![]);

        assert!(is_language_enabled(&settings, "go"));
    }

    #[test]
    fn test_compute_config_shas_skips_nonexistent_files() {
        let configs = vec![PathBuf::from("/definitely/does/not/exist/config.json")];

        let result = compute_config_shas(&configs);

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_compute_config_shas_computes_hash_for_existing_file() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_path_buf();

        let configs = vec![path.clone()];
        let result = compute_config_shas(&configs).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&path));
        // SHA should be non-empty hex string
        assert!(!result.get(&path).unwrap().as_str().is_empty());
    }
}
