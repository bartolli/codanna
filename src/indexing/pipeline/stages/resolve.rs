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
use crate::parsing::resolution::{GenericInheritanceResolver, InheritanceResolver};
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
    /// Per-language inheritance resolvers populated from `Extends`
    /// `UnresolvedRelationship`s by CONTEXT stage; absent ⇒ fall back to
    /// empty `GenericInheritanceResolver` (`parent_of` yields `None`).
    inheritance_resolvers: HashMap<LanguageId, Arc<dyn InheritanceResolver>>,
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
            inheritance_resolvers: HashMap::new(),
        }
    }

    /// Install per-language `InheritanceResolver`s (built by CONTEXT stage
    /// from `Extends` `UnresolvedRelationship`s). Consumed by
    /// `resolve_static_call` via `behavior.expand_static_class_keyword`.
    pub fn with_inheritance_resolvers(
        mut self,
        resolvers: HashMap<LanguageId, Arc<dyn InheritanceResolver>>,
    ) -> Self {
        self.inheritance_resolvers = resolvers;
        self
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
                if self.symbol_cache.has_candidates(&unresolved.to_name) {
                    stats.unresolved_ambiguous += 1;
                } else {
                    stats.unresolved_no_candidates += 1;
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

        let from_id = unresolved.from_id?;

        let caller_symbol = self.symbol_cache.get_ref(from_id);
        let caller = caller_symbol
            .as_deref()
            .map(|sym| {
                let language_id = sym.language_id.unwrap_or(context.language_id);
                // Missing behavior cannot happen for a parsed language; the
                // "::" fallback fails closed (under-resolves, never leaks).
                let separator = self
                    .get_behavior(&language_id)
                    .map(|b| b.module_separator())
                    .unwrap_or("::");
                CallerContext::new(sym.file_id, sym.module_path.clone(), language_id, separator)
            })
            .unwrap_or_else(|| CallerContext::from_file(context.file_id, context.language_id));
        let from_kind = caller_symbol.as_deref().map(|sym| sym.kind);
        drop(caller_symbol);

        if let Some(to_id) = context.resolve(&unresolved.to_name) {
            if self.is_compatible(
                from_kind,
                to_id,
                unresolved.kind,
                caller.file_id,
                &caller.language_id,
            ) && self.is_receiver_compat(to_id, unresolved, &caller.language_id)
                && self.is_instance_type_compatible(unresolved, to_id, &caller.language_id)
            {
                return Some(ResolvedRelationship {
                    from_id,
                    to_id,
                    kind: unresolved.kind,
                    metadata: unresolved.metadata.clone(),
                });
            }
        }

        // For qualified static calls (`Type::method` / `Type.method`), the
        // tier-based `cache.resolve()` returns the local same-name candidate
        // before consulting any non-local match. Bypass tier logic and filter
        // candidates by receiver-compat directly.
        if Self::is_qualified_static_call(unresolved) {
            return self.resolve_static_call(from_id, from_kind, unresolved, &caller, context);
        }

        let result = self.symbol_cache.resolve(
            &unresolved.to_name,
            &caller,
            unresolved.to_range.as_ref(),
            &context.imports,
        );

        match result {
            ResolveResult::Found(to_id) => {
                if !self.is_compatible(
                    from_kind,
                    to_id,
                    unresolved.kind,
                    caller.file_id,
                    &caller.language_id,
                ) {
                    return None;
                }
                if !self.is_instance_type_compatible(unresolved, to_id, &caller.language_id) {
                    return None;
                }
                Some(ResolvedRelationship {
                    from_id,
                    to_id,
                    kind: unresolved.kind,
                    metadata: unresolved.metadata.clone(),
                })
            }
            ResolveResult::Ambiguous(candidates) => {
                let to_id = self.disambiguate(&candidates, unresolved, context, false)?;
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

    fn is_compatible(
        &self,
        from_kind: Option<crate::SymbolKind>,
        to_id: SymbolId,
        rel_kind: RelationKind,
        file_id: FileId,
        language_id: &LanguageId,
    ) -> bool {
        let Some(from_kind) = from_kind else {
            return true;
        };
        let Some(to_kind) = self.symbol_cache.get_ref(to_id).map(|sym| sym.kind) else {
            return true;
        };
        let Some(behavior) = self.get_behavior(language_id) else {
            return true;
        };
        behavior.is_compatible_relationship(from_kind, to_kind, rel_kind, file_id)
    }

    /// Filter candidates by static-call receiver type.
    ///
    /// Returns `None` when the filter does not apply (no metadata, no receiver,
    /// or `static_call == false`). When it applies, delegates the per-candidate
    /// match to `LanguageBehavior::is_receiver_compatible` so each language can
    /// extend the default `class_name`/`module_path` match with its own aliases.
    fn filter_by_static_receiver(
        &self,
        candidates: &[SymbolId],
        unresolved: &UnresolvedRelationship,
        language_id: &LanguageId,
    ) -> Option<Vec<SymbolId>> {
        let metadata = unresolved.metadata.as_ref()?;
        if !metadata.static_call {
            return None;
        }
        let receiver = metadata.receiver.as_deref()?;
        let behavior = self.get_behavior(language_id)?;
        let caller = unresolved
            .from_id
            .and_then(|id| self.symbol_cache.get_ref(id));

        let matches: Vec<SymbolId> = candidates
            .iter()
            .copied()
            .filter(|&id| {
                self.symbol_cache.get_ref(id).is_some_and(|sym| {
                    behavior.is_receiver_compatible(&sym, receiver, caller.as_deref())
                })
            })
            .collect();
        Some(matches)
    }

    /// Returns the inferred receiver type for an instance call and the caller
    /// symbol used for compatibility checks. `None` when the filter does not
    /// apply: no metadata, static call, no receiver, no caller, no caller
    /// signature, or the receiver does not name a parameter on the caller
    /// (or its type is out of `extract_parameter_type` scope — generics-as-type,
    /// impl/dyn Trait, tuples, fn types).
    ///
    /// Caller-signature is read directly off the `Function`/`Method` symbol;
    /// bypasses per-parser `SymbolKind::Parameter` emission (today only
    /// C/C++/Go/Lua emit Parameter symbols).
    fn infer_receiver_type<'a>(
        &'a self,
        unresolved: &UnresolvedRelationship,
        language_id: &LanguageId,
    ) -> Option<(String, impl std::ops::Deref<Target = crate::Symbol> + 'a)> {
        let metadata = unresolved.metadata.as_ref()?;
        if metadata.static_call {
            return None;
        }
        let receiver = metadata.receiver.as_deref()?;
        let behavior = self.get_behavior(language_id)?;
        let caller = self.symbol_cache.get_ref(unresolved.from_id?)?;
        let signature = caller.signature.as_deref()?;
        let type_name = behavior.extract_parameter_type(signature, receiver)?;
        Some((type_name, caller))
    }

    /// Single-candidate gate (Found arm of `resolve_one`): when the inferred
    /// receiver type is known, the candidate is compatible iff its containing
    /// class matches via `is_receiver_compatible`. When no inference data is
    /// available (filter doesn't apply), returns `true` (pass-through).
    fn is_instance_type_compatible(
        &self,
        unresolved: &UnresolvedRelationship,
        to_id: SymbolId,
        language_id: &LanguageId,
    ) -> bool {
        let Some((type_name, caller)) = self.infer_receiver_type(unresolved, language_id) else {
            return true;
        };
        let Some(behavior) = self.get_behavior(language_id) else {
            return true;
        };
        let Some(candidate) = self.symbol_cache.get_ref(to_id) else {
            return true;
        };
        behavior.is_receiver_compatible(&candidate, &type_name, Some(&*caller))
    }

    /// Filter candidates by inferred parameter-type for instance calls.
    ///
    /// Called from `disambiguate()` for the multi-candidate (Ambiguous) path;
    /// the single-candidate (Found) path is gated by `is_instance_type_compatible`.
    fn filter_by_instance_receiver_type(
        &self,
        candidates: &[SymbolId],
        unresolved: &UnresolvedRelationship,
        language_id: &LanguageId,
    ) -> Option<Vec<SymbolId>> {
        let (type_name, caller) = self.infer_receiver_type(unresolved, language_id)?;
        let behavior = self.get_behavior(language_id)?;
        let matches: Vec<SymbolId> = candidates
            .iter()
            .copied()
            .filter(|&id| {
                self.symbol_cache.get_ref(id).is_some_and(|sym| {
                    behavior.is_receiver_compatible(&sym, &type_name, Some(&*caller))
                })
            })
            .collect();
        Some(matches)
    }

    fn is_qualified_static_call(unresolved: &UnresolvedRelationship) -> bool {
        if unresolved.kind != RelationKind::Calls {
            return false;
        }
        let Some(metadata) = unresolved.metadata.as_ref() else {
            return false;
        };
        metadata.static_call && metadata.receiver.is_some()
    }

    /// Resolution path for qualified static calls.
    ///
    /// Bypasses the tier-based `cache.resolve()` (which prefers a local
    /// same-name match before consulting non-locals). Filters the full
    /// candidate set by `LanguageBehavior::is_receiver_compatible` and
    /// kind-compatibility, then delegates to `disambiguate()` when more
    /// than one candidate survives.
    ///
    /// Pre-gate: `behavior.expand_static_class_keyword` rewrites keyword
    /// receivers (PHP `self`/`static`/`parent`) to concrete class names.
    /// Resolver is an empty `GenericInheritanceResolver`; arms that consult
    /// `parent_of` yield `None` until the resolver is populated.
    fn resolve_static_call(
        &self,
        from_id: SymbolId,
        from_kind: Option<crate::SymbolKind>,
        unresolved: &UnresolvedRelationship,
        caller: &CallerContext,
        context: &ResolutionContext,
    ) -> Option<ResolvedRelationship> {
        let raw_receiver = unresolved.metadata.as_ref()?.receiver.as_deref()?;
        let behavior = self.get_behavior(&caller.language_id)?;
        let caller_symbol = unresolved
            .from_id
            .and_then(|id| self.symbol_cache.get_ref(id));

        let fallback;
        let resolver: &dyn InheritanceResolver =
            match self.inheritance_resolvers.get(&caller.language_id) {
                Some(r) => r.as_ref(),
                None => {
                    fallback = GenericInheritanceResolver::new();
                    &fallback
                }
            };
        let expanded =
            behavior.expand_static_class_keyword(raw_receiver, caller_symbol.as_deref(), resolver);
        let receiver: &str = expanded.as_deref().unwrap_or(raw_receiver);

        let filtered: Vec<SymbolId> = self
            .symbol_cache
            .lookup_candidates(&unresolved.to_name)
            .into_iter()
            .filter(|&id| {
                if !self.is_compatible(
                    from_kind,
                    id,
                    unresolved.kind,
                    caller.file_id,
                    &caller.language_id,
                ) {
                    return false;
                }
                self.symbol_cache.get_ref(id).is_some_and(|sym| {
                    behavior.is_receiver_compatible(&sym, receiver, caller_symbol.as_deref())
                })
            })
            .collect();

        let to_id = match filtered.len() {
            0 => return None,
            1 => filtered[0],
            _ => self.disambiguate(&filtered, unresolved, context, true)?,
        };
        Some(ResolvedRelationship {
            from_id,
            to_id,
            kind: unresolved.kind,
            metadata: unresolved.metadata.clone(),
        })
    }

    /// Whether a single resolved candidate is receiver-compatible.
    ///
    /// Returns `true` when the filter does not apply (no metadata, no receiver,
    /// or `static_call == false`) — the caller should not reject in that case.
    /// Otherwise consults `LanguageBehavior::is_receiver_compatible`.
    fn is_receiver_compat(
        &self,
        to_id: SymbolId,
        unresolved: &UnresolvedRelationship,
        language_id: &LanguageId,
    ) -> bool {
        let Some(metadata) = unresolved.metadata.as_ref() else {
            return true;
        };
        if !metadata.static_call {
            return true;
        }
        let Some(receiver) = metadata.receiver.as_deref() else {
            return true;
        };
        let Some(candidate) = self.symbol_cache.get_ref(to_id) else {
            return true;
        };
        let Some(behavior) = self.get_behavior(language_id) else {
            return true;
        };
        let caller = unresolved
            .from_id
            .and_then(|id| self.symbol_cache.get_ref(id));
        behavior.is_receiver_compatible(&candidate, receiver, caller.as_deref())
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
        static_pre_filtered: bool,
    ) -> Option<SymbolId> {
        let file_id = context.file_id;
        let language_id = &context.language_id;

        let from_kind = unresolved
            .from_id
            .and_then(|id| self.symbol_cache.get_ref(id))
            .map(|sym| sym.kind);

        let filtered: Vec<SymbolId> = candidates
            .iter()
            .copied()
            .filter(|&id| self.is_compatible(from_kind, id, unresolved.kind, file_id, language_id))
            .collect();
        if filtered.is_empty() {
            return None;
        }

        // Static-call disambiguation: when the call is qualified (`Type::method`
        // or `Type.method`), filter to candidates whose containing type matches
        // the receiver. Skipped when `resolve_static_call` already applied the
        // same filter before delegating here.
        if !static_pre_filtered && unresolved.kind == RelationKind::Calls {
            if let Some(survivors) =
                self.filter_by_static_receiver(&filtered, unresolved, language_id)
            {
                if survivors.len() == 1 {
                    return Some(survivors[0]);
                }
            }
        }
        // Instance-call disambiguation via inferred parameter type: when the
        // receiver names a parameter on the caller's signature, filter candidates
        // to those whose containing type matches the inferred type. Zero
        // survivors → NotFound (don't fall through to a wrong-class same-name
        // pick); single survivor wins; multiple fall through.
        if unresolved.kind == RelationKind::Calls {
            if let Some(survivors) =
                self.filter_by_instance_receiver_type(&filtered, unresolved, language_id)
            {
                match survivors.len() {
                    1 => return Some(survivors[0]),
                    0 => return None,
                    _ => {}
                }
            }
        }
        let candidates = &filtered[..];

        let mut local_matches: Vec<SymbolId> = Vec::new();
        let mut imported_matches: Vec<SymbolId> = Vec::new();
        let mut language_matches: Vec<SymbolId> = Vec::new();

        for &candidate_id in candidates {
            if let Some(symbol) = self.symbol_cache.get_ref(candidate_id) {
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

        // No appropriate match found - don't resolve cross-language
        // Return None to prevent incorrect relationships (e.g., Java -> JavaScript)
        None
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
            if let Some(symbol) = self.symbol_cache.get_ref(candidate_id) {
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
            .and_then(|id| self.symbol_cache.get_ref(*id))
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

                // Re-exported path: the import names a module namespace
                // binding that resolves to this symbol's definition site.
                if self.symbol_cache.resolve_module_alias(&import.path) == Some(symbol.id) {
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
