//! Shared fixture support for CLI integration tests.

use std::path::Path;

/// A TOML string literal for `path`, serialization-grade escaped
/// (quotes included). Hand-formatted interpolation breaks on Windows:
/// native separators in a TOML basic string parse as escape sequences.
pub fn toml_path_literal(path: &Path) -> String {
    toml::Value::from(path.to_str().expect("path is valid UTF-8")).to_string()
}
