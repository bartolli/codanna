//! Context stage - build resolution contexts per file
//!
//! Groups unresolved relationships by file_id and builds ResolutionContext
//! with all data needed for resolution: local symbols, imports, language.
//!
//! Data flow:
//! - Receives: Vec<UnresolvedRelationship> from Phase 1
//! - Uses: SymbolLookupCache for local symbols + language, DocumentIndex for imports
//! - Uses: ParserFactory to get LanguageBehavior per language_id (language-agnostic)
//! - Outputs: Vec<ResolutionContext> for RESOLVE stage

use crate::indexing::pipeline::types::{
    ResolutionContext, SymbolLookupCache, UnresolvedRelationship,
};
use crate::parsing::{LanguageBehavior, LanguageId, ParserFactory};
use crate::storage::DocumentIndex;
use crate::types::FileId;
use std::collections::HashMap;
use std::sync::Arc;

/// Context stage for building resolution contexts.
///
/// Groups relationships by file and gathers all data needed for resolution.
/// Uses ParserFactory to get LanguageBehavior per language (language-agnostic).
pub struct ContextStage {
    symbol_cache: Arc<SymbolLookupCache>,
    index: Arc<DocumentIndex>,
    factory: Arc<ParserFactory>,
    /// Cached behaviors by language_id (created on demand)
    behaviors: std::sync::RwLock<HashMap<LanguageId, Arc<dyn LanguageBehavior>>>,
}

impl ContextStage {
    /// Create a new context stage.
    pub fn new(
        symbol_cache: Arc<SymbolLookupCache>,
        index: Arc<DocumentIndex>,
        factory: Arc<ParserFactory>,
    ) -> Self {
        Self {
            symbol_cache,
            index,
            factory,
            behaviors: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Get or create behavior for a language (cached).
    ///
    /// Language-agnostic: delegates to ParserFactory which uses the registry.
    pub fn get_behavior(&self, language_id: LanguageId) -> Arc<dyn LanguageBehavior> {
        // Check cache first (read lock)
        {
            let cache = self.behaviors.read().unwrap();
            if let Some(behavior) = cache.get(&language_id) {
                return Arc::clone(behavior);
            }
        }

        // Create behavior (write lock)
        let mut cache = self.behaviors.write().unwrap();
        // Double-check after acquiring write lock
        if let Some(behavior) = cache.get(&language_id) {
            return Arc::clone(behavior);
        }

        // Create from factory (language-agnostic)
        let behavior: Arc<dyn LanguageBehavior> = self
            .factory
            .create_behavior_from_registry(language_id)
            .into();
        cache.insert(language_id, Arc::clone(&behavior));
        behavior
    }

    /// Get all cached behaviors for RESOLVE stage.
    pub fn behaviors(&self) -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
        self.behaviors.read().unwrap().clone()
    }

    /// Build resolution contexts from unresolved relationships.
    ///
    /// Groups relationships by file_id and enriches each context with:
    /// - Local symbols from SymbolLookupCache
    /// - Imports from DocumentIndex
    pub fn build_contexts(
        &self,
        unresolved: Vec<UnresolvedRelationship>,
    ) -> Vec<ResolutionContext> {
        // Group relationships by file_id
        let mut by_file: HashMap<FileId, Vec<UnresolvedRelationship>> = HashMap::new();

        for rel in unresolved {
            by_file.entry(rel.file_id).or_default().push(rel);
        }

        // Build context for each file
        let mut contexts = Vec::with_capacity(by_file.len());

        for (file_id, rels) in by_file {
            let context = self.build_context_for_file(file_id, rels);
            contexts.push(context);
        }

        contexts
    }

    /// Build context for a single file.
    ///
    /// Calls `behavior.build_resolution_context_with_pipeline_cache()` to get
    /// a language-specific ResolutionScope with path alias enhancement.
    /// Uses enhanced imports (path aliases resolved) for proper Tier 2 matching.
    fn build_context_for_file(
        &self,
        file_id: FileId,
        unresolved_rels: Vec<UnresolvedRelationship>,
    ) -> ResolutionContext {
        // Get local symbols from cache (O(1))
        let local_symbols = self.symbol_cache.symbols_in_file(file_id);

        // Get language_id from first local symbol (all symbols in file share same language)
        let language_id = local_symbols
            .first()
            .and_then(|id| self.symbol_cache.get(*id))
            .and_then(|sym| sym.language_id)
            .unwrap_or_else(|| LanguageId::new("unknown"));

        // Get behavior for this language
        let behavior = self.get_behavior(language_id);

        // Get raw imports from Tantivy
        let raw_imports = self.index.get_imports_for_file(file_id).unwrap_or_default();

        // Build ResolutionScope via behavior - returns (scope, enhanced_imports)
        // Enhanced imports have path aliases resolved (e.g., @/components → src.components)
        let (scope, enhanced_imports) = behavior.build_resolution_context_with_pipeline_cache(
            file_id,
            &raw_imports,
            self.symbol_cache.as_ref(),
        );

        ResolutionContext {
            file_id,
            language_id,
            imports: enhanced_imports, // Use enhanced imports for Tier 2 matching
            local_symbols,
            scope,
            unresolved_rels,
        }
    }

    /// Get statistics about the contexts built.
    pub fn stats(&self, contexts: &[ResolutionContext]) -> ContextStats {
        let total_files = contexts.len();
        let total_rels: usize = contexts.iter().map(|c| c.relationship_count()).sum();
        let total_local_symbols: usize = contexts.iter().map(|c| c.local_symbols.len()).sum();
        let total_imports: usize = contexts.iter().map(|c| c.imports.len()).sum();

        ContextStats {
            total_files,
            total_relationships: total_rels,
            total_local_symbols,
            total_imports,
        }
    }
}

/// Statistics from context building.
#[derive(Debug, Default)]
pub struct ContextStats {
    pub total_files: usize,
    pub total_relationships: usize,
    pub total_local_symbols: usize,
    pub total_imports: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::indexing::pipeline::types::SymbolLookupCache;
    use crate::parsing::LanguageId;
    use crate::symbol::Symbol;
    use crate::types::{Range, SymbolId};
    use crate::{RelationKind, SymbolKind};
    use std::sync::Arc as StdArc;
    use tempfile::TempDir;

    fn make_test_symbol(id: u32, name: &str, file_id: u32, lang: LanguageId) -> Symbol {
        let mut sym = Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(file_id).unwrap(),
            Range::new(1, 0, 10, 1),
        );
        sym.language_id = Some(lang);
        sym
    }

    fn make_unresolved(from_name: &str, to_name: &str, file_id: u32) -> UnresolvedRelationship {
        UnresolvedRelationship {
            from_id: Some(SymbolId::new(1).unwrap()),
            from_name: StdArc::from(from_name),
            to_name: StdArc::from(to_name),
            file_id: FileId::new(file_id).unwrap(),
            kind: RelationKind::Calls,
            metadata: None,
            to_range: Some(Range::new(5, 4, 5, 20)),
        }
    }

    fn make_factory() -> Arc<ParserFactory> {
        Arc::new(ParserFactory::new(Arc::new(Settings::default())))
    }

    #[test]
    fn test_context_groups_by_file() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let factory = make_factory();

        // Build symbol cache with symbols in two files (different languages)
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_test_symbol(1, "foo", 1, LanguageId::new("rust")));
        cache.insert(make_test_symbol(2, "bar", 1, LanguageId::new("rust")));
        cache.insert(make_test_symbol(3, "baz", 2, LanguageId::new("typescript")));

        // Create unresolved relationships in two files
        let unresolved = vec![
            make_unresolved("foo", "helper1", 1),
            make_unresolved("bar", "helper2", 1),
            make_unresolved("baz", "helper3", 2),
        ];

        let stage = ContextStage::new(cache, index, factory);
        let contexts = stage.build_contexts(unresolved);

        assert_eq!(contexts.len(), 2, "Expected 2 file contexts");

        // Find context for each file
        let file1_ctx = contexts
            .iter()
            .find(|c| c.file_id == FileId::new(1).unwrap());
        let file2_ctx = contexts
            .iter()
            .find(|c| c.file_id == FileId::new(2).unwrap());

        assert!(file1_ctx.is_some());
        assert!(file2_ctx.is_some());

        let file1 = file1_ctx.unwrap();
        let file2 = file2_ctx.unwrap();

        assert_eq!(file1.unresolved_rels.len(), 2);
        assert_eq!(file1.local_symbols.len(), 2);
        assert_eq!(file1.language_id.as_str(), "rust");

        assert_eq!(file2.unresolved_rels.len(), 1);
        assert_eq!(file2.local_symbols.len(), 1);
        assert_eq!(file2.language_id.as_str(), "typescript");
    }

    #[test]
    fn test_context_empty_input() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let cache = Arc::new(SymbolLookupCache::new());
        let factory = make_factory();

        let stage = ContextStage::new(cache, index, factory);
        let contexts = stage.build_contexts(vec![]);

        assert!(contexts.is_empty());
    }

    #[test]
    fn test_context_stats() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let factory = make_factory();

        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_test_symbol(1, "foo", 1, LanguageId::new("rust")));
        cache.insert(make_test_symbol(2, "bar", 1, LanguageId::new("rust")));
        cache.insert(make_test_symbol(3, "baz", 2, LanguageId::new("rust")));

        let unresolved = vec![
            make_unresolved("foo", "helper1", 1),
            make_unresolved("bar", "helper2", 1),
            make_unresolved("baz", "helper3", 2),
        ];

        let stage = ContextStage::new(cache, index, factory);
        let contexts = stage.build_contexts(unresolved);
        let stats = stage.stats(&contexts);

        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.total_relationships, 3);
        assert_eq!(stats.total_local_symbols, 3);
    }

    #[test]
    fn test_context_preserves_relationship_data() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let cache = Arc::new(SymbolLookupCache::new());
        let factory = make_factory();

        let from_id = SymbolId::new(42).unwrap();
        let file_id = FileId::new(1).unwrap();
        let to_range = Range::new(10, 5, 10, 25);

        let rel = UnresolvedRelationship {
            from_id: Some(from_id),
            from_name: StdArc::from("caller"),
            to_name: StdArc::from("callee"),
            file_id,
            kind: RelationKind::Calls,
            metadata: None,
            to_range: Some(to_range),
        };

        let stage = ContextStage::new(cache, index, factory);
        let contexts = stage.build_contexts(vec![rel]);

        assert_eq!(contexts.len(), 1);
        let ctx = &contexts[0];
        assert_eq!(ctx.unresolved_rels.len(), 1);

        let preserved = &ctx.unresolved_rels[0];
        assert_eq!(preserved.from_id, Some(from_id));
        assert_eq!(preserved.from_name.as_ref(), "caller");
        assert_eq!(preserved.to_name.as_ref(), "callee");
        assert_eq!(preserved.to_range, Some(to_range));
    }

    #[test]
    fn test_context_caches_behaviors_per_language() {
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings::default();
        let index = Arc::new(DocumentIndex::new(temp_dir.path(), &settings).unwrap());
        let factory = make_factory();

        // Create symbols for two languages
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_test_symbol(1, "foo", 1, LanguageId::new("rust")));
        cache.insert(make_test_symbol(2, "bar", 2, LanguageId::new("typescript")));

        let unresolved = vec![
            make_unresolved("foo", "helper1", 1),
            make_unresolved("bar", "helper2", 2),
        ];

        let stage = ContextStage::new(cache, index, factory);
        let _contexts = stage.build_contexts(unresolved);

        // Check behaviors are cached
        let behaviors = stage.behaviors();
        assert_eq!(
            behaviors.len(),
            2,
            "Expected 2 behaviors (rust + typescript)"
        );
        assert!(
            behaviors.contains_key(&LanguageId::new("rust")),
            "Rust behavior should be cached"
        );
        assert!(
            behaviors.contains_key(&LanguageId::new("typescript")),
            "TypeScript behavior should be cached"
        );
    }
}
