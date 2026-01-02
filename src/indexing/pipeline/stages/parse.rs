//! Parse stage - parallel file parsing
//!
//! Converts FileContent into ParsedFile with RawSymbols.
//! Uses thread-local parsers to avoid contention.

use crate::Settings;
use crate::indexing::pipeline::types::{
    FileContent, ParsedFile, PipelineError, PipelineResult, RawImport, RawRelationship, RawSymbol,
};
use crate::parsing::{LanguageId, LanguageParser, get_registry};
use crate::types::{FileId, SymbolCounter};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Thread-local parser cache.
///
/// Each thread maintains its own set of parsers to avoid contention.
/// tree-sitter parsers are not Send, so this pattern is required.
struct ParserCache {
    parsers: HashMap<LanguageId, Box<dyn LanguageParser>>,
    settings: Arc<Settings>,
}

impl ParserCache {
    fn new(settings: Arc<Settings>) -> Self {
        Self {
            parsers: HashMap::new(),
            settings,
        }
    }

    fn get_or_create(
        &mut self,
        language_id: LanguageId,
    ) -> PipelineResult<&mut dyn LanguageParser> {
        if !self.parsers.contains_key(&language_id) {
            let parser = create_parser(language_id, &self.settings)?;
            self.parsers.insert(language_id, parser);
        }
        Ok(self.parsers.get_mut(&language_id).unwrap().as_mut())
    }
}

thread_local! {
    static PARSER_CACHE: RefCell<Option<ParserCache>> = const { RefCell::new(None) };
}

/// Initialize thread-local parser cache for current thread.
pub fn init_parser_cache(settings: Arc<Settings>) {
    PARSER_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(ParserCache::new(settings));
    });
}

/// Create a parser for the given language.
fn create_parser(
    language_id: LanguageId,
    settings: &Settings,
) -> PipelineResult<Box<dyn LanguageParser>> {
    let registry = get_registry();
    let registry = registry.lock().map_err(|e| PipelineError::Parse {
        path: Default::default(),
        reason: format!("Failed to acquire registry lock: {e}"),
    })?;

    registry
        .create_parser(language_id, settings)
        .map_err(|e| PipelineError::Parse {
            path: Default::default(),
            reason: e.to_string(),
        })
}

/// Detect language from file extension.
fn detect_language(path: &Path) -> PipelineResult<LanguageId> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    let registry = get_registry();
    let registry = registry.lock().map_err(|e| PipelineError::Parse {
        path: path.to_path_buf(),
        reason: format!("Failed to acquire registry lock: {e}"),
    })?;

    registry
        .get_by_extension(extension)
        .map(|def| def.id())
        .ok_or_else(|| PipelineError::UnsupportedFileType {
            path: path.to_path_buf(),
        })
}

/// Parse stage configuration.
#[derive(Debug, Clone)]
pub struct ParseStage {
    settings: Arc<Settings>,
}

impl ParseStage {
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }

    /// Get the settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Parse a file using this stage's settings.
    pub fn parse(&self, content: FileContent) -> PipelineResult<ParsedFile> {
        parse_file(content, &self.settings)
    }
}

/// Parse a single file into a ParsedFile.
///
/// This is the core parsing function. It:
/// 1. Detects the language from file extension
/// 2. Gets or creates a thread-local parser
/// 3. Extracts symbols, imports, and relationships
/// 4. Returns ParsedFile with RawSymbols (no IDs assigned)
pub fn parse_file(content: FileContent, settings: &Settings) -> PipelineResult<ParsedFile> {
    let language_id = detect_language(&content.path)?;

    PARSER_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let parser_cache = cache_ref
            .as_mut()
            .expect("Parser cache not initialized. Call init_parser_cache first.");

        let parser = parser_cache.get_or_create(language_id)?;

        parse_with_parser(content, language_id, parser, settings)
    })
}

/// Parse content using provided parser.
fn parse_with_parser(
    content: FileContent,
    language_id: LanguageId,
    parser: &mut dyn LanguageParser,
    settings: &Settings,
) -> PipelineResult<ParsedFile> {
    // Use a dummy file_id and counter - we just need to extract symbols
    // Real IDs are assigned in COLLECT stage
    let dummy_file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    // Compute module_path using the language behavior
    let module_path = compute_module_path(&content.path, language_id, settings);

    // Parse symbols
    let symbols = parser.parse(&content.content, dummy_file_id, &mut counter);

    // Convert to RawSymbols (strip the dummy ID)
    let raw_symbols: Vec<RawSymbol> = symbols
        .into_iter()
        .map(|sym| {
            let mut raw = RawSymbol::new(sym.name.clone(), sym.kind, sym.range);
            if let Some(sig) = sym.signature {
                raw = raw.with_signature(sig);
            }
            if let Some(doc) = sym.doc_comment {
                raw = raw.with_doc_comment(doc);
            }
            raw = raw.with_visibility(sym.visibility);
            if let Some(ctx) = sym.scope_context {
                raw = raw.with_scope_context(ctx);
            }
            raw
        })
        .collect();

    // Extract imports (without FileId)
    let imports = parser.find_imports(&content.content, dummy_file_id);
    let raw_imports: Vec<RawImport> = imports
        .into_iter()
        .map(|imp| {
            let mut raw = RawImport::new(&imp.path);
            if let Some(alias) = imp.alias {
                raw = raw.with_alias(alias);
            }
            if imp.is_glob {
                raw = raw.as_glob();
            }
            if imp.is_type_only {
                raw = raw.as_type_only();
            }
            raw
        })
        .collect();

    // Extract relationships
    let raw_relationships = extract_relationships(parser, &content.content);

    Ok(ParsedFile {
        path: content.path,
        content_hash: content.hash,
        language_id,
        module_path,
        raw_symbols,
        raw_imports,
        raw_relationships,
    })
}

/// Compute module_path for a file using the language behavior.
///
/// This calls behavior.module_path_from_file() which uses:
/// - For Rust: crate:: path from file location
/// - For Java/Swift: package from source root via resolution rules
/// - For TypeScript/JavaScript: path relative to tsconfig/jsconfig
/// - For other languages: path relative to project root
fn compute_module_path(
    file_path: &Path,
    language_id: LanguageId,
    settings: &Settings,
) -> Option<String> {
    let registry = get_registry();
    let registry_guard = registry.lock().ok()?;
    let definition = registry_guard.get(language_id)?;
    let behavior = definition.create_behavior();

    let project_root = settings
        .workspace_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));

    behavior.module_path_from_file(file_path, project_root)
}

/// Extract relationships from parsed content.
///
/// Range semantics:
/// - `from_range`: Definition location of the calling/containing symbol (for COLLECT lookup)
/// - `to_range`: Call site / reference location (for Phase 2 disambiguation)
///
/// For MethodCall: `caller_range` provides precise from_range when available.
/// For legacy find_* methods: range typically points to the reference site.
fn extract_relationships(parser: &mut dyn LanguageParser, content: &str) -> Vec<RawRelationship> {
    let mut relationships = Vec::new();

    // Function/method calls - MethodCall provides caller_range for precise lookup
    for call in parser.find_method_calls(content) {
        // Use caller_range when available, otherwise use call site (triggers fallback)
        let from_range = call.caller_range.unwrap_or(call.range);
        relationships.push(RawRelationship::new(
            call.caller,
            from_range,
            call.method_name,
            call.range, // to_range = call site
            crate::RelationKind::Calls,
        ));
    }

    // Plain function calls (legacy - no caller_range available)
    for (caller, called, call_site) in parser.find_calls(content) {
        // Avoid duplicates - method_calls should be comprehensive
        // but some parsers might return both
        let already_exists = relationships.iter().any(|r| {
            r.from_name.as_ref() == caller
                && r.to_name.as_ref() == called
                && r.to_range.start_line == call_site.start_line
        });
        if !already_exists {
            // from_range = call_site triggers fallback to name-only lookup in COLLECT
            relationships.push(RawRelationship::new(
                caller,
                call_site, // no caller_range available, use call_site
                called,
                call_site, // to_range = call site
                crate::RelationKind::Calls,
            ));
        }
    }

    // Trait implementations - range is the impl definition site
    for (type_name, trait_name, impl_range) in parser.find_implementations(content) {
        relationships.push(RawRelationship::new(
            type_name,
            impl_range, // from_range = where impl is defined
            trait_name,
            impl_range, // to_range = where trait is referenced
            crate::RelationKind::Implements,
        ));
    }

    // Inheritance (extends) - range is the class definition site
    for (derived, base, class_range) in parser.find_extends(content) {
        relationships.push(RawRelationship::new(
            derived,
            class_range, // from_range = where derived is defined
            base,
            class_range, // to_range = where base is referenced
            crate::RelationKind::Extends,
        ));
    }

    // Type usage - range is the usage site
    for (context, used_type, usage_range) in parser.find_uses(content) {
        relationships.push(RawRelationship::new(
            context,
            usage_range, // from_range = usage context (triggers fallback)
            used_type,
            usage_range, // to_range = where type is used
            crate::RelationKind::Uses,
        ));
    }

    // Method definitions (Defines relationships)
    for (definer, method, def_range) in parser.find_defines(content) {
        relationships.push(RawRelationship::new(
            definer,
            def_range, // from_range = where definer is
            method,
            def_range, // to_range = where method is defined
            crate::RelationKind::Defines,
        ));
    }

    relationships
}

/// Compute content hash using FNV-1a.
pub fn compute_hash(content: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in content {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Range;

    #[test]
    fn test_compute_hash() {
        let hash1 = compute_hash(b"hello world");
        let hash2 = compute_hash(b"hello world");
        let hash3 = compute_hash(b"different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_detect_language_rust() {
        let path = Path::new("test.rs");
        let result = detect_language(path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "rust");
    }

    #[test]
    fn test_detect_language_typescript() {
        let path = Path::new("app.ts");
        let result = detect_language(path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "typescript");
    }

    #[test]
    fn test_detect_language_unknown() {
        let path = Path::new("file.xyz");
        let result = detect_language(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_file_rust() {
        let settings = Arc::new(Settings::default());
        init_parser_cache(settings.clone());

        let content = FileContent::new(
            "test.rs".into(),
            r#"
fn hello() {
    println!("Hello");
}

pub struct Foo {
    value: i32,
}
"#
            .to_string(),
            "abc123def456".to_string(),
        );

        let result = parse_file(content, &settings);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.language_id.as_str(), "rust");
        assert!(!parsed.raw_symbols.is_empty());

        // Should have at least hello function and Foo struct
        let names: Vec<&str> = parsed.raw_symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"Foo"));
    }

    #[test]
    fn test_raw_symbol_has_no_id() {
        // RawSymbol intentionally has no id field
        let sym = RawSymbol::new("test", crate::SymbolKind::Function, Range::new(1, 0, 1, 10));

        // This test documents that RawSymbol does NOT have an id field
        // If this compiles, the test passes
        assert_eq!(sym.name.as_ref(), "test");
    }
}
