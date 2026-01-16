//! Python project configuration provider (pyproject.toml)
//!
//! Resolves Python module paths from pyproject.toml.
//! Supports multiple build backends: setuptools, Poetry, Hatch.
//!
//! The `[build-system].build-backend` field determines which `[tool.*]`
//! section is authoritative for source root detection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Build backend used by the Python project
///
/// Determines which `[tool.*]` configuration section is authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildBackend {
    /// setuptools (default if missing)
    Setuptools,
    /// Poetry (poetry-core)
    Poetry,
    /// Hatch (hatchling)
    Hatch,
    /// PDM
    Pdm,
    /// Flit
    Flit,
    /// Maturin (Rust+Python)
    Maturin,
    /// Unknown backend - use heuristics
    Unknown,
}

/// Detect build backend from pyproject.toml content
///
/// Reads `[build-system].build-backend` to determine which tool's
/// configuration is active.
fn detect_build_backend(toml: &toml::Value) -> BuildBackend {
    let backend = toml
        .get("build-system")
        .and_then(|bs| bs.get("build-backend"))
        .and_then(|v| v.as_str())
        .unwrap_or("setuptools.build_meta"); // Default if missing

    match backend {
        s if s.contains("poetry") => BuildBackend::Poetry,
        s if s.contains("hatch") => BuildBackend::Hatch,
        s if s.contains("pdm") => BuildBackend::Pdm,
        s if s.contains("flit") => BuildBackend::Flit,
        s if s.contains("maturin") => BuildBackend::Maturin,
        s if s.contains("setuptools") => BuildBackend::Setuptools,
        _ => BuildBackend::Unknown,
    }
}

use crate::config::Settings;
use crate::project_resolver::{
    ResolutionResult, Sha256Hash,
    helpers::{compute_config_shas, extract_language_config_paths, is_language_enabled},
    memo::ResolutionMemo,
    persist::{ResolutionIndex, ResolutionPersistence, ResolutionRules},
    provider::ProjectResolutionProvider,
};

/// Python-specific project configuration path (pyproject.toml)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PyProjectPath(PathBuf);

impl PyProjectPath {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

/// Information extracted from pyproject.toml
#[derive(Debug, Clone, Default)]
pub struct PyProjectInfo {
    /// Distribution name from [project.name] (for PyPI, not import)
    pub distribution_name: Option<String>,

    /// Maps source root directories to their importable package names
    /// Example: { "/project/src": ["mypackage", "utils"] }
    pub packages: HashMap<PathBuf, Vec<String>>,
}

/// Python project resolution provider
///
/// Handles pyproject.toml parsing to determine package name and source roots
/// for module path resolution.
pub struct PythonProvider {
    /// Thread-safe memoization cache for computed resolution data
    #[allow(dead_code)]
    memo: ResolutionMemo<HashMap<PyProjectPath, Sha256Hash>>,
}

impl Default for PythonProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonProvider {
    /// Create a new Python provider with empty memoization cache
    pub fn new() -> Self {
        Self {
            memo: ResolutionMemo::new(),
        }
    }

    /// Get module path for a Python source file
    ///
    /// Converts file path to module notation by stripping source root prefix.
    /// Example: /project/src/mypackage/utils/helper.py -> mypackage.utils.helper
    pub fn module_path_for_file(&self, file_path: &Path) -> Option<String> {
        let codanna_dir = Path::new(crate::init::local_dir_name());
        let persistence = ResolutionPersistence::new(codanna_dir);

        let index = persistence.load("python").ok()?;

        // Canonicalize file path to handle symlinks
        let canon_file = file_path.canonicalize().ok()?;

        // Find the config file for this source file
        let config_path = index.get_config_for_file(&canon_file)?;

        // Get the resolution rules for this config
        let rules = index.rules.get(config_path)?;

        // Extract module path from file path using source roots
        for root_path in rules.paths.keys() {
            let root = Path::new(root_path);
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

            if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                // Convert path to module notation
                // Remove .py extension and convert separators to dots
                let module_path = relative
                    .with_extension("")
                    .to_string_lossy()
                    .replace(['/', '\\'], ".");

                // Remove __init__ suffix (it's implicit in Python)
                let module_path = module_path
                    .strip_suffix(".__init__")
                    .unwrap_or(&module_path)
                    .to_string();

                return Some(module_path);
            }
        }

        None
    }

    /// Parse pyproject.toml to extract project information
    ///
    /// Detects the build backend and dispatches to the appropriate parser.
    fn parse_pyproject(&self, pyproject_path: &Path) -> ResolutionResult<PyProjectInfo> {
        use std::fs;

        let content = fs::read_to_string(pyproject_path).map_err(|e| {
            crate::project_resolver::ResolutionError::IoError {
                path: pyproject_path.to_path_buf(),
                cause: e.to_string(),
            }
        })?;

        let toml_value: toml::Value = toml::from_str(&content).map_err(|e| {
            crate::project_resolver::ResolutionError::ParseError {
                message: format!("Failed to parse pyproject.toml: {e}"),
            }
        })?;

        let backend = detect_build_backend(&toml_value);
        let project_dir = pyproject_path.parent().unwrap_or(Path::new("."));

        match backend {
            BuildBackend::Poetry => self.parse_poetry_config(&toml_value, project_dir),
            BuildBackend::Hatch => self.parse_hatch_config(&toml_value, project_dir),
            BuildBackend::Setuptools => self.parse_setuptools_config(&toml_value, project_dir),
            BuildBackend::Maturin => self.parse_maturin_config(&toml_value, project_dir),
            // PDM/Flit/Unknown fall back to heuristics
            _ => self.parse_with_heuristics(&toml_value, project_dir),
        }
    }

    /// Parse setuptools configuration from [tool.setuptools]
    ///
    /// Handles:
    /// - `[tool.setuptools.packages.find].where` and `.include`
    /// - `[tool.setuptools.package-dir]`
    fn parse_setuptools_config(
        &self,
        toml_value: &toml::Value,
        project_dir: &Path,
    ) -> ResolutionResult<PyProjectInfo> {
        let mut info = self.extract_project_metadata(toml_value);
        let mut found_config = false;

        if let Some(tool) = toml_value.get("tool") {
            if let Some(setuptools) = tool.get("setuptools") {
                // Check packages.find.where and include
                if let Some(packages) = setuptools.get("packages") {
                    if let Some(find) = packages.get("find") {
                        // Extract source roots from "where"
                        let source_roots: Vec<PathBuf> = if let Some(where_dirs) = find.get("where")
                        {
                            where_dirs
                                .as_array()
                                .map(|dirs| {
                                    dirs.iter()
                                        .filter_map(|d| d.as_str())
                                        .map(|s| project_dir.join(s))
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            // Default: project root
                            vec![project_dir.to_path_buf()]
                        };

                        // Extract import names from "include" patterns
                        let import_names: Vec<String> = if let Some(include) = find.get("include") {
                            include
                                .as_array()
                                .map(|patterns| {
                                    patterns
                                        .iter()
                                        .filter_map(|p| p.as_str())
                                        .map(|s| self.extract_package_from_pattern(s))
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            // No explicit include - use normalized distribution name
                            info.distribution_name
                                .as_ref()
                                .map(|n| vec![n.replace('-', "_")])
                                .unwrap_or_default()
                        };

                        for root in source_roots {
                            info.packages.insert(root, import_names.clone());
                            found_config = true;
                        }
                    }
                }

                // Check package-dir mapping (legacy)
                if !found_config {
                    if let Some(package_dir) = setuptools.get("package-dir") {
                        if let Some(table) = package_dir.as_table() {
                            for (pkg, dir) in table {
                                if let Some(dir_str) = dir.as_str() {
                                    let source_dir = project_dir.join(dir_str);
                                    let import_name = if pkg.is_empty() {
                                        // "" = "src" means root packages in src/
                                        info.distribution_name
                                            .as_ref()
                                            .map(|n| n.replace('-', "_"))
                                            .unwrap_or_default()
                                    } else {
                                        pkg.clone()
                                    };

                                    info.packages
                                        .entry(source_dir)
                                        .or_default()
                                        .push(import_name);
                                    found_config = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Default: check for src/ directory or use project root
        if !found_config {
            self.apply_default_source_dirs(&mut info, project_dir);
        }

        Ok(info)
    }

    /// Extract base package name from setuptools include pattern
    ///
    /// "mypackage*" -> "mypackage"
    /// "mypackage" -> "mypackage"
    fn extract_package_from_pattern(&self, pattern: &str) -> String {
        pattern
            .trim_end_matches('*')
            .trim_end_matches('.')
            .to_string()
    }

    /// Parse Poetry configuration from [tool.poetry]
    ///
    /// Handles:
    /// - `packages = [{ include = "pkg", from = "src" }]`
    ///
    /// The `include` field is the actual import name.
    fn parse_poetry_config(
        &self,
        toml_value: &toml::Value,
        project_dir: &Path,
    ) -> ResolutionResult<PyProjectInfo> {
        let mut info = self.extract_project_metadata(toml_value);
        let mut found_config = false;

        if let Some(tool) = toml_value.get("tool") {
            if let Some(poetry) = tool.get("poetry") {
                // Check packages array
                if let Some(packages) = poetry.get("packages") {
                    if let Some(pkg_array) = packages.as_array() {
                        for pkg in pkg_array {
                            // Each package is { include = "name", from = "dir" }
                            let import_name = pkg
                                .get("include")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            let source_root = pkg
                                .get("from")
                                .and_then(|v| v.as_str())
                                .map(|s| project_dir.join(s))
                                .unwrap_or_else(|| project_dir.to_path_buf());

                            if let Some(name) = import_name {
                                info.packages.entry(source_root).or_default().push(name);
                                found_config = true;
                            }
                        }
                    }
                }
            }
        }

        // Default: check for src/ directory or use project root
        if !found_config {
            self.apply_default_source_dirs(&mut info, project_dir);
        }

        Ok(info)
    }

    /// Parse Hatch configuration from [tool.hatch.build]
    ///
    /// Handles three patterns:
    /// 1. `packages = ["src/pkg"]` - explicit package paths
    /// 2. `sources = ["src"]` - source root with package discovery (black uses this)
    /// 3. `only-include = ["src"]` - include filter with package discovery
    ///
    /// Path format: "src/mypackage" -> source_root="src", import_name="mypackage"
    fn parse_hatch_config(
        &self,
        toml_value: &toml::Value,
        project_dir: &Path,
    ) -> ResolutionResult<PyProjectInfo> {
        let mut info = self.extract_project_metadata(toml_value);
        let mut found_config = false;

        if let Some(tool) = toml_value.get("tool") {
            if let Some(hatch) = tool.get("hatch") {
                if let Some(build) = hatch.get("build") {
                    if let Some(targets) = build.get("targets") {
                        if let Some(wheel) = targets.get("wheel") {
                            // Pattern 1: Explicit packages array (e.g., packages = ["src/mypackage"])
                            if let Some(packages) = wheel.get("packages") {
                                if let Some(pkg_array) = packages.as_array() {
                                    for pkg in pkg_array {
                                        if let Some(pkg_path) = pkg.as_str() {
                                            // Package path format: "src/mypackage"
                                            let (source_root, import_name) =
                                                self.parse_hatch_package_path(pkg_path);
                                            let full_path = project_dir.join(source_root);

                                            info.packages
                                                .entry(full_path)
                                                .or_default()
                                                .push(import_name.to_string());
                                            found_config = true;
                                        }
                                    }
                                }
                            }

                            // Pattern 2: sources/only-include (e.g., black uses this)
                            // sources = ["src"] means src/ is the source root, discover packages there
                            if !found_config {
                                if let Some(sources) = wheel.get("sources") {
                                    if let Some(sources_array) = sources.as_array() {
                                        for source in sources_array {
                                            if let Some(source_dir) = source.as_str() {
                                                let source_path = project_dir.join(source_dir);
                                                if source_path.exists() {
                                                    // Discover packages in source directory
                                                    let packages =
                                                        self.discover_packages_in_dir(&source_path);
                                                    if !packages.is_empty() {
                                                        info.packages.insert(source_path, packages);
                                                        found_config = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Pattern 3: only-include without sources (less common)
                            if !found_config {
                                if let Some(only_include) = wheel.get("only-include") {
                                    if let Some(include_array) = only_include.as_array() {
                                        for include in include_array {
                                            if let Some(include_dir) = include.as_str() {
                                                let include_path = project_dir.join(include_dir);
                                                if include_path.exists() {
                                                    let packages = self
                                                        .discover_packages_in_dir(&include_path);
                                                    if !packages.is_empty() {
                                                        info.packages
                                                            .insert(include_path, packages);
                                                        found_config = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Default: check for src/ directory or use project root
        if !found_config {
            self.apply_default_source_dirs(&mut info, project_dir);
        }

        Ok(info)
    }

    /// Discover Python packages in a directory
    ///
    /// A directory is a package if it contains __init__.py or is a namespace package
    /// Returns list of top-level package names found
    fn discover_packages_in_dir(&self, dir: &Path) -> Vec<String> {
        let mut packages = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check if it's a Python package (has __init__.py or is implicit namespace)
                    let init_py = path.join("__init__.py");
                    let has_py_files = std::fs::read_dir(&path)
                        .map(|entries| {
                            entries
                                .flatten()
                                .any(|e| e.path().extension().is_some_and(|ext| ext == "py"))
                        })
                        .unwrap_or(false);

                    if init_py.exists() || has_py_files {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            // Skip hidden directories and __pycache__
                            if !name.starts_with('.') && name != "__pycache__" {
                                packages.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        packages
    }

    /// Parse Hatch package path into (source_root, import_name)
    ///
    /// "src/mypackage" -> ("src", "mypackage")
    /// "mypackage" -> (".", "mypackage")
    fn parse_hatch_package_path<'a>(&self, pkg_path: &'a str) -> (&'a str, &'a str) {
        if let Some(idx) = pkg_path.rfind('/') {
            (&pkg_path[..idx], &pkg_path[idx + 1..])
        } else {
            (".", pkg_path)
        }
    }

    /// Parse Maturin configuration from [tool.maturin]
    ///
    /// Handles Rust+Python hybrid projects:
    /// - `python-source` - directory containing Python code (e.g., "python")
    /// - `python-packages` - list of package names (e.g., ["pendulum"])
    /// - `module-name` - the module name (e.g., "pydantic_core._pydantic_core")
    ///
    /// The import name is extracted from python-packages, module-name, or distribution name.
    fn parse_maturin_config(
        &self,
        toml_value: &toml::Value,
        project_dir: &Path,
    ) -> ResolutionResult<PyProjectInfo> {
        let mut info = self.extract_project_metadata(toml_value);
        let mut found_config = false;

        if let Some(tool) = toml_value.get("tool") {
            if let Some(maturin) = tool.get("maturin") {
                // Get python-source directory (explicit source root)
                let explicit_source = maturin.get("python-source").and_then(|v| v.as_str());

                // Get python-packages (explicit package names)
                let python_packages: Vec<String> = maturin
                    .get("python-packages")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                // Determine source root
                let source_root = if let Some(src) = explicit_source {
                    // Explicit python-source specified
                    project_dir.join(src)
                } else {
                    // Auto-detect: check src/ first, then project root
                    let src_dir = project_dir.join("src");
                    if src_dir.exists() {
                        src_dir
                    } else {
                        project_dir.to_path_buf()
                    }
                };

                // Determine import names
                let import_names: Vec<String> = if !python_packages.is_empty() {
                    // Use explicit python-packages
                    python_packages
                } else if let Some(module_name) =
                    maturin.get("module-name").and_then(|v| v.as_str())
                {
                    // Extract from module-name: "pydantic_core._pydantic_core" -> "pydantic_core"
                    vec![
                        module_name
                            .split('.')
                            .next()
                            .unwrap_or(module_name)
                            .to_string(),
                    ]
                } else {
                    // Fall back to normalized distribution name
                    info.distribution_name
                        .as_ref()
                        .map(|n| vec![n.replace('-', "_")])
                        .unwrap_or_default()
                };

                if !import_names.is_empty() {
                    info.packages.insert(source_root, import_names);
                    found_config = true;
                }
            }
        }

        // Default: check for python/ directory or use project root
        if !found_config {
            self.apply_default_source_dirs(&mut info, project_dir);
        }

        Ok(info)
    }

    /// Parse with heuristics for unknown or unsupported backends
    ///
    /// Falls back to convention-based detection (src/ or project root).
    fn parse_with_heuristics(
        &self,
        toml_value: &toml::Value,
        project_dir: &Path,
    ) -> ResolutionResult<PyProjectInfo> {
        let mut info = self.extract_project_metadata(toml_value);
        self.apply_default_source_dirs(&mut info, project_dir);
        Ok(info)
    }

    /// Extract common project metadata from [project] table
    fn extract_project_metadata(&self, toml_value: &toml::Value) -> PyProjectInfo {
        let mut info = PyProjectInfo::default();

        if let Some(project) = toml_value.get("project") {
            if let Some(name) = project.get("name").and_then(|v| v.as_str()) {
                info.distribution_name = Some(name.to_string());
            }
        }

        info
    }

    /// Apply default source directory detection
    ///
    /// Convention: check for src/ directory, fall back to project root.
    /// Uses normalized distribution name as expected import name.
    fn apply_default_source_dirs(&self, info: &mut PyProjectInfo, project_dir: &Path) {
        let src_dir = project_dir.join("src");
        let source_root = if src_dir.exists() {
            src_dir
        } else {
            // Flat layout: source files at project root
            project_dir.to_path_buf()
        };

        // Use normalized distribution name as expected import name
        // Convention: my-awesome-package -> my_awesome_package
        let import_names = if let Some(ref dist_name) = info.distribution_name {
            vec![dist_name.replace('-', "_")]
        } else {
            vec![]
        };

        info.packages.insert(source_root, import_names);
    }

    /// Build resolution rules from pyproject.toml
    fn build_rules_for_config(&self, config_path: &Path) -> ResolutionResult<ResolutionRules> {
        let pyproject_info = self.parse_pyproject(config_path)?;

        let paths: HashMap<String, Vec<String>> = pyproject_info
            .packages
            .into_iter()
            .map(|(root, names)| (root.to_string_lossy().to_string(), names))
            .collect();

        // Python doesn't use base_url - import paths are derived from source roots
        // The actual import names are stored in paths values
        Ok(ResolutionRules {
            base_url: None,
            paths,
        })
    }
}

impl ProjectResolutionProvider for PythonProvider {
    fn language_id(&self) -> &'static str {
        "python"
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        is_language_enabled(settings, "python")
    }

    fn config_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        extract_language_config_paths(settings, "python")
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

            // Skip configs with no package info (e.g., type-checker-only pyproject.toml)
            // These have paths with empty import_names and can't help with resolution
            let has_package_info = rules.paths.values().any(|names| !names.is_empty());
            if !has_package_info {
                continue;
            }

            // Map all .py files under project directory to this config
            let project_dir = config_path.parent().unwrap_or(Path::new("."));
            let pattern = format!("{}/**/*.py", project_dir.display());

            index.mappings.insert(pattern, config_path.clone());
            index.rules.insert(config_path.clone(), rules);
        }

        // Compute SHAs for all config files
        let shas = self.compute_shas(&config_paths)?;
        for (path, sha) in shas {
            index.hashes.insert(path, sha.0);
        }

        persistence.save("python", &index)?;

        Ok(())
    }

    fn select_affected_files(&self, _settings: &Settings) -> Vec<PathBuf> {
        // When pyproject.toml changes, all .py files need re-indexing
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_pyproject_extracts_distribution_name() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"
version = "1.0.0"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert_eq!(info.distribution_name, Some("mypackage".to_string()));
    }

    #[test]
    fn test_parse_pyproject_with_src_layout() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[tool.setuptools.packages.find]
where = ["src"]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert_eq!(info.distribution_name, Some("mypackage".to_string()));
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should have src directory"
        );
    }

    #[test]
    fn test_parse_pyproject_with_package_dir() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[tool.setuptools.package-dir]
"" = "lib"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert!(
            info.packages.keys().any(|d| d.ends_with("lib")),
            "Should have lib directory from package-dir"
        );
    }

    #[test]
    fn test_parse_pyproject_defaults_to_src_if_exists() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");
        let src_dir = temp_dir.path().join("src");

        // Create src directory
        fs::create_dir_all(&src_dir).unwrap();

        let pyproject_content = r#"[project]
name = "mypackage"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should default to src directory when it exists"
        );
    }

    #[test]
    fn test_parse_pyproject_defaults_to_project_root() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        // No src directory exists

        let pyproject_content = r#"[project]
name = "mypackage"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Should fall back to project root (flat layout)
        assert!(
            !info.packages.is_empty(),
            "Should have at least one source root"
        );
    }

    #[test]
    fn test_build_rules_has_no_base_url() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let rules = provider.build_rules_for_config(&pyproject_path).unwrap();

        // Python doesn't use base_url - import names are in paths values
        assert_eq!(rules.base_url, None);
    }

    #[test]
    #[ignore = "Requires filesystem isolation (changes cwd, conflicts with parallel tests)"]
    fn test_rebuild_cache_creates_resolution_json() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");
        let codanna_dir = temp_dir.path().join(crate::init::local_dir_name());

        let pyproject_content = r#"[project]
name = "mypackage"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        // Create settings with Python config
        let settings_content = format!(
            r#"[languages.python]
enabled = true
config_files = ["{}"]
"#,
            pyproject_path.display()
        );

        let settings: Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory
        let original_dir = std::env::current_dir().unwrap();

        // Use temp .codanna directory
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(&codanna_dir).unwrap();

        let provider = PythonProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Verify cache file exists
        let cache_path = codanna_dir.join("index/resolvers/python_resolution.json");
        assert!(
            cache_path.exists(),
            "Cache file should exist at {}",
            cache_path.display()
        );

        // Verify content
        let cache_content = fs::read_to_string(&cache_path).unwrap();
        assert!(
            cache_content.contains("mypackage"),
            "Cache should contain package name"
        );
    }

    #[test]
    fn test_provider_language_id() {
        let provider = PythonProvider::new();
        assert_eq!(provider.language_id(), "python");
    }

    #[test]
    fn test_provider_uses_helpers_for_settings() {
        let provider = PythonProvider::new();
        let settings = Settings::default();

        // Should use helper functions
        assert!(provider.is_enabled(&settings)); // Enabled by default
        assert!(provider.config_paths(&settings).is_empty()); // No config paths
    }

    // --- Backend detection tests ---

    #[test]
    fn test_detect_backend_setuptools() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "setuptools.build_meta"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Setuptools);
    }

    #[test]
    fn test_detect_backend_poetry() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "poetry.core.masonry.api"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Poetry);
    }

    #[test]
    fn test_detect_backend_hatch() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "hatchling.build"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Hatch);
    }

    #[test]
    fn test_detect_backend_pdm() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "pdm.backend"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Pdm);
    }

    #[test]
    fn test_detect_backend_flit() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "flit_core.buildapi"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Flit);
    }

    #[test]
    fn test_detect_backend_missing_defaults_to_setuptools() {
        let toml: toml::Value = toml::from_str(
            r#"
            [project]
            name = "mypackage"
            "#,
        )
        .unwrap();

        // When build-backend is missing, defaults to setuptools
        assert_eq!(detect_build_backend(&toml), BuildBackend::Setuptools);
    }

    #[test]
    fn test_detect_backend_unknown() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "some.unknown.backend"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Unknown);
    }

    // --- Poetry config parsing tests ---

    #[test]
    fn test_poetry_packages_with_from() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "poetry.core.masonry.api"

[tool.poetry]
packages = [
    { include = "mypackage", from = "src" }
]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert_eq!(info.distribution_name, Some("mypackage".to_string()));
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should extract src from poetry packages[].from"
        );
        // Check import name is extracted from "include"
        let src_packages: Vec<_> = info
            .packages
            .iter()
            .filter(|(k, _)| k.ends_with("src"))
            .flat_map(|(_, v)| v)
            .collect();
        assert!(
            src_packages.contains(&&"mypackage".to_string()),
            "Should extract import name from include field"
        );
    }

    #[test]
    fn test_poetry_packages_flat_layout() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "poetry.core.masonry.api"

[tool.poetry]
packages = [
    { include = "mypackage" }
]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // No "from" means project root
        assert!(
            !info.packages.is_empty(),
            "Should have at least one source root"
        );
        // The source root should be the project root (temp_dir)
        assert!(
            info.packages
                .keys()
                .any(|d| *d == temp_dir.path().to_path_buf()),
            "Should use project root for flat layout"
        );
    }

    #[test]
    fn test_poetry_multiple_packages() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "poetry.core.masonry.api"

[tool.poetry]
packages = [
    { include = "core", from = "src" },
    { include = "utils", from = "lib" }
]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should have src directory"
        );
        assert!(
            info.packages.keys().any(|d| d.ends_with("lib")),
            "Should have lib directory"
        );
        // Check import names are extracted
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(all_import_names.contains(&&"core".to_string()));
        assert!(all_import_names.contains(&&"utils".to_string()));
    }

    // --- Hatch config parsing tests ---

    #[test]
    fn test_hatch_packages_src_layout() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["src/mypackage"]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert_eq!(info.distribution_name, Some("mypackage".to_string()));
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should extract src from hatch packages path"
        );
        // Check import name is extracted from path
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(
            all_import_names.contains(&&"mypackage".to_string()),
            "Should extract import name from path"
        );
    }

    #[test]
    fn test_hatch_packages_multiple() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        // Like black: multiple packages in src/
        let pyproject_content = r#"[project]
name = "black"

[build-system]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["src/black", "src/blackd", "src/blib2to3"]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // All packages share src/ as source root
        assert_eq!(info.packages.len(), 1, "Should deduplicate source roots");
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should have src directory"
        );
        // Check all import names are extracted
        let src_packages: Vec<_> = info
            .packages
            .iter()
            .filter(|(k, _)| k.ends_with("src"))
            .flat_map(|(_, v)| v)
            .collect();
        assert!(src_packages.contains(&&"black".to_string()));
        assert!(src_packages.contains(&&"blackd".to_string()));
        assert!(src_packages.contains(&&"blib2to3".to_string()));
    }

    #[test]
    fn test_hatch_packages_flat_layout() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["mypackage"]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // No "/" in path means project root
        assert!(
            !info.packages.is_empty(),
            "Should have at least one source root"
        );
        // Check import name is extracted
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(all_import_names.contains(&&"mypackage".to_string()));
    }

    #[test]
    fn test_hatch_sources_pattern_discovers_packages() {
        // This tests the pattern black uses: sources = ["src"]
        // which requires discovering packages in the source directory
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");
        let src_dir = temp_dir.path().join("src");

        // Create src/ with multiple packages (like black has black/, blackd/, blib2to3/)
        fs::create_dir_all(src_dir.join("mypackage")).unwrap();
        fs::create_dir_all(src_dir.join("myutils")).unwrap();
        fs::create_dir_all(src_dir.join("mylib")).unwrap();

        // Add __init__.py to make them packages
        fs::write(src_dir.join("mypackage/__init__.py"), "").unwrap();
        fs::write(src_dir.join("myutils/__init__.py"), "").unwrap();
        fs::write(src_dir.join("mylib/__init__.py"), "").unwrap();

        let pyproject_content = r#"[project]
name = "myproject"

[build-system]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
only-include = ["src"]
sources = ["src"]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Should discover all three packages
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should have src as source root"
        );

        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(
            all_import_names.contains(&&"mypackage".to_string()),
            "Should discover mypackage"
        );
        assert!(
            all_import_names.contains(&&"myutils".to_string()),
            "Should discover myutils"
        );
        assert!(
            all_import_names.contains(&&"mylib".to_string()),
            "Should discover mylib"
        );
    }

    // --- Dispatch tests ---

    #[test]
    fn test_dispatch_uses_poetry_parser_for_poetry_backend() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        // Project with BOTH setuptools and poetry configs
        // Only poetry should be used (based on build-backend)
        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "poetry.core.masonry.api"

[tool.setuptools.packages.find]
where = ["lib"]

[tool.poetry]
packages = [
    { include = "mypackage", from = "src" }
]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Should use poetry's "src", not setuptools' "lib"
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should use poetry config (src)"
        );
        assert!(
            !info.packages.keys().any(|d| d.ends_with("lib")),
            "Should NOT use setuptools config (lib)"
        );
    }

    #[test]
    fn test_dispatch_uses_setuptools_parser_for_setuptools_backend() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        // Project with BOTH setuptools and poetry configs
        // Only setuptools should be used (based on build-backend)
        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "setuptools.build_meta"

[tool.setuptools.packages.find]
where = ["lib"]

[tool.poetry]
packages = [
    { include = "mypackage", from = "src" }
]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Should use setuptools' "lib", not poetry's "src"
        assert!(
            info.packages.keys().any(|d| d.ends_with("lib")),
            "Should use setuptools config (lib)"
        );
        assert!(
            !info.packages.keys().any(|d| d.ends_with("src")),
            "Should NOT use poetry config (src)"
        );
    }

    // --- Maturin config parsing tests ---

    #[test]
    fn test_detect_backend_maturin() {
        let toml: toml::Value = toml::from_str(
            r#"
            [build-system]
            build-backend = "maturin"
            "#,
        )
        .unwrap();

        assert_eq!(detect_build_backend(&toml), BuildBackend::Maturin);
    }

    #[test]
    fn test_maturin_with_python_source_and_module_name() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        // Like pydantic-core: python-source = "python", module-name = "pydantic_core._pydantic_core"
        let pyproject_content = r#"[project]
name = "pydantic_core"

[build-system]
build-backend = "maturin"

[tool.maturin]
python-source = "python"
module-name = "pydantic_core._pydantic_core"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        assert_eq!(info.distribution_name, Some("pydantic_core".to_string()));
        // Source root should be "python"
        assert!(
            info.packages.keys().any(|d| d.ends_with("python")),
            "Should extract python-source as source root"
        );
        // Import name should be first component of module-name
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(
            all_import_names.contains(&&"pydantic_core".to_string()),
            "Should extract first component of module-name as import name"
        );
    }

    #[test]
    fn test_maturin_without_module_name_uses_distribution_name() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        let pyproject_content = r#"[project]
name = "my-rust-lib"

[build-system]
build-backend = "maturin"

[tool.maturin]
python-source = "src"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Source root should be "src"
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should use python-source as source root"
        );
        // Import name should fall back to normalized distribution name
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(
            all_import_names.contains(&&"my_rust_lib".to_string()),
            "Should fall back to normalized distribution name"
        );
    }

    #[test]
    fn test_maturin_defaults_to_project_root() {
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");

        // Minimal maturin config without python-source
        let pyproject_content = r#"[project]
name = "mypackage"

[build-system]
build-backend = "maturin"

[tool.maturin]
module-name = "mypackage._core"
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Source root should default to project root (.)
        assert!(
            !info.packages.is_empty(),
            "Should have at least one source root"
        );
        // Import name should be first component of module-name
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(all_import_names.contains(&&"mypackage".to_string()));
    }

    #[test]
    fn test_maturin_python_packages_with_src_layout() {
        // This tests the pattern pendulum uses:
        // python-packages = ["pendulum"] with src/ layout
        let temp_dir = TempDir::new().unwrap();
        let pyproject_path = temp_dir.path().join("pyproject.toml");
        let src_dir = temp_dir.path().join("src");

        // Create src/pendulum/ directory
        fs::create_dir_all(src_dir.join("pendulum")).unwrap();
        fs::write(src_dir.join("pendulum/__init__.py"), "").unwrap();

        let pyproject_content = r#"[project]
name = "pendulum"

[build-system]
build-backend = "maturin"

[tool.maturin]
module-name = "pendulum._pendulum"
python-packages = ["pendulum"]
"#;

        fs::write(&pyproject_path, pyproject_content).unwrap();

        let provider = PythonProvider::new();
        let info = provider.parse_pyproject(&pyproject_path).unwrap();

        // Should auto-detect src/ as source root
        assert!(
            info.packages.keys().any(|d| d.ends_with("src")),
            "Should auto-detect src/ as source root"
        );
        // Import name should come from python-packages
        let all_import_names: Vec<_> = info.packages.values().flatten().collect();
        assert!(
            all_import_names.contains(&&"pendulum".to_string()),
            "Should use python-packages for import name"
        );
    }

    // --- Helper method tests ---

    #[test]
    fn test_parse_hatch_package_path() {
        let provider = PythonProvider::new();

        assert_eq!(
            provider.parse_hatch_package_path("src/mypackage"),
            ("src", "mypackage")
        );
        assert_eq!(
            provider.parse_hatch_package_path("lib/core"),
            ("lib", "core")
        );
        assert_eq!(
            provider.parse_hatch_package_path("mypackage"),
            (".", "mypackage")
        );
        // For nested paths, takes last component as package name
        assert_eq!(
            provider.parse_hatch_package_path("packages/utils/helpers"),
            ("packages/utils", "helpers")
        );
    }

    #[test]
    fn test_extract_package_from_pattern() {
        let provider = PythonProvider::new();

        assert_eq!(
            provider.extract_package_from_pattern("mypackage*"),
            "mypackage"
        );
        assert_eq!(
            provider.extract_package_from_pattern("mypackage"),
            "mypackage"
        );
        assert_eq!(
            provider.extract_package_from_pattern("mypackage.*"),
            "mypackage"
        );
    }
}
