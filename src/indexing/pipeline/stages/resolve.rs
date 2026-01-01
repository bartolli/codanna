//! Resolve stage - symbol resolution using cached metadata
//!
//! Resolves unresolved relationships to concrete SymbolId pairs.
//! Uses SymbolLookupCache for O(1) lookups instead of Tantivy queries.
//! Uses LanguageBehavior for language-specific import matching.
//!
//! Resolution strategy:
//! 1. Get candidates by name: cache.lookup_candidates(to_name)
//! 2. Get full metadata for each: cache.get(candidate_id)
//! 3. Disambiguate using: file_id, language_id, imports, module_path
//! 4. Use behavior.import_matches_symbol() for proper import matching
//! 5. Produce ResolvedRelationship with (from_id, to_id, kind, metadata)
//!
//! Two-pass execution:
//! - Pass 1: Resolve Defines relationships
//! - Pass 2: Resolve Calls (can reference Defines from Pass 1)

use crate::indexing::pipeline::types::{
    CallerContext, ResolutionContext, ResolvedBatch, ResolvedRelationship, SymbolLookupCache,
    UnresolvedRelationship,
};
use crate::parsing::{Import, LanguageBehavior, LanguageId};
use crate::types::{FileId, SymbolId};
use crate::{RelationKind, Symbol};
use std::collections::HashMap;
use std::sync::Arc;

/// Resolve stage for symbol resolution.
///
/// Language-agnostic: delegates language-specific logic to LanguageBehavior.
pub struct ResolveStage {
    symbol_cache: Arc<SymbolLookupCache>,
    /// Behaviors by language_id (from CONTEXT stage)
    behaviors: HashMap<LanguageId, Arc<dyn LanguageBehavior>>,
}

/// Statistics from resolution.
#[derive(Debug, Default)]
pub struct ResolveStats {
    /// Total relationships processed
    pub total_processed: usize,
    /// Successfully resolved
    pub resolved: usize,
    /// Failed to resolve (no candidates)
    pub unresolved_no_candidates: usize,
    /// Failed to resolve (ambiguous - multiple candidates, couldn't disambiguate)
    pub unresolved_ambiguous: usize,
    /// Defines resolved
    pub defines_resolved: usize,
    /// Calls resolved
    pub calls_resolved: usize,
}

impl ResolveStage {
    /// Create a new resolve stage with the symbol cache and behaviors.
    ///
    /// Behaviors are provided by CONTEXT stage, keyed by language_id.
    pub fn new(
        symbol_cache: Arc<SymbolLookupCache>,
        behaviors: HashMap<LanguageId, Arc<dyn LanguageBehavior>>,
    ) -> Self {
        Self {
            symbol_cache,
            behaviors,
        }
    }

    /// Get behavior for a language, if available.
    fn get_behavior(&self, language_id: &LanguageId) -> Option<&Arc<dyn LanguageBehavior>> {
        self.behaviors.get(language_id)
    }

    /// Resolve all relationships in a context.
    ///
    /// Returns resolved batch and statistics.
    pub fn resolve(&self, context: &ResolutionContext) -> (ResolvedBatch, ResolveStats) {
        let mut batch = ResolvedBatch::with_capacity(context.unresolved_rels.len());
        let mut stats = ResolveStats::default();

        for unresolved in &context.unresolved_rels {
            stats.total_processed += 1;

            if let Some(resolved) = self.resolve_one(unresolved, context) {
                match resolved.kind {
                    RelationKind::Defines => stats.defines_resolved += 1,
                    RelationKind::Calls => stats.calls_resolved += 1,
                    _ => {}
                }
                stats.resolved += 1;
                batch.push(resolved);
            } else {
                // Track why resolution failed
                let candidates = self.symbol_cache.lookup_candidates(&unresolved.to_name);
                if candidates.is_empty() {
                    stats.unresolved_no_candidates += 1;
                } else {
                    stats.unresolved_ambiguous += 1;
                }
            }
        }

        (batch, stats)
    }

    /// Resolve a single relationship.
    ///
    /// Uses PipelineSymbolCache.resolve() with CallerContext:
    /// - caller: file, module, language of the calling symbol
    /// - to_range: call site for shadowing disambiguation
    /// - imports: enhanced by behavior (path aliases resolved)
    fn resolve_one(
        &self,
        unresolved: &UnresolvedRelationship,
        context: &ResolutionContext,
    ) -> Option<ResolvedRelationship> {
        use crate::parsing::{PipelineSymbolCache, ResolveResult};

        // Must have from_id (assigned by COLLECT stage)
        let from_id = unresolved.from_id?;

        // Build CallerContext from the calling symbol
        // This gives us file_id, module_path, and language_id for visibility checks
        let caller = self
            .symbol_cache
            .get(from_id)
            .map(|sym| {
                CallerContext::new(
                    sym.file_id,
                    sym.module_path.clone(),
                    sym.language_id.unwrap_or(context.language_id),
                )
            })
            .unwrap_or_else(|| CallerContext::from_file(context.file_id, context.language_id));

        // Use cache.resolve() with CallerContext (imports enhanced by behavior)
        let result = self.symbol_cache.resolve(
            &unresolved.to_name,
            &caller,
            unresolved.to_range.as_ref(),
            &context.imports,
        );

        match result {
            ResolveResult::Found(to_id) => Some(ResolvedRelationship {
                from_id,
                to_id,
                kind: unresolved.kind,
                metadata: unresolved.metadata.clone(),
            }),
            ResolveResult::Ambiguous(candidates) => {
                // Multiple candidates - use behavior for disambiguation
                let to_id = self.disambiguate(&candidates, unresolved, context)?;
                Some(ResolvedRelationship {
                    from_id,
                    to_id,
                    kind: unresolved.kind,
                    metadata: unresolved.metadata.clone(),
                })
            }
            ResolveResult::NotFound => None,
        }
    }

    /// Disambiguate among multiple candidates.
    ///
    /// Priority order:
    /// 1. Local symbols (same file_id)
    /// 2. Imported symbols (via import statements)
    /// 3. Same language
    /// 4. First candidate (fallback)
    fn disambiguate(
        &self,
        candidates: &[SymbolId],
        unresolved: &UnresolvedRelationship,
        context: &ResolutionContext,
    ) -> Option<SymbolId> {
        let file_id = context.file_id;
        let language_id = &context.language_id;

        // Collect candidate metadata
        let mut local_matches: Vec<SymbolId> = Vec::new();
        let mut imported_matches: Vec<SymbolId> = Vec::new();
        let mut language_matches: Vec<SymbolId> = Vec::new();

        for &candidate_id in candidates {
            if let Some(symbol) = self.symbol_cache.get(candidate_id) {
                // Priority 1: Local symbol (same file)
                if symbol.file_id == file_id {
                    local_matches.push(candidate_id);
                    continue;
                }

                // Priority 2: Imported symbol (uses behavior for language-specific matching)
                if self.is_imported(&symbol, &context.imports, context) {
                    imported_matches.push(candidate_id);
                    continue;
                }

                // Priority 3: Same language
                if symbol.language_id.as_ref() == Some(language_id) {
                    language_matches.push(candidate_id);
                }
            }
        }

        // Return best match by priority
        if local_matches.len() == 1 {
            return Some(local_matches[0]);
        }
        if !local_matches.is_empty() {
            // Multiple local matches - use range for disambiguation
            if let Some(to_range) = &unresolved.to_range {
                // Find symbol whose range contains or is closest to call site
                return self.find_closest_by_range(&local_matches, to_range, file_id);
            }
            return Some(local_matches[0]);
        }

        if imported_matches.len() == 1 {
            return Some(imported_matches[0]);
        }
        if !imported_matches.is_empty() {
            return Some(imported_matches[0]);
        }

        if language_matches.len() == 1 {
            return Some(language_matches[0]);
        }
        if !language_matches.is_empty() {
            return Some(language_matches[0]);
        }

        // Fallback: first candidate
        candidates.first().copied()
    }

    /// Find the symbol closest to the call site by range.
    ///
    /// For scope resolution: prefer symbols defined BEFORE the call site,
    /// with the closest one winning (most recent definition shadows earlier ones).
    fn find_closest_by_range(
        &self,
        candidates: &[SymbolId],
        call_range: &crate::types::Range,
        file_id: FileId,
    ) -> Option<SymbolId> {
        let call_line = call_range.start_line;

        // Find symbols defined before the call site, pick the closest one
        let mut best: Option<(SymbolId, u32)> = None; // (id, definition_line)

        for &candidate_id in candidates {
            if let Some(symbol) = self.symbol_cache.get(candidate_id) {
                // Only consider symbols in the same file
                if symbol.file_id != file_id {
                    continue;
                }

                let def_line = symbol.range.start_line;

                // Symbol must be defined before call site
                if def_line <= call_line {
                    match best {
                        Some((_, best_line)) if def_line > best_line => {
                            // This symbol is defined later (closer to call) - it shadows
                            best = Some((candidate_id, def_line));
                        }
                        None => {
                            best = Some((candidate_id, def_line));
                        }
                        _ => {}
                    }
                }
            }
        }

        best.map(|(id, _)| id)
            .or_else(|| candidates.first().copied())
    }

    /// Check if a symbol is imported in the given imports.
    ///
    /// Uses LanguageBehavior::import_matches_symbol() for language-specific matching.
    /// Falls back to naive matching if no behavior available.
    fn is_imported(
        &self,
        symbol: &Symbol,
        imports: &[Import],
        context: &ResolutionContext,
    ) -> bool {
        let symbol_name = symbol.name.as_ref();
        let symbol_module_path = symbol.module_path.as_deref().unwrap_or("");

        // Get importing module path for relative import resolution
        // Use first local symbol's module path, or empty string
        let importing_module = context
            .local_symbols
            .first()
            .and_then(|id| self.symbol_cache.get(*id))
            .and_then(|s| s.module_path.as_deref().map(String::from));
        let importing_mod_ref = importing_module.as_deref();

        // Try language-specific matching first (via behavior)
        if let Some(behavior) = self.get_behavior(&context.language_id) {
            for import in imports {
                // Use behavior's import_matches_symbol for proper path resolution
                // This handles tsconfig paths, relative imports, etc.
                if behavior.import_matches_symbol(
                    &import.path,
                    symbol_module_path,
                    importing_mod_ref,
                ) {
                    return true;
                }

                // Also check alias
                if let Some(alias) = &import.alias {
                    if alias == symbol_name {
                        return true;
                    }
                }
            }
            return false;
        }

        // Fallback: naive matching (no behavior available)
        for import in imports {
            // Check if import path ends with symbol name
            if import.path.ends_with(symbol_name) {
                return true;
            }

            // Check alias
            if let Some(alias) = &import.alias {
                if alias == symbol_name {
                    return true;
                }
            }

            // Check if import is from same file as symbol
            if import.file_id == symbol.file_id {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolKind;
    use crate::parsing::{LanguageId, ResolutionScope, ScopeLevel, ScopeType};
    use crate::types::Range;
    use std::sync::Arc as StdArc;

    /// No-op resolution scope for testing RESOLVE stage logic.
    /// Tests use cache.resolve() with full context - scope is just context preparation.
    struct NoOpScope;

    impl ResolutionScope for NoOpScope {
        fn add_symbol(&mut self, _name: String, _symbol_id: SymbolId, _scope_level: ScopeLevel) {}
        fn resolve(&self, _name: &str) -> Option<SymbolId> {
            None
        }
        fn clear_local_scope(&mut self) {}
        fn enter_scope(&mut self, _scope_type: ScopeType) {}
        fn exit_scope(&mut self) {}
        fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
            vec![]
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// Helper to create ResolutionContext with NoOpScope for tests.
    fn make_context(
        file_id: u32,
        language_id: LanguageId,
        local_symbols: Vec<SymbolId>,
        unresolved_rels: Vec<UnresolvedRelationship>,
    ) -> ResolutionContext {
        ResolutionContext {
            file_id: FileId::new(file_id).unwrap(),
            language_id,
            imports: vec![],
            local_symbols,
            scope: Box::new(NoOpScope),
            unresolved_rels,
        }
    }

    fn make_symbol(id: u32, name: &str, file_id: u32, lang: LanguageId) -> Symbol {
        let mut sym = Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(file_id).unwrap(),
            Range::new(1, 0, 10, 1),
        );
        sym.language_id = Some(lang);
        // Default to Public for test symbols (visibility filtering applies to cross-file)
        sym.visibility = crate::Visibility::Public;
        sym
    }

    fn make_unresolved(
        from_id: u32,
        to_name: &str,
        file_id: u32,
        kind: RelationKind,
    ) -> UnresolvedRelationship {
        UnresolvedRelationship {
            from_id: Some(SymbolId::new(from_id).unwrap()),
            from_name: StdArc::from("caller"),
            to_name: StdArc::from(to_name),
            file_id: FileId::new(file_id).unwrap(),
            kind,
            metadata: None,
            to_range: Some(Range::new(5, 4, 5, 20)),
        }
    }

    /// Helper to create stage with empty behaviors (uses naive fallback matching)
    fn make_stage(cache: Arc<SymbolLookupCache>) -> ResolveStage {
        ResolveStage::new(cache, HashMap::new())
    }

    #[test]
    fn test_resolve_single_candidate() {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        cache.insert(make_symbol(2, "helper", 1, LanguageId::new("rust")));

        let stage = make_stage(cache);

        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap(), SymbolId::new(2).unwrap()],
            vec![make_unresolved(1, "helper", 1, RelationKind::Calls)],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.total_processed, 1);
        assert_eq!(stats.resolved, 1);
        assert_eq!(stats.calls_resolved, 1);
        assert_eq!(batch.len(), 1);

        let resolved = &batch.relationships[0];
        assert_eq!(resolved.from_id, SymbolId::new(1).unwrap());
        assert_eq!(resolved.to_id, SymbolId::new(2).unwrap());
        assert_eq!(resolved.kind, RelationKind::Calls);
    }

    #[test]
    fn test_resolve_prefers_local_symbol() {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        cache.insert(make_symbol(2, "helper", 1, LanguageId::new("rust"))); // Local
        cache.insert(make_symbol(3, "helper", 2, LanguageId::new("rust"))); // Different file

        let stage = make_stage(cache);

        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap(), SymbolId::new(2).unwrap()],
            vec![make_unresolved(1, "helper", 1, RelationKind::Calls)],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1);

        // Should resolve to local helper (id=2), not remote (id=3)
        let resolved = &batch.relationships[0];
        assert_eq!(resolved.to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn test_resolve_no_candidates() {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        // No "helper" symbol

        let stage = make_stage(cache);

        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap()],
            vec![make_unresolved(1, "helper", 1, RelationKind::Calls)],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.total_processed, 1);
        assert_eq!(stats.resolved, 0);
        assert_eq!(stats.unresolved_no_candidates, 1);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_resolve_missing_from_id() {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "helper", 1, LanguageId::new("rust")));

        let stage = make_stage(cache);

        // Unresolved with no from_id
        let unresolved = UnresolvedRelationship {
            from_id: None, // Missing!
            from_name: StdArc::from("unknown"),
            to_name: StdArc::from("helper"),
            file_id: FileId::new(1).unwrap(),
            kind: RelationKind::Calls,
            metadata: None,
            to_range: None,
        };

        let context = make_context(1, LanguageId::new("rust"), vec![], vec![unresolved]);

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.total_processed, 1);
        assert_eq!(stats.resolved, 0);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_resolve_prefers_same_language() {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        cache.insert(make_symbol(2, "format", 2, LanguageId::new("rust"))); // Same language
        cache.insert(make_symbol(3, "format", 3, LanguageId::new("python"))); // Different language

        let stage = make_stage(cache);

        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap()],
            vec![make_unresolved(1, "format", 1, RelationKind::Calls)],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1);

        // Should resolve to Rust format (id=2), not Python (id=3)
        let resolved = &batch.relationships[0];
        assert_eq!(resolved.to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn test_resolve_range_disambiguation() {
        // Two symbols with same name at different lines
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));

        // helper defined at line 5
        let mut helper1 = make_symbol(2, "helper", 1, LanguageId::new("rust"));
        helper1.range = Range::new(5, 0, 10, 1);
        cache.insert(helper1);

        // helper defined at line 15 (shadows the first one for calls after line 15)
        let mut helper2 = make_symbol(3, "helper", 1, LanguageId::new("rust"));
        helper2.range = Range::new(15, 0, 20, 1);
        cache.insert(helper2);

        let stage = make_stage(cache);

        // Call at line 12 - should resolve to helper1 (defined at line 5, before call)
        let unresolved_early = UnresolvedRelationship {
            from_id: Some(SymbolId::new(1).unwrap()),
            from_name: StdArc::from("caller"),
            to_name: StdArc::from("helper"),
            file_id: FileId::new(1).unwrap(),
            kind: RelationKind::Calls,
            metadata: None,
            to_range: Some(Range::new(12, 4, 12, 20)), // Call at line 12
        };

        // Call at line 25 - should resolve to helper2 (defined at line 15, closer to call)
        let unresolved_late = UnresolvedRelationship {
            from_id: Some(SymbolId::new(1).unwrap()),
            from_name: StdArc::from("caller"),
            to_name: StdArc::from("helper"),
            file_id: FileId::new(1).unwrap(),
            kind: RelationKind::Calls,
            metadata: None,
            to_range: Some(Range::new(25, 4, 25, 20)), // Call at line 25
        };

        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![
                SymbolId::new(1).unwrap(),
                SymbolId::new(2).unwrap(),
                SymbolId::new(3).unwrap(),
            ],
            vec![unresolved_early, unresolved_late],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 2);
        assert_eq!(batch.len(), 2);

        // First call (line 12) resolves to helper1 (id=2, defined at line 5)
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());

        // Second call (line 25) resolves to helper2 (id=3, defined at line 15)
        assert_eq!(batch.relationships[1].to_id, SymbolId::new(3).unwrap());
    }

    #[test]
    fn test_resolve_stats_tracks_kinds() {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "MyStruct", 1, LanguageId::new("rust")));
        cache.insert(make_symbol(2, "field", 1, LanguageId::new("rust")));
        cache.insert(make_symbol(3, "helper", 1, LanguageId::new("rust")));

        let stage = make_stage(cache);

        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![
                SymbolId::new(1).unwrap(),
                SymbolId::new(2).unwrap(),
                SymbolId::new(3).unwrap(),
            ],
            vec![
                make_unresolved(1, "field", 1, RelationKind::Defines),
                make_unresolved(1, "helper", 1, RelationKind::Calls),
            ],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.total_processed, 2);
        assert_eq!(stats.resolved, 2);
        assert_eq!(stats.defines_resolved, 1);
        assert_eq!(stats.calls_resolved, 1);
        assert_eq!(batch.len(), 2);
    }
}
