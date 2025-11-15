//! グローバル初期化モジュール
//!
//! Codannaのグローバルディレクトリ構造とプロジェクトレジストリを管理します。
//!
//! # 管理対象
//!
//! - FastEmbed用のグローバルモデルディレクトリ
//! - プロジェクトレジストリ
//! - モデルキャッシュ用のシンボリックリンク作成
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::init::{init_global_dirs, global_dir, models_dir};
//!
//! // グローバルディレクトリを初期化
//! init_global_dirs().expect("初期化に失敗しました");
//!
//! // グローバルディレクトリのパスを取得
//! let dir = global_dir();
//! println!("グローバルディレクトリ: {:?}", dir);
//!
//! // モデルディレクトリのパスを取得
//! let models = models_dir();
//! println!("モデルディレクトリ: {:?}", models);
//! ```

use crate::error::IndexError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// Configurable directory names for testing
// Change these to production values when ready
#[cfg(test)]
const GLOBAL_DIR_NAME: &str = ".codanna-test";
#[cfg(test)]
const LOCAL_DIR_NAME: &str = ".codanna-test-local";
#[cfg(test)]
const FASTEMBED_CACHE_NAME: &str = ".fastembed_cache";

#[cfg(not(test))]
const GLOBAL_DIR_NAME: &str = ".codanna";
#[cfg(not(test))]
const LOCAL_DIR_NAME: &str = ".codanna";
#[cfg(not(test))]
const FASTEMBED_CACHE_NAME: &str = ".fastembed_cache";

// Global directory cache
static GLOBAL_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Get the global Codanna directory
/// Returns ~/.codanna-dev (or test variant) on Unix-like systems
pub fn global_dir() -> PathBuf {
    GLOBAL_DIR
        .get_or_init(|| {
            dirs::home_dir()
                .expect("Failed to determine home directory")
                .join(GLOBAL_DIR_NAME)
        })
        .clone()
}

/// Get the models cache directory
/// Returns ~/.codanna-dev/models/
pub fn models_dir() -> PathBuf {
    global_dir().join("models")
}

/// Get the projects registry file
/// Returns ~/.codanna-dev/projects.json
pub fn projects_file() -> PathBuf {
    global_dir().join("projects.json")
}

/// Get the local project directory name
/// Returns the name used for local project config
pub fn local_dir_name() -> &'static str {
    LOCAL_DIR_NAME
}

/// Get the FastEmbed cache directory name
/// Returns the name used for the cache symlink
/// NOTE: FastEmbed library is hardcoded to use ".fastembed_cache"
/// so we must use this exact name for the symlink to work
pub fn fastembed_cache_name() -> &'static str {
    FASTEMBED_CACHE_NAME
}

/// Initialize global directory structure
pub fn init_global_dirs() -> Result<(), std::io::Error> {
    let global = global_dir();
    let models = models_dir();

    // Check and create global directory
    if !global.exists() {
        std::fs::create_dir(&global)?;
        println!("Created global directory: {}", global.display());
    } else {
        println!("Using existing global directory: {}", global.display());
    }

    // Check and create models directory
    if !models.exists() {
        std::fs::create_dir(&models)?;
        println!("Created models directory: {}", models.display());
    } else {
        println!("Using existing models directory: {}", models.display());
    }

    // Initialize profile infrastructure
    init_profile_infrastructure()?;

    Ok(())
}

/// Initialize profile system infrastructure
/// Creates ~/.codanna/providers.json if it doesn't exist
pub fn init_profile_infrastructure() -> Result<(), std::io::Error> {
    let providers_file = global_dir().join("providers.json");

    // Create empty providers registry if it doesn't exist
    if !providers_file.exists() {
        let empty_registry = serde_json::json!({
            "version": 1,
            "providers": {}
        });

        let content =
            serde_json::to_string_pretty(&empty_registry).map_err(std::io::Error::other)?;

        std::fs::write(&providers_file, content)?;
        println!("Created provider registry: {}", providers_file.display());
    }

    Ok(())
}

/// Create symlink from local FastEmbed cache to global models
/// Note: With fastembed 5.0+, we use with_cache_dir() API instead of relying on symlinks
/// This function is kept for backward compatibility and cleanup of old setups
pub fn create_fastembed_symlink() -> Result<(), std::io::Error> {
    let local_cache = PathBuf::from(fastembed_cache_name());
    let global_models = models_dir();

    // Check if symlink already exists and is correct
    if local_cache.exists() {
        if local_cache.is_symlink() {
            let target = std::fs::read_link(&local_cache)?;
            if target == global_models {
                // Check if the model actually exists at the target location
                // Look for the all-MiniLM-L6-v2 model which is the default
                let model_dir = global_models.join("models--Qdrant--all-MiniLM-L6-v2-onnx");
                if model_dir.exists() && model_dir.is_dir() {
                    println!(
                        "Symlink already exists: {} -> {} (model verified)",
                        local_cache.display(),
                        global_models.display()
                    );
                    return Ok(());
                } else {
                    // Symlink is correct but model doesn't exist
                    // Don't remove symlink, just inform that model will be downloaded
                    println!(
                        "Symlink exists but model not found. Model will be downloaded on first use."
                    );
                    return Ok(());
                }
            }
            // Remove incorrect symlink
            std::fs::remove_file(&local_cache)?;
        } else {
            // Real directory exists, don't delete user data
            eprintln!(
                "Warning: {} exists and is not a symlink",
                local_cache.display()
            );
            eprintln!("         Models will be downloaded locally");
            return Ok(());
        }
    }

    // Create symlink
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&global_models, &local_cache)?;
        println!(
            "Created symlink: {} -> {}",
            local_cache.display(),
            global_models.display()
        );
    }

    #[cfg(windows)]
    {
        // Windows requires different handling
        std::os::windows::fs::symlink_dir(&global_models, &local_cache)?;
        println!(
            "Created symlink: {} -> {}",
            local_cache.display(),
            global_models.display()
        );
    }

    Ok(())
}

// Project Registry implementation

/// Type-safe project identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(String);

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectId {
    /// Create a new random project ID using SHA-256 of random data
    pub fn new() -> Self {
        use rand::Rng;
        use sha2::{Digest, Sha256};

        // Generate 16 random bytes (128 bits, like UUID v4)
        let mut rng = rand::rng();
        let random_bytes: [u8; 16] = rng.random();

        // Hash to get a unique ID
        let mut hasher = Sha256::new();
        hasher.update(random_bytes);
        let result = hasher.finalize();

        // Take first 32 chars of hex for a shorter ID (still 128 bits of entropy)
        Self(format!("{result:x}")[..32].to_string())
    }

    /// Create from existing string (for deserialization)
    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    /// Get the ID as a string reference
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a registered project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// Absolute path to the project root
    pub path: PathBuf,
    /// Project name (extracted from path)
    pub name: String,
    /// Number of symbols indexed
    pub symbol_count: u32,
    /// Number of files indexed
    pub file_count: u32,
    /// Last modification timestamp (UTC)
    pub last_modified: u64,
    /// Number of documents in index
    pub doc_count: u64,
}

/// Registry schema for all indexed projects
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectRegistry {
    /// Schema version for future migrations
    version: u32,
    /// Map from UUID to project information
    projects: HashMap<String, ProjectInfo>,
    /// Optional default project UUID
    default_project: Option<String>,
}

impl ProjectRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            version: 1,
            projects: HashMap::new(),
            default_project: None,
        }
    }

    /// Load the registry from disk
    pub fn load() -> Result<Self, IndexError> {
        let path = projects_file();
        if !path.exists() {
            // Return empty registry if file doesn't exist
            return Ok(Self::new());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| IndexError::FileRead {
            path: path.clone(),
            source: e,
        })?;

        serde_json::from_str(&content).map_err(|e| {
            IndexError::General(format!(
                "Failed to parse project registry: {e}\nSuggestion: Back up and delete {}",
                path.display()
            ))
        })
    }

    /// Save the registry to disk
    pub fn save(&self) -> Result<(), IndexError> {
        let path = projects_file();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| IndexError::FileWrite {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let content = serde_json::to_string_pretty(&self).map_err(|e| {
            IndexError::General(format!("Failed to serialize project registry: {e}"))
        })?;

        std::fs::write(&path, content).map_err(|e| IndexError::FileWrite { path, source: e })
    }

    /// Register a new project - returns the generated project ID
    pub fn register_project(project_path: &Path) -> Result<String, IndexError> {
        let project_id = ProjectId::new();
        let project_info = Self::create_project_info(project_path);

        let mut registry = Self::load().unwrap_or_else(|_| Self::new());
        registry.add_project(project_id.as_str(), project_info);
        registry.save()?;

        Ok(project_id.to_string())
    }

    /// Register or update a project - returns the project ID (existing or new)
    pub fn register_or_update_project(project_path: &Path) -> Result<String, IndexError> {
        let mut registry = Self::load().unwrap_or_else(|_| Self::new());

        // Canonicalize the input path for consistent comparison
        let canonical_input = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());

        // Check if project already exists by path
        if let Some((existing_id, _)) = registry.find_project_by_path(&canonical_input) {
            // Update the existing project info (in case metadata changed)
            let updated_info = Self::create_project_info(project_path);
            registry.add_project(&existing_id, updated_info);
            registry.save()?;
            Ok(existing_id)
        } else {
            // Register as new project
            let project_id = ProjectId::new();
            let project_info = Self::create_project_info(project_path);
            registry.add_project(project_id.as_str(), project_info);
            registry.save()?;
            Ok(project_id.to_string())
        }
    }

    /// Create project info from path (helper function)
    fn create_project_info(project_path: &Path) -> ProjectInfo {
        // Always store canonicalized paths to avoid duplicates from symlinks
        let canonical_path = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());

        let name = canonical_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        ProjectInfo {
            path: canonical_path,
            name,
            symbol_count: 0,
            file_count: 0,
            last_modified: 0,
            doc_count: 0,
        }
    }

    /// Add a project to the registry (internal helper)
    fn add_project(&mut self, project_id: &str, project_info: ProjectInfo) {
        self.projects.insert(project_id.to_string(), project_info);
    }

    /// Find a project by its path
    fn find_project_by_path(&self, path: &Path) -> Option<(String, &ProjectInfo)> {
        // Canonicalize the search path for comparison
        let search_path = path.canonicalize().ok()?;

        self.projects.iter().find_map(|(id, info)| {
            // Compare canonicalized paths to handle symlinks and relative paths
            if let Ok(project_path) = info.path.canonicalize() {
                if project_path == search_path {
                    return Some((id.clone(), info));
                }
            }
            None
        })
    }

    /// Find a project by its UUID
    pub fn find_project_by_id(&self, project_id: &str) -> Option<&ProjectInfo> {
        self.projects.get(project_id)
    }

    /// Find a project by its UUID (mutable)
    pub fn find_project_by_id_mut(&mut self, project_id: &str) -> Option<&mut ProjectInfo> {
        self.projects.get_mut(project_id)
    }

    /// Update project path when it moves
    pub fn update_project_path(
        &mut self,
        project_id: &str,
        new_path: &Path,
    ) -> Result<(), IndexError> {
        let project = self.projects.get_mut(project_id).ok_or_else(|| {
            IndexError::General(format!(
                "Project {project_id} not found\nSuggestion: Run 'codanna init' in the project directory"
            ))
        })?;

        project.path = new_path.to_path_buf();
        project.name = new_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        self.save()
    }
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Path Resolution Utilities

/// Resolve the index path from settings, accounting for --config flag usage
///
/// When using --config from outside the project:
/// 1. If index_path is absolute: use as-is
/// 2. If index_path is relative and config_path provided: resolve relative to config file
/// 3. If workspace_root is set: resolve relative to workspace_root
/// 4. Otherwise: use as-is (will be relative to CWD)
pub fn resolve_index_path(
    settings: &crate::config::Settings,
    config_path: Option<&Path>,
) -> PathBuf {
    // If absolute, return as-is
    if settings.index_path.is_absolute() {
        return settings.index_path.clone();
    }

    // If we loaded from a specific config file, resolve relative to it
    if let Some(cfg_path) = config_path {
        if let Some(parent) = cfg_path.parent() {
            // Check if parent is our local config directory
            let local_dir = local_dir_name();
            if parent.file_name() == Some(std::ffi::OsStr::new(local_dir)) {
                // Go up one more level to get workspace root
                if let Some(workspace) = parent.parent() {
                    return workspace.join(&settings.index_path);
                }
            }
            // Otherwise resolve relative to config directory
            return parent.join(&settings.index_path);
        }
    }

    // If workspace_root is set in settings, use it
    if let Some(workspace_root) = &settings.workspace_root {
        return workspace_root.join(&settings.index_path);
    }

    // Fallback: use as-is (relative to CWD)
    settings.index_path.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_names() {
        // Verify test configuration
        assert_eq!(GLOBAL_DIR_NAME, ".codanna-test");
        assert_eq!(LOCAL_DIR_NAME, ".codanna-test-local");
        assert_eq!(FASTEMBED_CACHE_NAME, ".fastembed_cache"); // FastEmbed requires this exact name
    }

    #[test]
    fn test_global_paths() {
        let global = global_dir();
        assert!(global.ends_with(GLOBAL_DIR_NAME));

        let models = models_dir();
        assert!(models.ends_with(format!("{GLOBAL_DIR_NAME}/models")));

        let projects = projects_file();
        assert!(projects.ends_with(format!("{GLOBAL_DIR_NAME}/projects.json")));
    }
}
