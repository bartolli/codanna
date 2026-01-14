//! Path utilities for module path computation
//!
//! Provides OS-agnostic path normalization for computing module paths from file paths.
//! All functions use `Path` APIs instead of string manipulation to handle
//! different path separators across operating systems.

use std::path::{Path, PathBuf};

/// Normalize a file path for module path computation.
///
/// Ensures the path is in a consistent format for `module_path_from_file`:
/// - If `file_path` is relative, prepends `workspace_root` to make it absolute
/// - If `file_path` is already absolute, returns it unchanged
///
/// This ensures language behaviors always receive paths in a consistent
/// coordinate system where `strip_prefix(workspace_root)` will work.
pub fn normalize_for_module_path(file_path: &Path, workspace_root: &Path) -> PathBuf {
    if file_path.is_relative() {
        workspace_root.join(file_path)
    } else {
        file_path.to_path_buf()
    }
}

/// Strip configured source roots from a path.
///
/// Attempts to strip each source root in order, returning the first match.
/// Uses `Path::strip_prefix` for OS-agnostic handling.
///
/// # Arguments
/// * `path` - The path to strip (should be relative to workspace root)
/// * `source_roots` - List of source root directories to try (e.g., `["src", "lib", "app"]`)
///
/// # Returns
/// The path with the source root stripped, or the original path if no match.
pub fn strip_source_root<'a>(path: &'a Path, source_roots: &[&str]) -> &'a Path {
    for root in source_roots {
        if let Ok(stripped) = path.strip_prefix(root) {
            return stripped;
        }
    }
    path
}

/// Strip configured source roots from a path, returning owned PathBuf.
///
/// Same as `strip_source_root` but returns an owned `PathBuf`.
pub fn strip_source_root_owned(path: &Path, source_roots: &[&str]) -> PathBuf {
    strip_source_root(path, source_roots).to_path_buf()
}

/// Strip file extension from a path string.
///
/// Extensions from the registry do NOT include the dot (e.g., "rs", "py").
/// Tries each extension in order and returns the first match.
///
/// # Arguments
/// * `path_str` - The path string to strip extension from
/// * `extensions` - List of extensions WITHOUT dots (e.g., `["rs"]`, `["py", "pyi"]`)
///
/// # Returns
/// The path with extension stripped, or original if no match.
pub fn strip_extension<'a>(path_str: &'a str, extensions: &[&str]) -> &'a str {
    for ext in extensions {
        // Build the suffix with dot (e.g., ".rs")
        let suffix = format!(".{ext}");
        if let Some(stripped) = path_str.strip_suffix(&suffix) {
            return stripped;
        }
    }
    path_str
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_relative_path() {
        let file_path = Path::new("src/foo/bar.rs");
        let workspace_root = Path::new("/home/user/workspace");

        let result = normalize_for_module_path(file_path, workspace_root);

        assert!(result.is_absolute());
        assert!(result.ends_with("src/foo/bar.rs"));
    }

    #[test]
    fn test_normalize_absolute_path() {
        let file_path = Path::new("/home/user/workspace/src/foo/bar.rs");
        let workspace_root = Path::new("/home/user/workspace");

        let result = normalize_for_module_path(file_path, workspace_root);

        assert_eq!(result, file_path);
    }

    #[test]
    fn test_strip_source_root_matches_first() {
        let path = Path::new("src/foo/bar.rs");
        let source_roots = &["src", "lib", "app"];

        let result = strip_source_root(path, source_roots);

        assert_eq!(result, Path::new("foo/bar.rs"));
    }

    #[test]
    fn test_strip_source_root_matches_second() {
        let path = Path::new("lib/utils/helper.rs");
        let source_roots = &["src", "lib", "app"];

        let result = strip_source_root(path, source_roots);

        assert_eq!(result, Path::new("utils/helper.rs"));
    }

    #[test]
    fn test_strip_source_root_no_match() {
        let path = Path::new("tests/integration.rs");
        let source_roots = &["src", "lib", "app"];

        let result = strip_source_root(path, source_roots);

        assert_eq!(result, path);
    }

    #[test]
    fn test_strip_source_root_empty_roots() {
        let path = Path::new("src/foo/bar.rs");
        let source_roots: &[&str] = &[];

        let result = strip_source_root(path, source_roots);

        assert_eq!(result, path);
    }

    #[test]
    fn test_strip_extension_simple() {
        assert_eq!(strip_extension("foo.rs", &["rs"]), "foo");
        assert_eq!(strip_extension("bar.py", &["py", "pyi"]), "bar");
    }

    #[test]
    fn test_strip_extension_compound_typescript() {
        // TypeScript declaration files - order matters, longer first
        let ts_extensions = &["d.ts", "tsx", "ts", "mts", "cts"];
        assert_eq!(strip_extension("types.d.ts", ts_extensions), "types");
        assert_eq!(strip_extension("component.tsx", ts_extensions), "component");
        assert_eq!(strip_extension("main.ts", ts_extensions), "main");
    }

    #[test]
    fn test_strip_extension_compound_php() {
        // PHP class files - order matters, longer first
        let php_extensions = &["class.php", "inc.php", "php", "inc"];
        assert_eq!(strip_extension("User.class.php", php_extensions), "User");
        assert_eq!(strip_extension("config.inc.php", php_extensions), "config");
        assert_eq!(strip_extension("index.php", php_extensions), "index");
    }

    #[test]
    fn test_strip_extension_no_match() {
        assert_eq!(strip_extension("README.md", &["rs", "py"]), "README.md");
        assert_eq!(strip_extension("no_extension", &["rs"]), "no_extension");
    }

    #[test]
    fn test_strip_extension_priority() {
        // First matching extension wins
        let extensions = &["ts", "d.ts"]; // Wrong order
        assert_eq!(strip_extension("types.d.ts", extensions), "types.d");

        // Correct order: longer extensions first
        let extensions = &["d.ts", "ts"];
        assert_eq!(strip_extension("types.d.ts", extensions), "types");
    }
}
