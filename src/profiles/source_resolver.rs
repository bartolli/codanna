//! Profile source resolution - converts provider sources to profile directories
//!
//! Handles local paths, GitHub repos, and Git URLs. For remote sources, clones to
//! a temporary directory that is automatically cleaned up.

use super::error::{ProfileError, ProfileResult};
use super::provider_registry::ProviderSource;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Resolved profile source with path to profile directory
#[derive(Debug)]
pub enum ResolvedProfileSource {
    /// Local provider source
    Local { path: PathBuf },

    /// Git provider source (temporary clone)
    Git { temp_dir: TempDir, commit: String },
}

impl ResolvedProfileSource {
    /// Get the path to the profile directory
    pub fn profile_dir(&self, profile_name: &str) -> PathBuf {
        match self {
            Self::Local { path } => path.join(profile_name),
            Self::Git { temp_dir, .. } => temp_dir
                .path()
                .join(".codanna-profile")
                .join("profiles")
                .join(profile_name),
        }
    }

    /// Get commit SHA if this is a git source
    pub fn commit(&self) -> Option<&str> {
        match self {
            Self::Git { commit, .. } => Some(commit),
            Self::Local { .. } => None,
        }
    }
}

/// Resolve provider source to a profile directory
///
/// For local sources, verifies the profile directory exists.
/// For git sources, clones to a temporary directory.
pub fn resolve_profile_source(
    provider_source: &ProviderSource,
    profile_name: &str,
) -> ProfileResult<ResolvedProfileSource> {
    match provider_source {
        ProviderSource::Local { path } => resolve_local_source(path, profile_name),
        ProviderSource::Github { repo } => {
            let url = format!("https://github.com/{repo}.git");
            resolve_git_source(&url, profile_name)
        }
        ProviderSource::Url { url } => resolve_git_source(url, profile_name),
    }
}

/// Resolve local provider source
fn resolve_local_source(path: &str, profile_name: &str) -> ProfileResult<ResolvedProfileSource> {
    let base_path = Path::new(path);
    let profiles_path = base_path.join(".codanna-profile").join("profiles");
    let profile_path = profiles_path.join(profile_name);

    if !profile_path.exists() {
        return Err(ProfileError::ProfileNotFoundInProvider {
            profile: profile_name.to_string(),
            provider: path.to_string(),
        });
    }

    Ok(ResolvedProfileSource::Local {
        path: profiles_path,
    })
}

/// Resolve git provider source (clones to temp directory)
fn resolve_git_source(url: &str, profile_name: &str) -> ProfileResult<ResolvedProfileSource> {
    let temp_dir = tempfile::tempdir()?;

    // Clone repository
    let commit = clone_repository(url, temp_dir.path(), None)?;

    // Verify profile exists in cloned repo
    let profile_path = temp_dir
        .path()
        .join(".codanna-profile")
        .join("profiles")
        .join(profile_name);

    if !profile_path.exists() {
        return Err(ProfileError::ProfileNotFoundInProvider {
            profile: profile_name.to_string(),
            provider: url.to_string(),
        });
    }

    Ok(ResolvedProfileSource::Git { temp_dir, commit })
}

/// Clone git repository using system git
fn clone_repository(url: &str, dest: &Path, git_ref: Option<&str>) -> ProfileResult<String> {
    super::git::clone_repository(url, dest, git_ref)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_local_source() {
        let temp = tempdir().unwrap();
        let provider_dir = temp.path().join("my-provider");

        // Create profile structure
        let profiles_dir = provider_dir.join(".codanna-profile/profiles");
        let profile_dir = profiles_dir.join("test-profile");
        fs::create_dir_all(&profile_dir).unwrap();

        // Create profile.json
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"name": "test-profile", "version": "1.0.0"}"#,
        )
        .unwrap();

        let source = ProviderSource::Local {
            path: provider_dir.to_string_lossy().to_string(),
        };

        let resolved = resolve_profile_source(&source, "test-profile").unwrap();

        match &resolved {
            ResolvedProfileSource::Local { path } => {
                assert_eq!(path, &profiles_dir);
                assert!(resolved.profile_dir("test-profile").exists());
            }
            _ => panic!("Expected Local source"),
        }
    }

    #[test]
    fn test_resolve_local_source_profile_not_found() {
        let temp = tempdir().unwrap();
        let provider_dir = temp.path().join("my-provider");
        fs::create_dir_all(&provider_dir).unwrap();

        let source = ProviderSource::Local {
            path: provider_dir.to_string_lossy().to_string(),
        };

        let result = resolve_profile_source(&source, "nonexistent");
        assert!(result.is_err());

        match result.unwrap_err() {
            ProfileError::ProfileNotFoundInProvider { profile, provider } => {
                assert_eq!(profile, "nonexistent");
                assert!(provider.contains("my-provider"));
            }
            e => panic!("Expected ProfileNotFoundInProvider, got: {e:?}"),
        }
    }

    #[test]
    #[ignore] // Requires network
    fn test_resolve_git_source_github() {
        // This test requires an actual GitHub repo with proper structure
        // Skip in normal test runs
        let source = ProviderSource::Github {
            repo: "codanna/test-profiles".to_string(),
        };

        let result = resolve_profile_source(&source, "test-profile");
        // Would succeed if repo exists with correct structure
        // For now, just verify it attempts git cloning (not "not implemented" error)
        if let Err(e) = result {
            // Should be a git error or profile not found, not "not implemented"
            assert!(!e.to_string().contains("not yet implemented"));
        }
    }

    #[test]
    fn test_resolved_source_commit() {
        let temp = tempdir().unwrap();
        let provider_dir = temp.path().join("my-provider");
        let profiles_dir = provider_dir.join(".codanna-profile/profiles");
        let profile_dir = profiles_dir.join("test-profile");
        fs::create_dir_all(&profile_dir).unwrap();

        let source = ProviderSource::Local {
            path: provider_dir.to_string_lossy().to_string(),
        };

        let resolved = resolve_profile_source(&source, "test-profile").unwrap();

        // Local sources have no commit
        assert!(resolved.commit().is_none());
    }

    #[test]
    fn test_profile_dir_path_construction() {
        let temp = tempdir().unwrap();
        let provider_dir = temp.path().join("my-provider");
        let profiles_dir = provider_dir.join(".codanna-profile/profiles");
        let profile_dir = profiles_dir.join("my-profile");
        fs::create_dir_all(&profile_dir).unwrap();

        let source = ProviderSource::Local {
            path: provider_dir.to_string_lossy().to_string(),
        };

        let resolved = resolve_profile_source(&source, "my-profile").unwrap();
        let constructed_path = resolved.profile_dir("my-profile");

        assert_eq!(constructed_path, profile_dir);
        assert!(constructed_path.ends_with("my-profile"));
    }
}
