//! Config file initialization: settings.toml generation with comments, .codannaignore.

use super::Settings;
use std::path::PathBuf;

impl Settings {
    /// Create a default settings file with helpful comments
    pub fn init_config_file(force: bool) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Use configurable directory name from init module
        let local_dir = crate::init::local_dir_name();
        let config_path = PathBuf::from(local_dir).join("settings.toml");

        if !force && config_path.exists() {
            return Err("Configuration file already exists. Use --force to overwrite".into());
        }

        // Create parent directory if needed
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create settings with project-specific values
        let settings = Settings::for_init()?;

        // Convert to TOML
        let toml_string = toml::to_string_pretty(&settings)?;

        // Enhance with comments and documentation
        let final_toml = Self::add_config_comments(toml_string);

        std::fs::write(&config_path, final_toml)?;

        if force {
            println!("Overwrote configuration at: {}", config_path.display());
        } else {
            println!(
                "Created default configuration at: {}",
                config_path.display()
            );
        }

        // Create default .codannaignore file
        Self::create_default_ignore_file(force)?;

        // Initialize global directories and symlink
        crate::init::init_global_dirs()?;

        // Try to create symlink, but don't fail if it doesn't work (Windows privileges)
        // The symlink is optional since we use with_cache_dir() API in fastembed 5.0+
        if let Err(e) = crate::init::create_fastembed_symlink() {
            eprintln!("Note: Could not create model cache symlink: {e}");
            eprintln!("      This is normal on Windows without Developer Mode enabled.");
            eprintln!("      Models will be managed via cache directory API instead.");
        }

        // Create index directory structure (including tantivy subdirectory)
        let index_path = PathBuf::from(crate::init::local_dir_name()).join("index");
        std::fs::create_dir_all(&index_path)?;
        let tantivy_path = index_path.join("tantivy");
        std::fs::create_dir_all(&tantivy_path)?;

        // Check if project is already registered (by path in registry or by local file)
        let local_dir = crate::init::local_dir_name();
        let project_id_path = PathBuf::from(local_dir).join(".project-id");
        let project_path = std::env::current_dir()?;

        // Always use register_or_update which checks for existing projects by path
        let project_id = crate::init::ProjectRegistry::register_or_update_project(&project_path)?;

        // Check if we need to update the local .project-id file
        if project_id_path.exists() {
            let existing_id = std::fs::read_to_string(&project_id_path)?;
            if existing_id.trim() != project_id {
                // Update the file if the ID changed (shouldn't happen normally)
                std::fs::write(&project_id_path, &project_id)?;
                println!("Updated project ID: {project_id}");
            } else {
                println!("Project already registered with ID: {project_id}");
            }
        } else {
            // Create .project-id file for the first time
            std::fs::write(&project_id_path, &project_id)?;
            println!("Project registered with ID: {project_id}");
        }

        Ok(config_path)
    }

    /// Add helpful comments to the generated TOML configuration
    pub(super) fn add_config_comments(toml: String) -> String {
        let mut result = String::from(
            "# Codanna Configuration File\n\
             # https://github.com/bartolli/codanna\n\n",
        );

        let mut in_languages_section = false;
        let mut prev_line_was_section = false;

        for line in toml.lines() {
            // Skip empty lines after section headers to avoid double spacing
            if line.is_empty() && prev_line_was_section {
                prev_line_was_section = false;
                continue;
            }
            prev_line_was_section = false;

            // Add section and field comments
            if line == "version = 1" {
                result.push_str("# Version of the configuration schema\n");
            } else if line.starts_with("index_path = ") {
                result.push_str("\n# Path to the index directory (relative to workspace root)\n");
            } else if line.starts_with("workspace_root = ") {
                result.push_str("\n# Workspace root directory (automatically detected)\n");
            } else if line == "[indexing]" {
                result.push_str("\n[indexing]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("parallelism = ") {
                result.push_str("# CPU cores to use for indexing (default: all cores)\n");
                result.push_str("# Thread counts for each stage are derived from this value\n");
            } else if line.starts_with("tantivy_heap_mb = ") {
                result.push_str("\n# Tantivy heap size in megabytes\n");
                result.push_str("# Reduce to 15-25MB if you have permission issues (antivirus, SELinux, containers)\n");
                result.push_str(
                    "# Increase to 100-200MB if you have plenty of RAM and no restrictions\n",
                );
            } else if line.starts_with("max_retry_attempts = ") {
                result.push_str("\n# Retry attempts for transient file system errors\n");
                result.push_str("# Exponential backoff: 100ms, 200ms, 400ms delays\n");
            } else if line.starts_with("ignore_patterns = ") {
                result.push_str("\n# Additional patterns to ignore during indexing\n");
            } else if line.starts_with("indexed_paths = ") {
                result.push_str("\n# List of directories to index\n");
                result.push_str("# Add folders using: codanna add-dir <path>\n");
                result.push_str("# Remove folders using: codanna remove-dir <path>\n");
                result.push_str("# List all folders using: codanna list-dirs\n");
            } else if line.starts_with("batch_size = ") {
                result.push_str("\n# Items per batch before flushing to index (default: 5000)\n");
            } else if line.starts_with("batches_per_commit = ") {
                result.push_str("\n# Number of batches before committing to disk (default: 10)\n");
            } else if line.starts_with("pipeline_tracing = ") {
                result.push_str("\n# Enable detailed pipeline stage tracing\n");
                result.push_str("# Shows timing, throughput, and memory for each stage\n");
                result.push_str("# Requires: logging.modules.pipeline = \"info\"\n");
            } else if line.starts_with("show_progress = ") {
                result.push_str("\n# Show progress bars during indexing (default: true)\n");
                result.push_str("# Use --no-progress CLI flag to override\n");
            } else if line == "[mcp]" {
                result.push_str("\n[mcp]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("max_context_size = ") {
                result.push_str("# Maximum context size in bytes for MCP server\n");
                result.push_str(line);
                result.push('\n');
                result.push_str(
                    "\n# Host allowlist for Streamable HTTP inbound. None = loopback-only (localhost, 127.0.0.1, ::1).\n",
                );
                result.push_str("# Required for non-loopback binds (0.0.0.0, public hostnames).\n");
                result.push_str(
                    "# allowed_hosts = [\"codanna.example.com\", \"codanna.example.com:8080\"]\n",
                );
                result.push_str(
                    "\n# Origin allowlist for Streamable HTTP inbound. None = no Origin check. MCP clients are not browsers.\n",
                );
                result.push_str("# Set when serving browser clients that send Origin headers.\n");
                result.push_str("# allowed_origins = [\"https://app.example.com\"]\n");
                continue;
            } else if line == "[semantic_search]" {
                result.push_str("\n[semantic_search]\n");
                result.push_str("# Semantic search for natural language code queries\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("enabled = ") && !in_languages_section {
                // enabled field in semantic_search - comment already added above
            } else if line.starts_with("model = ") {
                result.push_str("\n# Model to use for embeddings\n");
                result.push_str(
                    "# Note: Changing models requires re-indexing (codanna index --force)\n",
                );
                result.push_str("# - AllMiniLML6V2: English-only, 384 dimensions (default)\n");
                result.push_str("# - MultilingualE5Small: 94 languages including, 384 dimensions (recommended for multilingual)\n");
                result.push_str(
                    "# - MultilingualE5Base: 94 languages, 768 dimensions (better quality)\n",
                );
                result.push_str(
                    "# - MultilingualE5Large: 94 languages, 1024 dimensions (best quality)\n",
                );
                result.push_str("# - BGESmallZHV15: Chinese-specialized, 512 dimensions\n");
                result.push_str("# - See documentation for full list of available models\n");
            } else if line.starts_with("threshold = ") {
                result.push_str("\n# Similarity threshold for search results (0.0 to 1.0)\n");
            } else if line.starts_with("embedding_threads = ") {
                result.push_str("\n# Number of parallel embedding model instances (default: 3)\n");
                result
                    .push_str("# Each instance uses ~86MB RAM. Higher values = faster indexing.\n");
                result.push_str("# Set to 1 for low-memory systems, 4-6 for high-end machines.\n");
                result
                    .push_str("\n# Remote embedding server (optional, replaces local fastembed)\n");
                result.push_str("# Supports OpenAI, Ollama, vLLM, Infinity, or any OpenAI-compatible endpoint.\n");
                result.push_str(
                    "# Uncomment and configure to use a remote server instead of local models.\n",
                );
                result.push_str("# remote_url = \"http://localhost:11434\"  # server base URL\n");
                result.push_str("# remote_model = \"nomic-embed-text\"     # model name to send\n");
                result.push_str("# remote_dim = 768                       # output dimension\n");
                result.push_str("# API key: set CODANNA_EMBED_API_KEY environment variable (not stored in config)\n");
                result.push_str("# Override any field with env vars: CODANNA_EMBED_URL, CODANNA_EMBED_MODEL, CODANNA_EMBED_DIM\n");
            } else if line == "[file_watch]" {
                result.push_str("\n[file_watch]\n");
                result.push_str("# Enable automatic file watching for indexed files\n");
                result.push_str("# When enabled, the MCP server will automatically re-index files when they change\n");
                result.push_str("# Default: true (enabled for better user experience)\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("enabled = ") && in_languages_section {
                // Skip comment for language enabled field
            } else if line.starts_with("debounce_ms = ") {
                result.push_str("\n# Debounce interval in milliseconds\n");
                result.push_str("# How long to wait after a file change before re-indexing\n");
            } else if line == "[server]" {
                result.push_str("\n[server]\n");
                result.push_str("# Server mode: \"stdio\" (default) or \"http\"\n");
                result.push_str("# stdio: Lightweight, spawns per request (best for production)\n");
                result.push_str(
                    "# http: Persistent server, real-time file watching (best for development)\n",
                );
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("mode = ") {
                // mode field - comment already added above
            } else if line.starts_with("bind = ") {
                result.push_str("\n# HTTP server bind address (only used when mode = \"http\" or --http flag)\n");
            } else if line.starts_with("watch_interval = ") {
                result.push_str("\n# Watch interval for stdio mode in seconds (how often to check for file changes)\n");
            } else if line == "[logging]" {
                result.push_str("\n[logging]\n");
                result.push_str("# Logging configuration\n");
                result.push_str("# Levels: \"error\", \"warn\" (default/quiet), \"info\", \"debug\", \"trace\"\n");
                result.push_str("# Override with RUST_LOG env var: RUST_LOG=debug codanna index\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("default = ") && !in_languages_section {
                result.push_str("# Default log level (\"warn\" = quiet, \"info\" = normal, \"debug\" = verbose)\n");
            } else if line == "[logging.modules]" {
                result.push_str("\n[logging.modules]\n");
                result.push_str("# Per-module log level overrides\n");
                result.push_str("# Internal modules (auto-prefixed with codanna::): watcher, mcp, indexing, storage\n");
                result.push_str(
                    "# External targets (used as-is): cli, tantivy, pipeline, semantic, rag\n",
                );
                result.push_str("# Examples (uncomment to enable):\n");
                result.push_str("# pipeline = \"info\"   # Code indexing stages and progress\n");
                result.push_str("# semantic = \"info\"   # Embedding pool and code embeddings\n");
                result.push_str("# rag = \"info\"        # Document collections and chunks\n");
                result.push_str("# watcher = \"debug\"   # File watcher events\n");
                result.push_str("# mcp = \"debug\"       # MCP server operations\n");
                prev_line_was_section = true;
                continue;
            } else if line == "[documents]" {
                result.push_str("\n[documents]\n");
                result.push_str("# Document embedding for RAG (Retrieval-Augmented Generation)\n");
                result.push_str("# Index markdown and text files for semantic search\n");
                prev_line_was_section = true;
                continue;
            } else if line == "[documents.defaults]" {
                result.push_str("\n[documents.defaults]\n");
                result.push_str("# Default chunking settings for all collections\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("strategy = ") {
                result.push_str(
                    "# Chunking strategy: \"hybrid\" (paragraph-based with size constraints)\n",
                );
            } else if line.starts_with("min_chunk_chars = ") {
                result.push_str("\n# Minimum characters per chunk (small chunks merged)\n");
            } else if line.starts_with("max_chunk_chars = ") {
                result.push_str("\n# Maximum characters per chunk (large chunks split)\n");
            } else if line.starts_with("overlap_chars = ") {
                result.push_str("\n# Overlap between chunks when splitting\n");
            } else if line == "[documents.search]" {
                result.push_str("\n[documents.search]\n");
                result.push_str("# Search result display settings\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("preview_mode = ") {
                result.push_str("# Preview mode: \"kwic\" (Keyword In Context) or \"full\"\n");
                result.push_str("# kwic: Centers preview around keyword match (recommended)\n");
                result.push_str("# full: Shows entire chunk content\n");
            } else if line.starts_with("preview_chars = ") {
                result.push_str("\n# Number of characters to show in preview (for kwic mode)\n");
            } else if line.starts_with("highlight = ") {
                result.push_str("\n# Highlight matching keywords with **markers**\n");
            } else if line == "[documents.collections]" {
                result.push_str("\n[documents.collections]\n");
                result.push_str("# Add document collections to index. Example:\n");
                result.push_str("# [documents.collections.my-docs]\n");
                result.push_str("# paths = [\"docs/\"]\n");
                result.push_str("# patterns = [\"**/*.md\"]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("[documents.collections.") {
                result.push_str("\n# Collection configuration\n");
                result.push_str("# paths: directories or files to include\n");
                result.push_str("# patterns: glob patterns to match (default: [\"**/*.md\"])\n");
            } else if line.starts_with("[languages.") {
                if !in_languages_section {
                    result.push_str("\n# Language-specific settings\n");
                    in_languages_section = true;
                }
                result.push('\n');

                // Add project resolver documentation for supported languages
                if line == "[languages.csharp]" {
                    result.push_str(line);
                    result.push_str("\n# Namespace resolution via .csproj (RootNamespace)\n");
                    result.push_str(
                        "# Resolves namespaces like MyCompany.MyApp.Controllers, Microsoft.EntityFrameworkCore\n",
                    );
                    result.push_str("# config_files = [\"/path/to/project/MyProject.csproj\"]\n");
                    continue;
                } else if line == "[languages.go]" {
                    result.push_str(line);
                    result.push_str("\n# Module path resolution via go.mod\n");
                    result.push_str(
                        "# Resolves imports like github.com/gin-gonic/gin, internal/handlers\n",
                    );
                    result.push_str("# config_files = [\"/path/to/project/go.mod\"]\n");
                    continue;
                } else if line == "[languages.java]" {
                    result.push_str(line);
                    result.push_str("\n# Package path resolution via build.gradle or pom.xml\n");
                    result.push_str(
                        "# Resolves imports like com.example.service, org.company.utils\n",
                    );
                    result.push_str(
                        "# If both exist, specify the one you use for building (typically Gradle)\n",
                    );
                    result.push_str("# config_files = [\"/path/to/project/build.gradle\"]\n");
                    result.push_str("# For custom source layouts:\n");
                    result.push_str("# [[languages.java.projects]]\n");
                    result.push_str("# config_file = \"/path/to/project/build.gradle\"\n");
                    result.push_str("# source_layout = \"jvm\"  # jvm | standard-kmp | flat-kmp\n");
                    continue;
                } else if line == "[languages.javascript]" {
                    result.push_str(line);
                    result.push_str(
                        "\n# Path alias resolution via jsconfig.json (CRA, Next.js, Vite)\n",
                    );
                    result
                        .push_str("# Resolves imports like @components/Button, @/utils/helpers\n");
                    result.push_str("# config_files = [\"/path/to/project/jsconfig.json\"]\n");
                    continue;
                } else if line == "[languages.kotlin]" {
                    result.push_str(line);
                    result.push_str("\n# Source root resolution via build.gradle.kts\n");
                    result
                        .push_str("# Resolves imports like com.example.shared, io.ktor.network\n");
                    result.push_str("# config_files = [\"/path/to/project/build.gradle.kts\"]\n");
                    result.push_str("# For Kotlin Multiplatform with custom layouts:\n");
                    result.push_str("# [[languages.kotlin.projects]]\n");
                    result.push_str("# config_file = \"/path/to/project/build.gradle.kts\"\n");
                    result.push_str(
                        "# source_layout = \"flat-kmp\"  # jvm | standard-kmp | flat-kmp\n",
                    );
                    continue;
                } else if line == "[languages.php]" {
                    result.push_str(line);
                    result.push_str(
                        "\n# PSR-4 namespace resolution via composer.json autoload section\n",
                    );
                    result.push_str(
                        "# Resolves namespaces like App\\Controllers\\UserController, Tests\\Unit\n",
                    );
                    result.push_str("# config_files = [\"/path/to/project/composer.json\"]\n");
                    continue;
                } else if line == "[languages.python]" {
                    result.push_str(line);
                    result.push_str(
                        "\n# Module resolution via pyproject.toml (Poetry, Hatch, Maturin, setuptools)\n",
                    );
                    result.push_str("# Resolves imports like mypackage.utils, src.models\n");
                    result.push_str("# config_files = [\"/path/to/project/pyproject.toml\"]\n");
                    continue;
                } else if line == "[languages.swift]" {
                    result.push_str(line);
                    result.push_str(
                        "\n# Module resolution via Package.swift (Swift Package Manager)\n",
                    );
                    result
                        .push_str("# Resolves imports like MyLibrary.Models, PackageName.Utils\n");
                    result.push_str("# config_files = [\"/path/to/project/Package.swift\"]\n");
                    continue;
                } else if line == "[languages.typescript]" {
                    result.push_str(line);
                    result
                        .push_str("\n# Path alias resolution via tsconfig.json (baseUrl, paths)\n");
                    result.push_str(
                        "# Resolves imports like @components/Button, @utils/helpers, @/types\n",
                    );
                    result.push_str("# config_files = [\"/path/to/project/tsconfig.json\"]\n");
                    result.push_str("# For monorepos with multiple tsconfigs:\n");
                    result.push_str("# config_files = [\n");
                    result.push_str("#     \"/path/to/project/tsconfig.json\",\n");
                    result.push_str("#     \"/path/to/project/packages/web/tsconfig.json\",\n");
                    result.push_str("# ]\n");
                    continue;
                }
            }

            result.push_str(line);
            result.push('\n');
        }

        result
    }

    /// Create a default .codannaignore file with helpful patterns
    fn create_default_ignore_file(force: bool) -> Result<(), Box<dyn std::error::Error>> {
        let ignore_path = PathBuf::from(".codannaignore");

        if !force && ignore_path.exists() {
            println!("Found existing .codannaignore file");
            return Ok(());
        }

        let default_content = r#"# Codanna ignore patterns (gitignore syntax)
# https://git-scm.com/docs/gitignore
#
# This file tells codanna which files to exclude from indexing.
# Each line specifies a pattern. Patterns follow the same rules as .gitignore.

# Build artifacts
target/
build/
dist/
*.o
*.so
*.dylib
*.exe
*.dll

# Test files (uncomment to exclude tests from indexing)
# tests/
# *_test.rs
# *.test.js
# *.spec.ts
# test_*.py

# Temporary files
*.tmp
*.temp
*.bak
*.swp
*.swo
*~
.DS_Store

# Codanna's own directory
.codanna/

# Dependency directories
node_modules/
vendor/
.venv/
venv/
__pycache__/
*.egg-info/
.cargo/

# IDE and editor directories
.idea/
.vscode/
*.iml
.project
.classpath
.settings/

# Documentation (uncomment if you don't want to index docs)
# docs/
# *.md

# Generated files
*.generated.*
*.auto.*
*_pb2.py
*.pb.go

# Version control
.git/
.svn/
.hg/

# Example of including specific files from ignored directories:
# !target/doc/
# !vendor/specific-file.rs
"#;

        std::fs::write(&ignore_path, default_content)?;

        if force && ignore_path.exists() {
            println!("Overwrote .codannaignore file");
        } else {
            println!("Created default .codannaignore file");
        }

        Ok(())
    }
}
