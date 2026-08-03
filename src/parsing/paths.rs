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

/// Relative path segments of `path` below `base`, split by the OS
/// path parser.
///
/// The language-agnostic half of module derivation: behaviors apply
/// their conventions (join separator, package-file collapse,
/// source-root names) over these segments and never over path text.
/// Returns `None` when `path` is not below `base` or escapes it;
/// `Some(vec![])` when `path` equals `base`.
pub fn relative_segments(path: &Path, base: &Path) -> Option<Vec<String>> {
    let rel = path.strip_prefix(base).ok()?;
    let mut segments = Vec::new();
    for component in rel.components() {
        match component {
            std::path::Component::Normal(seg) => segments.push(seg.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(segments)
}

/// Portable-form rendering of a relative path: `Normal` components
/// joined with `/` on every platform. `None` when the path is empty
/// or carries non-`Normal` components (`./`, `..`, roots) — callers
/// keep their stored fallback for those.
pub fn portable_join(path: &Path) -> Option<String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(seg) => segments.push(seg.to_string_lossy()),
            _ => return None,
        }
    }
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("/"))
}

/// User-facing rendering of an ABSOLUTE path: the Windows verbatim
/// prefix is stripped when the path is losslessly representable in
/// legacy form (`\\?\C:\x` renders `C:\x`). Only
/// `Prefix::VerbatimDisk` simplifies; every other prefix kind and
/// every path dunce judges unrepresentable (length, reserved names,
/// trailing dots/spaces, control characters) passes through
/// unchanged. Internal comparison surfaces keep canonical paths;
/// this applies only where a path is formatted for output.
#[cfg(windows)]
pub fn render_absolute_path(path: &Path) -> &Path {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return path;
    };
    if !matches!(prefix.kind(), Prefix::VerbatimDisk(_)) {
        return path;
    }
    // dunce 1.0.5's reserved-name table lists the ASCII DOS device
    // names but not the superscript aliases; preserve those verbatim.
    let has_superscript_device = path.components().any(|component| {
        if let Component::Normal(seg) = component {
            let seg = seg.to_string_lossy();
            let stem = seg.split('.').next().unwrap_or(&seg).to_lowercase();
            (stem.starts_with("com") || stem.starts_with("lpt"))
                && matches!(&stem[3..], "\u{b9}" | "\u{b2}" | "\u{b3}")
        } else {
            false
        }
    });
    if has_superscript_device {
        return path;
    }
    dunce::simplified(path)
}

/// Non-Windows: verbatim prefixes do not exist, and a filename
/// literally spelled `\\?\...` is legal — identity by construction.
#[cfg(not(windows))]
pub fn render_absolute_path(path: &Path) -> &Path {
    path
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
    use tempfile::TempDir;

    #[test]
    fn test_normalize_relative_path() {
        let file_path = Path::new("src/foo/bar.rs");
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        let result = normalize_for_module_path(file_path, workspace_root);

        assert!(result.is_absolute());
        assert!(result.ends_with("src/foo/bar.rs"));
    }

    #[test]
    fn test_normalize_absolute_path() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();
        let file_path = workspace_root.join("src/foo/bar.rs");

        let result = normalize_for_module_path(&file_path, workspace_root);

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
    fn render_absolute_path_is_identity_off_windows() {
        // A unix filename literally spelled like a verbatim prefix is
        // legal and must never be rewritten.
        let path = Path::new(r"/tmp/\\?\literal/x.rs");
        assert_eq!(render_absolute_path(path), path);
        let plain = Path::new("/tmp/x.rs");
        assert_eq!(render_absolute_path(plain), plain);
    }

    #[cfg(windows)]
    #[test]
    fn render_absolute_path_simplifies_verbatim_disk_only() {
        assert_eq!(
            render_absolute_path(Path::new(r"\\?\C:\Users\x")),
            Path::new(r"C:\Users\x")
        );
        // Non-VerbatimDisk prefixes pass through.
        let unc = Path::new(r"\\?\UNC\server\share\x");
        assert_eq!(render_absolute_path(unc), unc);
        // Superscript DOS device aliases stay verbatim.
        let dev = Path::new("\\\\?\\C:\\x\\com\u{b9}.txt");
        assert_eq!(render_absolute_path(dev), dev);
    }

    #[test]
    fn test_relative_segments_below_base() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let path = base.join("pkg").join("util.py");

        assert_eq!(
            relative_segments(&path, base),
            Some(vec!["pkg".to_string(), "util.py".to_string()])
        );
    }

    #[test]
    fn test_relative_segments_outside_base_is_none() {
        let temp_dir = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();

        assert_eq!(
            relative_segments(&other.path().join("x.rs"), temp_dir.path()),
            None
        );
    }

    #[test]
    fn test_relative_segments_at_base_is_empty() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        assert_eq!(relative_segments(base, base), Some(vec![]));
    }

    #[test]
    fn test_relative_segments_traversal_is_none() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let path = base.join("..").join("x.rs");

        assert_eq!(relative_segments(&path, base), None);
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
