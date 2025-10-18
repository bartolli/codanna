//! Claude configuration template management
//!
//! This module handles copying .claude configuration files (agents, commands, prompts, hooks)
//! to newly initialized projects. Templates are embedded in the binary at compile time.

use rust_embed::RustEmbed;
use std::path::{Path, PathBuf};

/// Embedded Claude configuration templates
/// These files are embedded at compile time from the .claude/ directory
#[derive(RustEmbed)]
#[folder = ".claude/"]
#[exclude = "settings.local.json"]
#[exclude = "README.md"]
#[exclude = ".gitignore"]
#[exclude = "*.swp"]
#[exclude = "*~"]
struct ClaudeTemplates;

/// Statistics about the copy operation
#[derive(Debug, Default)]
pub struct CopyStats {
    pub files_copied: usize,
    pub files_skipped: usize,
    pub dirs_created: usize,
}

impl CopyStats {
    fn new() -> Self {
        Self::default()
    }
}

/// Main entry point for copying Claude configuration
///
/// # Arguments
/// * `copy_claude` - If true, copy embedded templates
/// * `copy_claude_from` - Optional path to copy from instead of embedded
/// * `force` - If true, overwrite existing files
///
/// # Returns
/// * `Ok(())` if successful
/// * `Err(String)` with error message on failure
pub fn copy_claude_config(
    copy_claude: bool,
    copy_claude_from: Option<&Path>,
    force: bool,
) -> Result<(), String> {
    // Determine source
    if let Some(source_path) = copy_claude_from {
        copy_from_path(source_path, force)
    } else if copy_claude {
        copy_embedded_templates(force)
    } else {
        // Nothing to do
        Ok(())
    }
}

/// Copy embedded templates to .codanna/ directory
fn copy_embedded_templates(force: bool) -> Result<(), String> {
    println!("Copying embedded .claude configuration...");

    let mut stats = CopyStats::new();
    let target_base = PathBuf::from(".codanna");

    // Iterate through all embedded files
    for file_path_str in ClaudeTemplates::iter() {
        let file_path = Path::new(file_path_str.as_ref());

        // Get the file content
        let content = ClaudeTemplates::get(file_path_str.as_ref())
            .ok_or_else(|| format!("Failed to read embedded file: {}", file_path.display()))?;

        // Construct target path
        let target_path = target_base.join(file_path);

        // Write the file
        write_template_file(&target_path, &content.data, force, &mut stats)?;
    }

    // Print summary
    println!("\nSummary:");
    println!(
        "- {} files copied from embedded templates",
        stats.files_copied
    );
    if stats.files_skipped > 0 {
        println!("- {} files skipped (already exist)", stats.files_skipped);
        println!("- Use --force to overwrite existing files");
    }
    println!(
        "- Templates version: codanna v{}",
        env!("CARGO_PKG_VERSION")
    );

    Ok(())
}

/// Copy .claude files from a custom filesystem path
fn copy_from_path(source_path: &Path, force: bool) -> Result<(), String> {
    if !source_path.exists() {
        return Err(format!(
            "Source path does not exist: {}",
            source_path.display()
        ));
    }

    // Look for .claude directory in source
    let source_claude = source_path.join(".claude");
    if !source_claude.exists() || !source_claude.is_dir() {
        return Err(format!(
            "No .claude directory found at: {}",
            source_path.display()
        ));
    }

    println!(
        "Copying .claude configuration from {}...",
        source_path.display()
    );

    let mut stats = CopyStats::new();
    let target_base = PathBuf::from(".codanna");

    // Walk the source directory
    walkdir::WalkDir::new(&source_claude)
        .into_iter()
        .filter_entry(should_copy_entry)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .try_for_each(|entry| {
            let source_file = entry.path();

            // Get relative path from source .claude
            let rel_path = source_file
                .strip_prefix(&source_claude)
                .map_err(|e| format!("Failed to get relative path: {e}"))?;

            // Construct target path
            let target_path = target_base.join(rel_path);

            // Read source file
            let content = std::fs::read(source_file)
                .map_err(|e| format!("Failed to read {}: {}", source_file.display(), e))?;

            // Write the file
            write_template_file(&target_path, &content, force, &mut stats)
        })?;

    // Print summary
    println!("\nSummary:");
    println!("- {} files copied from custom template", stats.files_copied);
    if stats.files_skipped > 0 {
        println!("- {} files skipped (already exist)", stats.files_skipped);
        println!("- Use --force to overwrite existing files");
    }
    println!("- Source: {}", source_path.display());

    Ok(())
}

/// Write a template file to the target location
///
/// Handles:
/// - Directory creation
/// - Conflict detection
/// - Force overwrite
fn write_template_file(
    target_path: &Path,
    content: &[u8],
    force: bool,
    stats: &mut CopyStats,
) -> Result<(), String> {
    // Create parent directory if needed
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
            println!("✓ Created {}/", parent.display());
            stats.dirs_created += 1;
        }
    }

    // Check if file exists
    if target_path.exists() && !force {
        println!("ℹ Skipped {} (already exists)", target_path.display());
        stats.files_skipped += 1;
        return Ok(());
    }

    // Write the file
    std::fs::write(target_path, content)
        .map_err(|e| format!("Failed to write {}: {}", target_path.display(), e))?;

    if target_path.exists() && force {
        println!("⚠ Overwrote {}", target_path.display());
    } else {
        println!("✓ Copied {}", target_path.display());
    }
    stats.files_copied += 1;

    Ok(())
}

/// Filter function for walkdir to determine which entries to process
fn should_copy_entry(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();

    // Skip hidden files and directories (except at root)
    if entry.depth() > 0 && name.starts_with('.') {
        return false;
    }

    // Skip files we explicitly don't want to copy
    if entry.file_type().is_file()
        && (name == "README.md"
            || name == "settings.local.json"
            || name.ends_with(".swp")
            || name.ends_with('~')
            || name == ".gitignore")
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_templates_exist() {
        // Verify that we have embedded templates
        let count = ClaudeTemplates::iter().count();
        assert!(
            count > 0,
            "No embedded templates found. Make sure .claude/ directory exists at compile time."
        );
    }

    #[test]
    fn test_embedded_templates_structure() {
        // Check for expected directories
        let files: Vec<String> = ClaudeTemplates::iter().map(|s| s.to_string()).collect();

        // We should have files from agents, commands, prompts subdirectories
        let has_agents = files.iter().any(|f| f.starts_with("agents/"));
        let has_commands = files.iter().any(|f| f.starts_with("commands/"));
        let has_prompts = files.iter().any(|f| f.starts_with("prompts/"));

        assert!(
            has_agents || has_commands || has_prompts,
            "Expected to find files in agents/, commands/, or prompts/ directories"
        );
    }

    #[test]
    fn test_should_not_embed_excluded_files() {
        let files: Vec<String> = ClaudeTemplates::iter().map(|s| s.to_string()).collect();

        // Verify excluded files are not embedded
        assert!(
            !files.iter().any(|f| f.contains("README.md")),
            "README.md should be excluded"
        );
        assert!(
            !files.iter().any(|f| f.contains("settings.local.json")),
            "settings.local.json should be excluded"
        );
    }

    #[test]
    fn test_copy_stats() {
        let mut stats = CopyStats::new();
        assert_eq!(stats.files_copied, 0);
        assert_eq!(stats.files_skipped, 0);
        assert_eq!(stats.dirs_created, 0);

        stats.files_copied = 5;
        stats.files_skipped = 2;
        stats.dirs_created = 3;

        assert_eq!(stats.files_copied, 5);
        assert_eq!(stats.files_skipped, 2);
        assert_eq!(stats.dirs_created, 3);
    }
}
