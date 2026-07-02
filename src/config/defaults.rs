//! Default values: serde default fns, guidance templates, language defaults.

use super::{GuidanceRange, GuidanceTemplate, LanguageConfig};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::PathBuf;
pub(super) fn default_log_level() -> String {
    "warn".to_string() // Quiet by default, use RUST_LOG=info for normal output
}

pub(super) fn default_logging_modules() -> IndexMap<String, String> {
    let mut modules = IndexMap::new();
    // Suppress verbose Tantivy internal logs by default
    modules.insert("tantivy".to_string(), "warn".to_string());
    // Pipeline logs at warn by default (use "info" to see progress)
    modules.insert("pipeline".to_string(), "warn".to_string());
    modules
}

// Default value functions
pub(super) fn default_version() -> u32 {
    1
}
pub(super) fn default_index_path() -> PathBuf {
    // Use configurable directory name from init module
    let local_dir = crate::init::local_dir_name();
    PathBuf::from(local_dir).join("index")
}
pub(super) fn default_parallelism() -> usize {
    num_cpus::get()
}
pub(super) fn default_tantivy_heap_mb() -> usize {
    50 // Universal default that balances performance and permissions
}
pub(super) fn default_max_retry_attempts() -> u32 {
    3 // Exponential backoff: 100ms, 200ms, 400ms
}
pub(super) fn default_batch_size() -> usize {
    5000 // Symbols per batch before Tantivy flush
}
pub(super) fn default_batches_per_commit() -> usize {
    10 // Commit every 10 batches (~50K symbols)
}
pub(super) fn default_true() -> bool {
    true
}
pub(super) fn default_false() -> bool {
    false
}
pub(super) fn default_max_context_size() -> usize {
    100_000
}
pub(super) fn default_embedding_model() -> String {
    "AllMiniLML6V2".to_string()
}
pub(super) fn default_similarity_threshold() -> f32 {
    0.6
}
pub(super) fn default_embedding_threads() -> usize {
    3
}
pub(super) fn default_debounce_ms() -> u64 {
    500
}
pub(super) fn default_server_mode() -> String {
    "stdio".to_string()
}
pub(super) fn default_bind_address() -> String {
    "127.0.0.1:8080".to_string()
}
pub(super) fn default_watch_interval() -> u64 {
    5
}

pub(super) fn default_guidance_templates() -> IndexMap<String, GuidanceTemplate> {
    let mut templates = IndexMap::new();

    // Semantic search docs
    templates.insert("semantic_search_docs".to_string(), GuidanceTemplate {
        no_results: Some("No results found. Try broader search terms or check if the codebase is indexed.".to_string()),
        single_result: Some("Found one match. Consider using 'find_symbol' or 'get_calls' to explore this symbol's relationships.".to_string()),
        multiple_results: Some("Found {result_count} matches. Consider using 'find_symbol' on the most relevant result for detailed analysis, or refine your search query.".to_string()),
        custom: vec![
            GuidanceRange {
                min: 10,
                max: None,
                template: "Found {result_count} matches. Consider refining your search with more specific terms.".to_string(),
            }
        ],
    });

    // Find symbol
    templates.insert("find_symbol".to_string(), GuidanceTemplate {
        no_results: Some("Symbol not found. Use 'search_symbols' with fuzzy matching or 'semantic_search_docs' for broader search.".to_string()),
        single_result: Some("Symbol found with full context. Explore 'get_calls' to see what it calls, 'find_callers' to see usage, or 'analyze_impact' to understand change implications.".to_string()),
        multiple_results: Some("Found {result_count} symbols with that name. Review each to find the one you're looking for.".to_string()),
        custom: vec![],
    });

    // Get calls
    templates.insert("get_calls".to_string(), GuidanceTemplate {
        no_results: Some("No function calls found. This might be a leaf function or data structure.".to_string()),
        single_result: Some("Found 1 function call. Use 'find_symbol' to explore this dependency.".to_string()),
        multiple_results: Some("Found {result_count} function calls. Consider using 'find_symbol' on key dependencies or 'analyze_impact' to trace the call chain further.".to_string()),
        custom: vec![],
    });

    // Find callers
    templates.insert("find_callers".to_string(), GuidanceTemplate {
        no_results: Some("No callers found. This might be an entry point, unused code, or called dynamically.".to_string()),
        single_result: Some("Found 1 caller. Use 'find_symbol' to explore where this function is used.".to_string()),
        multiple_results: Some("Found {result_count} callers. Consider 'analyze_impact' for complete dependency graph or investigate specific callers with 'find_symbol'.".to_string()),
        custom: vec![],
    });

    // Analyze impact
    templates.insert("analyze_impact".to_string(), GuidanceTemplate {
        no_results: Some("No impact detected. This symbol appears isolated. Consider using the codanna-navigator agent for comprehensive multi-hop analysis of complex relationships.".to_string()),
        single_result: Some("Minimal impact radius. This symbol has limited dependencies.".to_string()),
        multiple_results: Some("Impact analysis shows {result_count} affected symbols. Focus on critical paths or use 'find_symbol' on key dependencies.".to_string()),
        custom: vec![
            GuidanceRange {
                min: 2,
                max: Some(5),
                template: "Limited impact radius with {result_count} affected symbols. This change is relatively contained.".to_string(),
            },
            GuidanceRange {
                min: 20,
                max: None,
                template: "Significant impact with {result_count} affected symbols. Consider breaking this change into smaller parts.".to_string(),
            }
        ],
    });

    // Search symbols
    templates.insert("search_symbols".to_string(), GuidanceTemplate {
        no_results: Some("No symbols match your query. Try 'semantic_search_docs' for natural language search or adjust your pattern.".to_string()),
        single_result: Some("Found exactly one match. Use 'find_symbol' to get full details about this symbol.".to_string()),
        multiple_results: Some("Found {result_count} matching symbols. Use 'find_symbol' on specific results for full context or narrow your search with 'kind' parameter.".to_string()),
        custom: vec![],
    });

    // Semantic search with context
    templates.insert("semantic_search_with_context".to_string(), GuidanceTemplate {
        no_results: Some("No semantic matches found. Try different phrasing or ensure documentation exists for the concepts you're searching.".to_string()),
        single_result: Some("Found one match with full context. Review the relationships to understand how this fits into the codebase.".to_string()),
        multiple_results: Some("Rich context provided for {result_count} matches. Investigate specific relationships using targeted tools like 'get_calls' or 'find_callers'.".to_string()),
        custom: vec![],
    });

    // Get index info
    templates.insert(
        "get_index_info".to_string(),
        GuidanceTemplate {
            no_results: None, // Not applicable
            single_result: Some(
                "Index statistics loaded. Use search tools to explore the codebase.".to_string(),
            ),
            multiple_results: None, // Not applicable
            custom: vec![],
        },
    );

    templates
}

pub(super) fn default_guidance_variables() -> IndexMap<String, String> {
    let mut vars = IndexMap::new();
    vars.insert("project".to_string(), "codanna".to_string());
    vars
}

/// Generate language defaults from the registry
/// This queries the language registry to get all registered languages
/// and their default configurations (sorted alphabetically)
pub(super) fn generate_language_defaults() -> IndexMap<String, LanguageConfig> {
    // Try to get languages from the registry
    if let Ok(registry) = crate::parsing::get_registry().lock() {
        // Collect to Vec for sorting
        let mut entries: Vec<_> = registry
            .iter_all()
            .map(|def| {
                (
                    def.id().as_str().to_string(),
                    LanguageConfig {
                        enabled: def.default_enabled(),
                        extensions: def.extensions().iter().map(|s| s.to_string()).collect(),
                        parser_options: HashMap::new(),
                        config_files: Vec::new(),
                        projects: Vec::new(),
                    },
                )
            })
            .collect();

        // Sort alphabetically by language name
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Build IndexMap from sorted entries (preserves order)
        let configs: IndexMap<_, _> = entries.into_iter().collect();

        // Return registry-generated configs if we got any
        if !configs.is_empty() {
            return configs;
        }
    }

    // Minimal fallback for catastrophic failure
    // Only include Rust as it's the most essential language
    fallback_minimal_languages()
}

/// Minimal fallback language configuration
/// Used only when registry is completely unavailable
pub(super) fn fallback_minimal_languages() -> IndexMap<String, LanguageConfig> {
    let mut langs = IndexMap::new();

    // Include only Rust as the minimal working configuration
    langs.insert(
        "rust".to_string(),
        LanguageConfig {
            enabled: true,
            extensions: vec!["rs".to_string()],
            parser_options: HashMap::new(),
            config_files: Vec::new(),
            projects: Vec::new(),
        },
    );

    langs
}
