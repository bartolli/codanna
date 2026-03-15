//! Git operations using system git binary
//!
//! Replaces direct libgit2 usage to honor ~/.ssh/config, credential helpers,
//! and other git configuration that libgit2 does not support.

use std::path::Path;
use std::process::Command;
use thiserror::Error;

/// Git process exit code with user-friendly Display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitExitCode(pub Option<i32>);

impl std::fmt::Display for GitExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(c) => write!(f, "{c}"),
            None => write!(f, "unknown"),
        }
    }
}

#[derive(Error, Debug)]
pub enum GitError {
    #[error(
        "git is not installed or not found on PATH\nSuggestion: Install git and ensure it is in your PATH"
    )]
    GitNotFound,

    #[error("repository not found: {url}\nSuggestion: Check the URL and your access permissions")]
    RepositoryNotFound { url: String },

    #[error(
        "reference '{ref_name}' not found in {url}\nSuggestion: Use a valid branch name or tag"
    )]
    ReferenceNotFound { url: String, ref_name: String },

    #[error(
        "git {command} failed (exit {exit_code}): {stderr}\nSuggestion: Check network connection and repository permissions"
    )]
    CommandFailed {
        command: String,
        stderr: String,
        exit_code: GitExitCode,
    },

    #[error("IO error: {0}\nSuggestion: Check file permissions and disk space")]
    Io(#[from] std::io::Error),
}

impl GitError {
    /// Error detail without the suggestion line.
    /// Use when embedding in a wrapper error that provides its own suggestion.
    pub fn message(&self) -> String {
        let full = self.to_string();
        match full.find("\nSuggestion:") {
            Some(pos) => full[..pos].to_string(),
            None => full,
        }
    }
}

pub type GitResult<T> = Result<T, GitError>;

/// Run a git command and return stdout on success, GitError on failure.
fn run_git(args: &[&str]) -> GitResult<String> {
    let output = Command::new("git")
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::GitNotFound
            } else {
                GitError::Io(e)
            }
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let command = args.join(" ");
        Err(GitError::CommandFailed {
            command,
            stderr,
            exit_code: GitExitCode(output.status.code()),
        })
    }
}

/// Clone a repository. Uses --depth 1 for remote repos. Returns commit SHA.
pub fn clone_repository(
    repo_url: &str,
    target_dir: &Path,
    git_ref: Option<&str>,
) -> GitResult<String> {
    let is_local = repo_url.starts_with("file://") || Path::new(repo_url).exists();

    if let Some(parent) = target_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)?;
    }

    let mut args: Vec<String> = vec!["clone".to_string()];

    if !is_local {
        args.push("--depth".to_string());
        args.push("1".to_string());
    }

    if let Some(ref_value) = git_ref {
        args.push("--branch".to_string());
        args.push(ref_value.to_string());
    }

    args.push(repo_url.to_string());
    args.push(target_dir.to_string_lossy().to_string());

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_git(&arg_refs).map_err(|e| match e {
        GitError::CommandFailed {
            stderr, exit_code, ..
        } => GitError::CommandFailed {
            command: format!("clone {repo_url}"),
            stderr,
            exit_code,
        },
        other => other,
    })?;

    get_commit_sha(target_dir)
}

/// Resolve a git reference to a commit SHA without cloning.
pub fn resolve_reference(repo_url: &str, git_ref: &str) -> GitResult<String> {
    let heads_ref = format!("refs/heads/{git_ref}");
    let tags_ref = format!("refs/tags/{git_ref}");

    let args = [
        "ls-remote".to_string(),
        "--exit-code".to_string(),
        repo_url.to_string(),
        git_ref.to_string(),
        heads_ref,
        tags_ref,
    ];
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let output = run_git(&arg_refs);

    match output {
        Ok(stdout) => {
            // Parse first line: "<sha>\t<refname>"
            if let Some(sha) = stdout
                .lines()
                .next()
                .and_then(|line| line.split('\t').next())
            {
                Ok(sha.to_string())
            } else {
                Err(GitError::ReferenceNotFound {
                    url: repo_url.to_string(),
                    ref_name: git_ref.to_string(),
                })
            }
        }
        Err(GitError::CommandFailed {
            exit_code: GitExitCode(Some(2)),
            ..
        }) => Err(GitError::ReferenceNotFound {
            url: repo_url.to_string(),
            ref_name: git_ref.to_string(),
        }),
        Err(GitError::CommandFailed {
            exit_code: GitExitCode(Some(128)),
            ..
        }) => Err(GitError::RepositoryNotFound {
            url: repo_url.to_string(),
        }),
        Err(other) => Err(other),
    }
}

/// Check if a URL points to a valid Git repository.
pub fn validate_repository(repo_url: &str) -> GitResult<()> {
    let output = Command::new("git")
        .args(["ls-remote", "--exit-code", repo_url, "HEAD"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::GitNotFound
            } else {
                GitError::Io(e)
            }
        })?;

    match output.status.code() {
        Some(0) | Some(2) => Ok(()),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("not found")
                || stderr.contains("does not exist")
                || stderr.contains("Could not read from remote")
            {
                Err(GitError::RepositoryNotFound {
                    url: repo_url.to_string(),
                })
            } else {
                Err(GitError::CommandFailed {
                    command: format!("ls-remote {repo_url}"),
                    stderr,
                    exit_code: GitExitCode(output.status.code()),
                })
            }
        }
    }
}

/// Get the current commit SHA of a local repository.
pub fn get_commit_sha(repo_dir: &Path) -> GitResult<String> {
    let dir_str = repo_dir.to_string_lossy();
    run_git(&["-C", &dir_str, "rev-parse", "HEAD"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    /// Create a test git repo with one commit. Disables gpgsign for CI/local compat.
    fn init_test_repo(path: &Path) {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap_or_else(|e| panic!("failed to run git {}: {e}", args.join(" ")));
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        std::fs::write(path.join("README.md"), "test").expect("write file");
        run(&["add", "-A"]);
        run(&["commit", "-m", "initial commit"]);
    }

    #[test]
    fn test_get_commit_sha_valid_repo() {
        let dir = tempdir().unwrap();
        init_test_repo(dir.path());
        let sha = get_commit_sha(dir.path()).unwrap();
        assert_eq!(sha.len(), 40, "SHA should be 40 hex chars");
        assert!(
            sha.chars().all(|c| c.is_ascii_hexdigit()),
            "SHA should be hex"
        );
    }

    #[test]
    fn test_get_commit_sha_not_a_repo() {
        let dir = tempdir().unwrap();
        // No git init -- just an empty directory
        let result = get_commit_sha(dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::CommandFailed {
                command, stderr, ..
            } => {
                assert!(
                    command.contains("rev-parse"),
                    "command should mention rev-parse"
                );
                assert!(!stderr.is_empty(), "stderr should have details");
            }
            other => panic!("expected CommandFailed, got: {other:?}"),
        }
    }

    #[test]
    fn test_clone_local_repository() {
        let source = tempdir().unwrap();
        init_test_repo(source.path());

        let target = tempdir().unwrap();
        let clone_path = target.path().join("cloned");

        let sha = clone_repository(source.path().to_str().unwrap(), &clone_path, None).unwrap();
        assert_eq!(sha.len(), 40);
        assert!(clone_path.join("README.md").exists());
    }

    #[test]
    fn test_clone_local_with_branch() {
        let source = tempdir().unwrap();
        init_test_repo(source.path());

        // Create a branch with a new file
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(source.path())
                .output()
                .unwrap_or_else(|e| panic!("failed to run git {}: {e}", args.join(" ")));
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["checkout", "-b", "feature"]);
        std::fs::write(source.path().join("feature.txt"), "feature content").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-m", "feature commit"]);

        let target = tempdir().unwrap();
        let clone_path = target.path().join("cloned");

        let sha = clone_repository(
            source.path().to_str().unwrap(),
            &clone_path,
            Some("feature"),
        )
        .unwrap();
        assert_eq!(sha.len(), 40);
        assert!(clone_path.join("feature.txt").exists());
    }

    #[test]
    fn test_clone_nonexistent_source() {
        let target = tempdir().unwrap();
        let clone_path = target.path().join("cloned");
        let result = clone_repository("/nonexistent/path/repo", &clone_path, None);
        match result.unwrap_err() {
            GitError::CommandFailed {
                command, exit_code, ..
            } => {
                assert!(command.contains("clone"), "should be a clone error");
                assert_eq!(
                    exit_code,
                    GitExitCode(Some(128)),
                    "git clone should exit 128 for invalid repo"
                );
            }
            other => panic!("expected CommandFailed, got: {other:?}"),
        }
    }

    #[test]
    fn test_resolve_reference_head() {
        let dir = tempdir().unwrap();
        init_test_repo(dir.path());

        let sha = resolve_reference(dir.path().to_str().unwrap(), "HEAD").unwrap();
        let expected = get_commit_sha(dir.path()).unwrap();
        assert_eq!(sha, expected);
    }

    #[test]
    fn test_resolve_reference_branch() {
        let dir = tempdir().unwrap();
        init_test_repo(dir.path());

        // Read the default branch name from .git/HEAD
        let head = std::fs::read_to_string(dir.path().join(".git/HEAD")).unwrap();
        let branch = head.trim().strip_prefix("ref: refs/heads/").unwrap();

        let sha = resolve_reference(dir.path().to_str().unwrap(), branch).unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn test_resolve_reference_not_found() {
        let dir = tempdir().unwrap();
        init_test_repo(dir.path());

        let result = resolve_reference(dir.path().to_str().unwrap(), "nonexistent-ref-xyz");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GitError::ReferenceNotFound { .. }
        ));
    }

    #[test]
    fn test_validate_local_repo() {
        let dir = tempdir().unwrap();
        init_test_repo(dir.path());
        assert!(validate_repository(dir.path().to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_validate_nonexistent() {
        let result = validate_repository("/nonexistent/path/repo");
        assert!(
            matches!(result.unwrap_err(), GitError::RepositoryNotFound { .. }),
            "should return RepositoryNotFound for nonexistent path"
        );
    }

    #[test]
    fn test_error_messages_include_suggestions() {
        let errors: Vec<GitError> = vec![
            GitError::GitNotFound,
            GitError::RepositoryNotFound {
                url: "test".to_string(),
            },
            GitError::ReferenceNotFound {
                url: "test".to_string(),
                ref_name: "main".to_string(),
            },
            GitError::CommandFailed {
                command: "test".to_string(),
                stderr: "fail".to_string(),
                exit_code: GitExitCode(Some(1)),
            },
        ];
        for err in errors {
            assert!(
                err.to_string().contains("Suggestion:"),
                "Missing Suggestion in: {err}"
            );
        }
    }
}
