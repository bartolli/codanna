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

        // `super()` receivers name a target the index already holds:
        // enclosing class -> Extends -> parent member. Handled before the
        // scope lookup, which would surface the same-name override (the
        // self-edge class); misses fail closed instead of falling through
        // to a bare-name guess.
        if Self::is_super_instance_call(unresolved) {
            return self.resolve_super_call(from_id, unresolved, context, &caller);
        }

        // Self-aliased receivers (self/this/cls) name their container: the
        // caller's own enclosing type, evidenced by its ClassMember scope.
        // Resolve within that type's direct members before the scope
        // lookup, whose frozen winner may be a same-name member of a
        // sibling type. A miss falls through — implicit-this languages
        // emit the alias for free-function calls too.
        if self.is_self_form_instance_call(unresolved, &caller.language_id) {
            if let Some(resolved) = self.resolve_self_form_member(from_id, unresolved) {
                return Some(resolved);
            }
        }

        if let Some(to_id) = context.resolve(&unresolved.to_name) {
            if self.is_compatible(
                from_kind,
                to_id,
                unresolved.kind,
                caller.file_id,
                &caller.language_id,
            ) && self.is_receiver_compat(to_id, unresolved, &caller.language_id)
                && self.is_instance_type_compatible(unresolved, to_id, &caller.language_id, context)
            {
                return self.accept_unwitnessed_pick(from_id, to_id, unresolved);
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
                if !self.is_instance_type_compatible(
                    unresolved,
                    to_id,
                    &caller.language_id,
                    context,
                ) {
                    return None;
                }
                self.accept_unwitnessed_pick(from_id, to_id, unresolved)
            }
            ResolveResult::Ambiguous(candidates) => {
                let to_id = self.disambiguate(&candidates, unresolved, context, false)?;
                self.accept_unwitnessed_pick(from_id, to_id, unresolved)
            }
            ResolveResult::NotFound => {
                self.resolve_typed_receiver_global(from_id, from_kind, unresolved, &caller, context)
            }
        }
    }

    /// Type-directed member lookup for instance calls whose receiver type is
    /// inferred but whose bare name is not in scope — an inherited member
    /// called from a file that never imports it (`m = Model(...);
    /// m.model_dump()` in a test file). Evidence-gated: candidates come from
    /// the global name index, filtered to Method-kind symbols of the caller's
    /// language whose ClassMember class sits on the receiver type's chain;
    /// exactly one minimal-distance survivor resolves, anything else fails
    /// closed.
    fn resolve_typed_receiver_global(
        &self,
        from_id: SymbolId,
        from_kind: Option<crate::SymbolKind>,
        unresolved: &UnresolvedRelationship,
        caller: &CallerContext,
        context: &ResolutionContext,
    ) -> Option<ResolvedRelationship> {
        if unresolved.kind != RelationKind::Calls {
            return None;
        }
        let (type_name, caller_sym) =
            self.infer_receiver_type(unresolved, &caller.language_id, context)?;
        let mut best: Option<usize> = None;
        let mut scored: Vec<(SymbolId, usize)> = Vec::new();
        for id in self.symbol_cache.lookup_candidates(&unresolved.to_name) {
            let Some(sym) = self.symbol_cache.get_ref(id) else {
                continue;
            };
            if sym.kind != crate::SymbolKind::Method
                || sym.language_id.as_ref() != Some(&caller.language_id)
            {
                continue;
            }
            if let Some(distance) = self.receiver_chain_distance(
                &sym,
                &type_name,
                Some(&*caller_sym),
                &caller.language_id,
                context,
            ) {
                best = Some(best.map_or(distance, |b| b.min(distance)));
                scored.push((id, distance));
            }
        }
        let survivors: Vec<SymbolId> = scored
            .into_iter()
            .filter(|&(_, distance)| Some(distance) == best)
            .map(|(id, _)| id)
            .collect();
        let [to_id] = survivors[..] else {
            return None;
        };
        if !self.is_compatible(
            from_kind,
            to_id,
            unresolved.kind,
            caller.file_id,
            &caller.language_id,
        ) {
            return None;
        }
        self.accept_unwitnessed_pick(from_id, to_id, unresolved)
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
        context: &ResolutionContext,
    ) -> Option<(String, impl std::ops::Deref<Target = crate::Symbol> + 'a)> {
        let metadata = unresolved.metadata.as_ref()?;
        if metadata.static_call {
            return None;
        }
        let receiver = metadata.receiver.as_deref()?;
        let behavior = self.get_behavior(language_id)?;
        let caller = self.symbol_cache.get_ref(unresolved.from_id?)?;

        // Local bindings outrank the parameter annotation: they carry a
        // binding site, so the last binding before the call wins over a
        // position-less signature type (a rebinding shadows the parameter).
        // Self-aliased receivers never consult bindings — the self-form arm
        // and alias vocabulary own them; a rebound `cls` (metaclass idiom)
        // must not trade a resolvable alias for a method-result guess.
        if !behavior.self_receiver_aliases().contains(&receiver) {
            if let Some(type_name) =
                self.binding_type_at_call_site(unresolved, receiver, &caller, context)
            {
                return Some((type_name, caller));
            }
        }

        let signature = caller.signature.as_deref()?;
        let type_name = behavior.extract_parameter_type(signature, receiver)?;
        Some((type_name, caller))
    }

    /// Type of the last in-scope binding of `receiver` before the call site.
    /// A binding counts when it sits inside the caller's span, precedes the
    /// call, and is not enclosed by a narrower function-like symbol (a
    /// nested def's locals do not leak outward).
    fn binding_type_at_call_site(
        &self,
        unresolved: &UnresolvedRelationship,
        receiver: &str,
        caller: &crate::Symbol,
        context: &ResolutionContext,
    ) -> Option<String> {
        let call_site = unresolved.to_range.as_ref()?;
        let mut best: Option<&crate::indexing::pipeline::VariableBinding> = None;
        for binding in &context.variable_bindings {
            if binding.name != receiver
                || !range_contains(&caller.range, &binding.range)
                || !starts_before(&binding.range, call_site)
                || self.binding_in_narrower_scope(&binding.range, caller, context)
            {
                continue;
            }
            if best.is_none_or(|b| starts_before(&b.range, &binding.range)) {
                best = Some(binding);
            }
        }
        best.map(|b| b.type_name.clone())
    }

    /// A binding enclosed by a function-like symbol narrower than the caller
    /// belongs to that nested scope, not the caller's.
    fn binding_in_narrower_scope(
        &self,
        binding_range: &crate::Range,
        caller: &crate::Symbol,
        context: &ResolutionContext,
    ) -> bool {
        context.local_symbols.iter().any(|&id| {
            if id == caller.id {
                return false;
            }
            self.symbol_cache.get_ref(id).is_some_and(|sym| {
                matches!(
                    sym.kind,
                    crate::SymbolKind::Function | crate::SymbolKind::Method
                ) && range_contains(&caller.range, &sym.range)
                    && sym.range != caller.range
                    && range_contains(&sym.range, binding_range)
            })
        })
    }

    /// An instance call whose receiver is syntactically present but whose
    /// type is neither inferred nor indexed. Bare-name fallback fails closed
    /// here: with no type to check, any same-name candidate is a guess, and
    /// first-pick guesses attach std/foreign-receiver calls to arbitrary
    /// user methods. Self-form receivers stay resolvable: the enclosing
    /// type is known even without signature inference.
    fn has_uninferrable_instance_receiver(
        &self,
        unresolved: &UnresolvedRelationship,
        language_id: &LanguageId,
        context: &ResolutionContext,
    ) -> bool {
        if unresolved.kind != RelationKind::Calls {
            return false;
        }
        let Some(metadata) = unresolved.metadata.as_ref() else {
            return false;
        };
        if metadata.static_call {
            return false;
        }
        let Some(receiver) = metadata.receiver.as_deref() else {
            return false;
        };
        let self_aliases = self
            .get_behavior(language_id)
            .map(|b| b.self_receiver_aliases())
            .unwrap_or(&["self"]);
        if self_aliases.contains(&receiver) {
            return false;
        }
        self.infer_receiver_type(unresolved, language_id, context)
            .is_none()
    }

    /// Single-candidate gate (Found arm of `resolve_one`): when the inferred
    /// receiver type is known, the candidate is compatible iff its containing
    /// class matches via `is_receiver_compatible`. When inference misses,
    /// three-state: no receiver in play (plain call, static call, self form)
    /// passes through; a receiver whose type is unknown fails closed.
    fn is_instance_type_compatible(
        &self,
        unresolved: &UnresolvedRelationship,
        to_id: SymbolId,
        language_id: &LanguageId,
        context: &ResolutionContext,
    ) -> bool {
        let Some((type_name, caller)) = self.infer_receiver_type(unresolved, language_id, context)
        else {
            return !self.has_uninferrable_instance_receiver(unresolved, language_id, context);
        };
        if self.get_behavior(language_id).is_none() {
            return true;
        }
        let Some(candidate) = self.symbol_cache.get_ref(to_id) else {
            return true;
        };
        self.receiver_chain_distance(&candidate, &type_name, Some(&*caller), language_id, context)
            .is_some()
    }

    /// Ancestor-aware receiver compatibility: 0 = member of the receiver's
    /// own type, n = member of the n-nearest type on its inheritance chain
    /// (Extends evidence, MRO order for python), None = not on the chain.
    /// Inherited members resolve through the chain; the concrete type's own
    /// override always ranks closer.
    /// Identity anchor for an inferred receiver type: the specific Class
    /// symbol the type name denotes in THIS file — file-scope resolution
    /// (imports included) first, then the exactly-one function-local Class
    /// inside the caller's span. None when the name does not resolve to a
    /// Class here (unresolvable import specifier, non-Class kinds such as
    /// rust Structs): copy selection is then impossible and distance 0
    /// stays name-keyed.
    fn receiver_type_anchor(
        &self,
        type_name: &str,
        caller: Option<&crate::Symbol>,
        context: &ResolutionContext,
    ) -> Option<SymbolId> {
        context
            .resolve(type_name)
            .filter(|&id| {
                self.symbol_cache
                    .get_ref(id)
                    .is_some_and(|sym| sym.kind == crate::SymbolKind::Class)
            })
            .or_else(|| {
                // Function-local classes never reach the file scope; a Class
                // of this name inside the caller's own span is identity
                // evidence. Exactly one, else fail closed.
                let caller_sym = caller?;
                let mut only = None;
                for &id in &context.local_symbols {
                    let is_match = self.symbol_cache.get_ref(id).is_some_and(|sym| {
                        sym.kind == crate::SymbolKind::Class
                            && sym.name.as_ref() == type_name
                            && range_contains(&caller_sym.range, &sym.range)
                    });
                    if is_match {
                        if only.is_some() {
                            return None;
                        }
                        only = Some(id);
                    }
                }
                only
            })
    }

    fn receiver_chain_distance(
        &self,
        candidate: &crate::Symbol,
        type_name: &str,
        caller: Option<&crate::Symbol>,
        language_id: &LanguageId,
        context: &ResolutionContext,
    ) -> Option<usize> {
        let behavior = self.get_behavior(language_id)?;
        // Distance 0: with an identity anchor, membership is in the ANCHORED
        // class copy — same file, inside the anchor's span, direct member.
        // Name-keyed compatibility alone cannot split duplicate class copies
        // (vendored bundles, twin test files) or same-file same-name locals.
        // Without an anchor, name-keyed is the only evidence there is.
        let anchor = self.receiver_type_anchor(type_name, caller, context);
        match anchor {
            Some(anchor_id) => {
                let (anchor_name, anchor_file, anchor_range) = {
                    let sym = self.symbol_cache.get_ref(anchor_id)?;
                    (sym.name.clone(), sym.file_id, sym.range)
                };
                if candidate.file_id == anchor_file
                    && range_contains(&anchor_range, &candidate.range)
                    && self.is_direct_member(
                        candidate.scope_context.as_ref(),
                        candidate.range,
                        anchor_id,
                        &anchor_name,
                        anchor_file,
                    )
                {
                    return Some(0);
                }
            }
            None => {
                if behavior.is_receiver_compatible(candidate, type_name, caller) {
                    return Some(0);
                }
            }
        }
        // Ancestor hops are identity-grade: the receiver type must resolve
        // to a Class through THIS file's scope, and each hop follows that
        // class's own Extends edges with scope-resolved parents. Bare-name
        // chains merge every same-named class in the corpus (the pydantic
        // test `Model` population) and mis-attribute inherited members.
        // Chains dead-end after leaving the file (a parent's own Extends
        // edges live in its file's context): deep cross-file chains
        // under-report.
        let start = anchor?;
        let mut frontier = vec![start];
        for distance in 1..=8usize {
            let mut next = Vec::new();
            for class_id in frontier {
                for rel in &context.unresolved_rels {
                    if rel.kind != RelationKind::Extends || rel.from_id != Some(class_id) {
                        continue;
                    }
                    let Some(parent_id) = context.resolve(&rel.to_name) else {
                        continue;
                    };
                    let Some(parent) = self.symbol_cache.get_ref(parent_id) else {
                        continue;
                    };
                    if parent.kind != crate::SymbolKind::Class
                        || parent.language_id.as_ref() != Some(language_id)
                    {
                        continue;
                    }
                    let parent_name = parent.name.clone();
                    let parent_file = parent.file_id;
                    drop(parent);
                    if candidate.file_id == parent_file
                        && self.is_direct_member(
                            candidate.scope_context.as_ref(),
                            candidate.range,
                            parent_id,
                            &parent_name,
                            parent_file,
                        )
                    {
                        return Some(distance);
                    }
                    next.push(parent_id);
                }
            }
            if next.is_empty() {
                return None;
            }
            frontier = next;
        }
        None
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
        context: &ResolutionContext,
    ) -> Option<Vec<SymbolId>> {
        let (type_name, caller) = self.infer_receiver_type(unresolved, language_id, context)?;
        self.get_behavior(language_id)?;
        // Nearest-class-wins: the concrete type's own member (distance 0)
        // shadows inherited same-name members further up the chain.
        let mut best: Option<usize> = None;
        let mut scored: Vec<(SymbolId, usize)> = Vec::new();
        for &id in candidates {
            let Some(sym) = self.symbol_cache.get_ref(id) else {
                continue;
            };
            if let Some(distance) =
                self.receiver_chain_distance(&sym, &type_name, Some(&*caller), language_id, context)
            {
                best = Some(best.map_or(distance, |b| b.min(distance)));
                scored.push((id, distance));
            }
        }
        let matches: Vec<SymbolId> = scored
            .into_iter()
            .filter(|&(_, distance)| Some(distance) == best)
            .map(|(id, _)| id)
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
    /// An instance call whose receiver is the python `super()` form.
    fn is_super_instance_call(unresolved: &UnresolvedRelationship) -> bool {
        unresolved.kind == RelationKind::Calls
            && unresolved
                .metadata
                .as_ref()
                .is_some_and(|m| !m.static_call && m.receiver.as_deref() == Some("super()"))
    }

    /// Resolve `super().method()` through the parent chain: the caller's
    /// enclosing class, its Extends targets in base-list order, and the
    /// first parent declaring a same-name method as a DIRECT member
    /// (`is_direct_member` — not mere containment in the parent's span).
    /// Single hop — a parent that resolves but does not declare the
    /// member fails closed rather than walking a cross-file chain.
    fn resolve_super_call(
        &self,
        from_id: SymbolId,
        unresolved: &UnresolvedRelationship,
        context: &ResolutionContext,
        caller: &CallerContext,
    ) -> Option<ResolvedRelationship> {
        let caller_sym = self.symbol_cache.get_ref(from_id)?;
        let caller_range = caller_sym.range;
        let file_id = caller_sym.file_id;
        drop(caller_sym);

        // Innermost class in the caller's file whose range encloses the
        // calling method.
        let class_id = self
            .symbol_cache
            .symbols_in_file(file_id)
            .into_iter()
            .filter(|&id| {
                self.symbol_cache.get_ref(id).is_some_and(|sym| {
                    sym.kind == crate::SymbolKind::Class
                        && sym.range.start_line <= caller_range.start_line
                        && caller_range.end_line <= sym.range.end_line
                })
            })
            .max_by_key(|&id| {
                self.symbol_cache
                    .get_ref(id)
                    .map(|sym| sym.range.start_line)
                    .unwrap_or(0)
            })?;

        for rel in &context.unresolved_rels {
            if rel.kind != RelationKind::Extends || rel.from_id != Some(class_id) {
                continue;
            }
            // Same lookup the Extends edge itself resolves through: the
            // scope has the file's import bindings (e.g. `from .main
            // import BaseModel`). A scope miss skips the base — parents
            // reachable only via Tier 3 under-resolve, never mis-resolve.
            let Some(parent_id) = context.resolve(&rel.to_name) else {
                continue;
            };
            let Some(parent) = self.symbol_cache.get_ref(parent_id) else {
                continue;
            };
            if parent.kind != crate::SymbolKind::Class
                || parent.language_id.as_ref() != Some(&caller.language_id)
            {
                continue;
            }
            let (parent_file, parent_range) = (parent.file_id, parent.range);
            let parent_name = parent.name.clone();
            drop(parent);

            let member = self
                .symbol_cache
                .lookup_candidates(&unresolved.to_name)
                .into_iter()
                .find(|&id| {
                    let Some((member_range, member_scope)) =
                        self.symbol_cache.get_ref(id).and_then(|sym| {
                            (sym.file_id == parent_file
                                && sym.kind == crate::SymbolKind::Method
                                && parent_range.start_line <= sym.range.start_line
                                && sym.range.end_line <= parent_range.end_line)
                                .then(|| (sym.range, sym.scope_context.clone()))
                        })
                    else {
                        return false;
                    };
                    self.is_direct_member(
                        member_scope.as_ref(),
                        member_range,
                        parent_id,
                        &parent_name,
                        parent_file,
                    )
                });
            if let Some(to_id) = member {
                return Some(ResolvedRelationship {
                    from_id,
                    to_id,
                    kind: unresolved.kind,
                    metadata: unresolved.metadata.clone(),
                });
            }
        }
        None
    }

    /// Direct membership in the parent class, not mere line-range
    /// containment: a Method inside the parent's span may belong to a
    /// class nested in the parent's body. The parser's
    /// `ClassMember.class_name` decides when present; parser-declared
    /// non-member scopes reject; untracked scope falls back to the
    /// innermost-enclosing-class walk (the candidate's innermost
    /// enclosing Class must BE the parent, mirroring the caller side).
    fn is_direct_member(
        &self,
        member_scope: Option<&crate::symbol::ScopeContext>,
        member_range: crate::types::Range,
        parent_id: SymbolId,
        parent_name: &str,
        parent_file: FileId,
    ) -> bool {
        use crate::symbol::ScopeContext;
        match member_scope {
            Some(ScopeContext::ClassMember {
                class_name: Some(class_name),
            }) => return class_name.as_ref() == parent_name,
            Some(ScopeContext::ClassMember { class_name: None }) | None => {}
            Some(_) => return false,
        }
        let innermost = self
            .symbol_cache
            .symbols_in_file(parent_file)
            .into_iter()
            .filter(|&id| {
                self.symbol_cache.get_ref(id).is_some_and(|sym| {
                    sym.kind == crate::SymbolKind::Class
                        && sym.range.start_line <= member_range.start_line
                        && member_range.end_line <= sym.range.end_line
                })
            })
            .max_by_key(|&id| {
                self.symbol_cache
                    .get_ref(id)
                    .map(|sym| sym.range.start_line)
                    .unwrap_or(0)
            });
        innermost == Some(parent_id)
    }

    /// An instance call whose receiver is a self alias for the caller's
    /// language (`self`, `this`, `cls`, ...).
    fn is_self_form_instance_call(
        &self,
        unresolved: &UnresolvedRelationship,
        language_id: &LanguageId,
    ) -> bool {
        if unresolved.kind != RelationKind::Calls {
            return false;
        }
        let Some(metadata) = unresolved.metadata.as_ref() else {
            return false;
        };
        if metadata.static_call {
            return false;
        }
        let Some(receiver) = metadata.receiver.as_deref() else {
            return false;
        };
        self.get_behavior(language_id)
            .map(|b| b.self_receiver_aliases())
            .unwrap_or(&["self"])
            .contains(&receiver)
    }

    /// Resolve a self-form call within the caller's own type: the caller's
    /// `ClassMember { class_name: Some }` names the enclosing type, and a
    /// same-file candidate carrying the same named evidence is its direct
    /// member. Exactly one survivor resolves; zero or several return None
    /// and the caller falls through to the ordinary path.
    fn resolve_self_form_member(
        &self,
        from_id: SymbolId,
        unresolved: &UnresolvedRelationship,
    ) -> Option<ResolvedRelationship> {
        let caller_sym = self.symbol_cache.get_ref(from_id)?;
        let caller_file = caller_sym.file_id;
        let enclosing = match caller_sym.scope_context.as_ref() {
            Some(crate::symbol::ScopeContext::ClassMember {
                class_name: Some(name),
            }) => name.clone(),
            _ => return None,
        };
        drop(caller_sym);

        let mut members = self
            .symbol_cache
            .lookup_candidates(&unresolved.to_name)
            .into_iter()
            .filter(|&id| {
                self.symbol_cache.get_ref(id).is_some_and(|sym| {
                    sym.file_id == caller_file
                        && matches!(
                            sym.scope_context.as_ref(),
                            Some(crate::symbol::ScopeContext::ClassMember {
                                class_name: Some(name),
                            }) if *name == enclosing
                        )
                })
            });
        match (members.next(), members.next()) {
            (Some(to_id), None) => Some(ResolvedRelationship {
                from_id,
                to_id,
                kind: unresolved.kind,
                metadata: unresolved.metadata.clone(),
            }),
            _ => None,
        }
    }

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

    /// Final acceptance for a pick made without receiver evidence.
    ///
    /// Scope maps and the locality ladder pick last-declared/closest, not
    /// the right container: when the winner is one of several same-file
    /// member symbols with the same name, the pick is a guess. Calls fail
    /// closed (under-report, not mis-report); Defines fall back to the
    /// definer's own contained member before failing closed. Non-member
    /// collisions (plain-function shadowing) pass through untouched —
    /// closest-before-call IS the scope semantics there.
    fn accept_unwitnessed_pick(
        &self,
        from_id: SymbolId,
        to_id: SymbolId,
        unresolved: &UnresolvedRelationship,
    ) -> Option<ResolvedRelationship> {
        let to_id = if self.is_unwitnessed_member_pick(to_id, unresolved) {
            if unresolved.kind != RelationKind::Defines {
                return None;
            }
            self.defines_member_by_containment(from_id, unresolved)?
        } else {
            to_id
        };
        Some(ResolvedRelationship {
            from_id,
            to_id,
            kind: unresolved.kind,
            metadata: unresolved.metadata.clone(),
        })
    }

    fn is_unwitnessed_member_pick(
        &self,
        to_id: SymbolId,
        unresolved: &UnresolvedRelationship,
    ) -> bool {
        if !matches!(unresolved.kind, RelationKind::Calls | RelationKind::Defines) {
            return false;
        }
        if unresolved
            .metadata
            .as_ref()
            .is_some_and(|m| m.receiver.is_some())
        {
            return false;
        }
        let Some(winner) = self.symbol_cache.get_ref(to_id) else {
            return false;
        };
        if !Self::is_member_symbol(&winner) {
            return false;
        }
        let (winner_file, winner_name) = (winner.file_id, winner.name.clone());
        drop(winner);
        self.symbol_cache
            .lookup_candidates(&winner_name)
            .into_iter()
            .filter(|&id| id != to_id)
            .any(|id| {
                self.symbol_cache
                    .get_ref(id)
                    .is_some_and(|sym| sym.file_id == winner_file && Self::is_member_symbol(&sym))
            })
    }

    fn is_member_symbol(sym: &Symbol) -> bool {
        sym.kind == crate::SymbolKind::Method
            || matches!(
                sym.scope_context,
                Some(crate::symbol::ScopeContext::ClassMember { .. })
            )
    }

    /// Membership pick for gated Defines: parser evidence names the
    /// definer's own member without guessing. Evidence order:
    ///
    /// 1. Site identity — the edge's `to_range` is the member's
    ///    parse-recorded location; the same-file candidate starting
    ///    there IS the member. Disambiguates same-name members of one
    ///    definer (field + accessor) and same-name definers in one
    ///    file, where name evidence alone sees rival claimants.
    /// 2. `ClassMember { class_name: Some }` matching the definer —
    ///    direct membership regardless of span containment (rust
    ///    members live in impl blocks outside the struct's span).
    /// 3. Span containment plus `is_direct_member` for unnamed or
    ///    untracked scopes.
    ///
    /// Exactly one survivor resolves; zero or several fail closed.
    fn defines_member_by_containment(
        &self,
        from_id: SymbolId,
        unresolved: &UnresolvedRelationship,
    ) -> Option<SymbolId> {
        let definer = self.symbol_cache.get_ref(from_id)?;
        let (definer_file, definer_range) = (definer.file_id, definer.range);
        let definer_name = definer.name.clone();
        drop(definer);

        if let Some(to_range) = unresolved.to_range {
            let mut at_site = self
                .symbol_cache
                .lookup_candidates(&unresolved.to_name)
                .into_iter()
                .filter(|&id| {
                    self.symbol_cache.get_ref(id).is_some_and(|sym| {
                        sym.file_id == definer_file && sym.range.start_line == to_range.start_line
                    })
                });
            if let (Some(id), None) = (at_site.next(), at_site.next()) {
                return Some(id);
            }
        }

        let mut contained = self
            .symbol_cache
            .lookup_candidates(&unresolved.to_name)
            .into_iter()
            .filter(|&id| {
                let Some((member_range, member_scope)) =
                    self.symbol_cache.get_ref(id).and_then(|sym| {
                        (sym.file_id == definer_file)
                            .then(|| (sym.range, sym.scope_context.clone()))
                    })
                else {
                    return false;
                };
                if let Some(crate::symbol::ScopeContext::ClassMember {
                    class_name: Some(class_name),
                }) = member_scope.as_ref()
                {
                    return class_name.as_ref() == &*definer_name;
                }
                definer_range.start_line <= member_range.start_line
                    && member_range.end_line <= definer_range.end_line
                    && self.is_direct_member(
                        member_scope.as_ref(),
                        member_range,
                        from_id,
                        &definer_name,
                        definer_file,
                    )
            });
        let survivor = contained.next();
        match (survivor, contained.next()) {
            (Some(id), None) => Some(id),
            _ => None,
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
        // Instance-call disambiguation via inferred receiver type: filter
        // candidates to those whose containing type matches. Single survivor
        // wins; zero fail closed; multiple ALSO fail closed — the survivors
        // are class-correct but copy-ambiguous (duplicate class copies in a
        // corpus), and the ladder below is name-keyed first-pick, not
        // identity evidence. An unresolved edge beats a wrong-copy or
        // wrong-class guess.
        if unresolved.kind == RelationKind::Calls {
            if let Some(survivors) =
                self.filter_by_instance_receiver_type(&filtered, unresolved, language_id, context)
            {
                return match survivors.len() {
                    1 => Some(survivors[0]),
                    _ => None,
                };
            } else if self.has_uninferrable_instance_receiver(unresolved, language_id, context) {
                // Receiver present, type unknown: an unresolved edge beats
                // a first-pick guess from the priority ladder below.
                return None;
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

fn starts_before(a: &crate::Range, b: &crate::Range) -> bool {
    (a.start_line, a.start_column) < (b.start_line, b.start_column)
}

fn range_contains(outer: &crate::Range, inner: &crate::Range) -> bool {
    (outer.start_line, outer.start_column) <= (inner.start_line, inner.start_column)
        && (inner.end_line, inner.end_column) <= (outer.end_line, outer.end_column)
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
            variable_bindings: vec![],
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

    fn make_instance_call(
        from_id: u32,
        to_name: &str,
        file_id: u32,
        receiver: &str,
    ) -> UnresolvedRelationship {
        let mut unresolved = make_unresolved(from_id, to_name, file_id, RelationKind::Calls);
        unresolved.metadata = Some(crate::relationship::RelationshipMetadata {
            line: Some(5),
            column: Some(4),
            context: None,
            receiver: Some(receiver.into()),
            static_call: false,
        });
        unresolved
    }

    /// Scope resolving a fixed name map — stands in for import bindings.
    struct MapScope(std::collections::HashMap<String, SymbolId>);

    impl ResolutionScope for MapScope {
        fn add_symbol(&mut self, _name: String, _symbol_id: SymbolId, _scope_level: ScopeLevel) {}
        fn resolve(&self, name: &str) -> Option<SymbolId> {
            self.0.get(name).copied()
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

    fn make_member(
        id: u32,
        name: &str,
        file_id: u32,
        lang: LanguageId,
        start: u32,
        end: u32,
    ) -> Symbol {
        let mut sym = make_symbol(id, name, file_id, lang);
        sym.kind = SymbolKind::Method;
        sym.range = Range::new(start, 0, end, 1);
        sym
    }

    /// Caller + three same-file same-name methods (the lua
    /// `createObject` shape: Config:new / Container:new / Vector:new).
    fn member_collision_cache() -> Arc<SymbolLookupCache> {
        let lua = LanguageId::new("lua");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut caller = make_symbol(1, "createObject", 1, lua);
        caller.range = Range::new(300, 0, 316, 1);
        cache.insert(caller);
        cache.insert(make_member(2, "new", 1, lua, 100, 110));
        cache.insert(make_member(3, "new", 1, lua, 150, 160));
        cache.insert(make_member(4, "new", 1, lua, 252, 260));
        cache
    }

    #[test]
    fn metadataless_call_frozen_scope_winner_fails_closed() {
        // The scope map hands back the last-declared same-name member; with
        // no receiver evidence and two rivals in the winner's file, the pick
        // is a guess and must fail closed.
        let lua = LanguageId::new("lua");
        let cache = member_collision_cache();
        let stage = make_stage(cache);

        let mut rel = make_unresolved(1, "new", 1, RelationKind::Calls);
        rel.to_range = Some(Range::new(308, 8, 308, 20));
        let mut context = make_context(1, lua, vec![SymbolId::new(1).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("new".to_string(), SymbolId::new(4).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 0, "member collision must not resolve");
        assert!(batch.is_empty());
    }

    #[test]
    fn metadataless_call_member_collision_ladder_fails_closed() {
        // Same collision through the cache tiers: Ambiguous -> disambiguate
        // must not fall back to closest-by-range among member candidates.
        let lua = LanguageId::new("lua");
        let cache = member_collision_cache();
        let stage = make_stage(cache);

        let mut rel = make_unresolved(1, "new", 1, RelationKind::Calls);
        rel.to_range = Some(Range::new(308, 8, 308, 20));
        let context = make_context(1, lua, vec![SymbolId::new(1).unwrap()], vec![rel]);

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 0, "ladder must not re-guess a gated pick");
        assert!(batch.is_empty());
    }

    #[test]
    fn plain_function_shadowing_still_resolves() {
        // Two same-file same-name FUNCTIONS are genuine shadowing;
        // closest-before-call is the scope semantics, not a guess.
        let lua = LanguageId::new("lua");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, lua));
        let mut early = make_symbol(2, "helper", 1, lua);
        early.range = Range::new(1, 0, 2, 1);
        cache.insert(early);
        let mut late = make_symbol(3, "helper", 1, lua);
        late.range = Range::new(3, 0, 4, 1);
        cache.insert(late);
        let stage = make_stage(cache);

        let rel = make_unresolved(1, "helper", 1, RelationKind::Calls);
        let context = make_context(1, lua, vec![SymbolId::new(1).unwrap()], vec![rel]);

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "function shadowing must keep resolving");
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(3).unwrap());
    }

    #[test]
    fn single_member_candidate_still_resolves() {
        let lua = LanguageId::new("lua");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, lua));
        cache.insert(make_member(2, "init", 1, lua, 100, 110));
        let stage = make_stage(cache);

        let rel = make_unresolved(1, "init", 1, RelationKind::Calls);
        let mut context = make_context(1, lua, vec![SymbolId::new(1).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("init".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "unambiguous member must keep resolving");
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn defines_collision_resolves_to_definer_own_member() {
        // Base(5..25) __init__@10..20; Derived(35..55) __init__@40..50.
        // Defines from Derived through a scope map frozen on Base's member
        // must land on Derived's own member (containment + direct
        // membership), not the frozen winner.
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut base = make_symbol(5, "Base", 1, python);
        base.kind = SymbolKind::Class;
        base.range = Range::new(5, 0, 25, 1);
        cache.insert(base);
        cache.insert(make_member(2, "__init__", 1, python, 10, 20));
        let mut derived = make_symbol(3, "Derived", 1, python);
        derived.kind = SymbolKind::Class;
        derived.range = Range::new(35, 0, 55, 1);
        cache.insert(derived);
        cache.insert(make_member(4, "__init__", 1, python, 40, 50));
        let stage = make_stage(cache);

        let mut rel = make_unresolved(3, "__init__", 1, RelationKind::Defines);
        rel.to_range = Some(Range::new(40, 0, 50, 1));
        let mut context = make_context(1, python, vec![SymbolId::new(3).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("__init__".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "defines must resolve via containment");
        assert_eq!(
            batch.relationships[0].to_id,
            SymbolId::new(4).unwrap(),
            "must be the definer's own member, not the frozen winner"
        );
    }

    #[test]
    fn defines_collision_without_contained_member_fails_closed() {
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_member(2, "__init__", 1, python, 10, 20));
        cache.insert(make_member(4, "__init__", 1, python, 40, 50));
        let mut definer = make_symbol(3, "Empty", 1, python);
        definer.kind = SymbolKind::Class;
        definer.range = Range::new(60, 0, 70, 1);
        cache.insert(definer);
        let stage = make_stage(cache);

        let mut rel = make_unresolved(3, "__init__", 1, RelationKind::Defines);
        rel.to_range = Some(Range::new(60, 0, 70, 1));
        let mut context = make_context(1, python, vec![SymbolId::new(3).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("__init__".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 0, "no contained member: fail closed");
        assert!(batch.is_empty());
    }

    /// The rust witness shape: struct spans hold only the declaration;
    /// members live in impl blocks OUTSIDE the span, carrying
    /// `ClassMember { class_name: Some }` as the membership evidence.
    fn rust_impl_member_cache() -> Arc<SymbolLookupCache> {
        let rust = LanguageId::new("rust");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut method_call = make_symbol(1, "MethodCall", 1, rust);
        method_call.kind = SymbolKind::Struct;
        method_call.range = Range::new(62, 0, 70, 1);
        cache.insert(method_call);
        let mut resolver = make_symbol(3, "MethodCallResolver", 1, rust);
        resolver.kind = SymbolKind::Struct;
        resolver.range = Range::new(205, 0, 210, 1);
        cache.insert(resolver);
        let mut new_a = make_member(2, "new", 1, rust, 100, 110);
        new_a.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("MethodCall".into()),
        });
        cache.insert(new_a);
        let mut new_b = make_member(4, "new", 1, rust, 214, 220);
        new_b.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("MethodCallResolver".into()),
        });
        cache.insert(new_b);
        cache
    }

    #[test]
    fn rust_defines_rescue_reaches_impl_member_outside_span() {
        // Defines from MethodCallResolver, scope map frozen on the OTHER
        // struct's `new`. The definer's own member sits outside its span
        // but carries named ClassMember evidence — rescue must re-target
        // onto it instead of dropping.
        let rust = LanguageId::new("rust");
        let cache = rust_impl_member_cache();
        let stage = make_stage(cache);

        let mut rel = make_unresolved(3, "new", 1, RelationKind::Defines);
        rel.to_range = Some(Range::new(214, 0, 220, 1));
        let mut context = make_context(1, rust, vec![SymbolId::new(3).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("new".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(
            stats.resolved, 1,
            "named ClassMember evidence must rescue the gated Defines"
        );
        assert_eq!(
            batch.relationships[0].to_id,
            SymbolId::new(4).unwrap(),
            "must be the definer's own impl member"
        );
    }

    #[test]
    fn rust_defines_witness_pair_each_struct_names_own_member() {
        // Both structs' Defines in one pass, both frozen on the same
        // winner: each must land its OWN impl member, never a cross-pick.
        let rust = LanguageId::new("rust");
        let cache = rust_impl_member_cache();
        let stage = make_stage(cache);

        let mut rel_a = make_unresolved(1, "new", 1, RelationKind::Defines);
        rel_a.to_range = Some(Range::new(100, 0, 110, 1));
        let mut rel_b = make_unresolved(3, "new", 1, RelationKind::Defines);
        rel_b.to_range = Some(Range::new(214, 0, 220, 1));
        let mut context = make_context(
            1,
            rust,
            vec![SymbolId::new(1).unwrap(), SymbolId::new(3).unwrap()],
            vec![rel_a, rel_b],
        );
        let mut map = std::collections::HashMap::new();
        map.insert("new".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 2, "both gated Defines must rescue");
        let pairs: Vec<(u32, u32)> = batch
            .relationships
            .iter()
            .map(|r| (r.from_id.value(), r.to_id.value()))
            .collect();
        assert!(pairs.contains(&(1, 2)), "MethodCall defines its own new");
        assert!(
            pairs.contains(&(3, 4)),
            "MethodCallResolver defines its own new"
        );
    }

    #[test]
    fn self_form_call_resolves_within_enclosing_type() {
        // Shape::display calls self-form `area()`; three same-file `area`
        // members (Shape/Circle/Rectangle), scope frozen on Rectangle's.
        // The caller's own ClassMember evidence names Shape — the call
        // must land on Shape's member, not the frozen winner.
        let lang = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut display = make_member(1, "display", 1, lang, 57, 60);
        display.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("Shape".into()),
        });
        cache.insert(display);
        for (id, class, line) in [(2, "Shape", 46), (3, "Circle", 83), (4, "Rectangle", 111)] {
            let mut m = make_member(id, "area", 1, lang, line, line + 2);
            m.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
                class_name: Some(class.into()),
            });
            cache.insert(m);
        }
        let stage = make_stage(cache);

        let mut rel = make_instance_call(1, "area", 1, "self");
        rel.to_range = Some(Range::new(58, 8, 58, 20));
        let mut context = make_context(1, lang, vec![SymbolId::new(1).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("area".to_string(), SymbolId::new(4).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "self-form member call must resolve");
        assert_eq!(
            batch.relationships[0].to_id,
            SymbolId::new(2).unwrap(),
            "must be the enclosing type's own member, not the frozen winner"
        );
    }

    #[test]
    fn self_form_miss_falls_through_to_existing_path() {
        // Enclosing type has no such member: the self-form arm must not
        // fail the call closed — implicit-this languages emit the alias
        // for free-function calls too, and the plain path still applies.
        let lang = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut caller = make_member(1, "display", 1, lang, 57, 60);
        caller.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("Shape".into()),
        });
        cache.insert(caller);
        cache.insert(make_symbol(2, "helper", 1, lang));
        let stage = make_stage(cache);

        let mut rel = make_instance_call(1, "helper", 1, "self");
        rel.to_range = Some(Range::new(58, 8, 58, 20));
        let mut context = make_context(1, lang, vec![SymbolId::new(1).unwrap()], vec![rel]);
        let mut map = std::collections::HashMap::new();
        map.insert("helper".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "arm miss keeps the existing path");
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn rust_defines_field_and_accessor_both_resolve_by_site() {
        // One struct owns BOTH a field `state` (inside its span) and an
        // accessor method `state` (in the impl block, outside). Two true
        // Defines edges; each carries the member's parse-recorded
        // to_range. Both must resolve to their own site — name evidence
        // alone sees two claimants and would fail closed.
        let rust = LanguageId::new("rust");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut owner = make_symbol(1, "CBehavior", 1, rust);
        owner.kind = SymbolKind::Struct;
        owner.range = Range::new(12, 0, 20, 1);
        cache.insert(owner);
        let mut field = make_member(2, "state", 1, rust, 14, 14);
        field.kind = SymbolKind::Field;
        field.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("CBehavior".into()),
        });
        cache.insert(field);
        let mut accessor = make_member(3, "state", 1, rust, 100, 105);
        accessor.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("CBehavior".into()),
        });
        cache.insert(accessor);
        let stage = make_stage(cache);

        let mut field_rel = make_unresolved(1, "state", 1, RelationKind::Defines);
        field_rel.to_range = Some(Range::new(14, 4, 14, 20));
        let mut accessor_rel = make_unresolved(1, "state", 1, RelationKind::Defines);
        accessor_rel.to_range = Some(Range::new(100, 4, 105, 5));
        let mut context = make_context(
            1,
            rust,
            vec![SymbolId::new(1).unwrap()],
            vec![field_rel, accessor_rel],
        );
        let mut map = std::collections::HashMap::new();
        map.insert("state".to_string(), SymbolId::new(2).unwrap());
        context.scope = Box::new(MapScope(map));

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 2, "both member edges must resolve by site");
        let pairs: Vec<(u32, u32)> = batch
            .relationships
            .iter()
            .map(|r| (r.from_id.value(), r.to_id.value()))
            .collect();
        assert!(pairs.contains(&(1, 2)), "field edge lands on the field");
        assert!(
            pairs.contains(&(1, 3)),
            "accessor edge lands on the accessor"
        );
    }

    /// class A: def m (5..25, m at 10..20); class B(A): def m (35..55,
    /// m at 40..50); B.m calls super().m().
    fn super_call_fixture() -> (Arc<SymbolLookupCache>, Vec<UnresolvedRelationship>) {
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        let mk = |id: u32, name: &str, kind: SymbolKind, start: u32, end: u32| {
            let mut sym = make_symbol(id, name, 1, python);
            sym.kind = kind;
            sym.range = Range::new(start, 0, end, 1);
            sym
        };
        cache.insert(mk(1, "A", SymbolKind::Class, 5, 25));
        cache.insert(mk(2, "m", SymbolKind::Method, 10, 20));
        cache.insert(mk(3, "B", SymbolKind::Class, 35, 55));
        cache.insert(mk(4, "m", SymbolKind::Method, 40, 50));

        let extends = make_unresolved(3, "A", 1, RelationKind::Extends);
        let mut super_call = make_instance_call(4, "m", 1, "super()");
        super_call.to_range = Some(Range::new(45, 8, 45, 20));
        (cache, vec![extends, super_call])
    }

    #[test]
    fn super_call_resolves_to_parent_method() {
        let python = LanguageId::new("python");
        let (cache, rels) = super_call_fixture();
        let stage = make_stage(Arc::clone(&cache));
        let mut context = make_context(1, python, vec![], rels);
        context.scope = Box::new(MapScope(
            [("A".to_string(), SymbolId::new(1).unwrap())]
                .into_iter()
                .collect(),
        ));

        let (batch, _) = stage.resolve(&context);
        let call = batch
            .relationships
            .iter()
            .find(|r| r.kind == RelationKind::Calls)
            .expect("super() call must resolve when the parent chain is known");
        assert_eq!(
            call.to_id,
            SymbolId::new(2).unwrap(),
            "target is A.m, not the B.m self-edge"
        );
    }

    /// class A (5..25) contains class Inner (7..12) with its own m
    /// (8..11) declared BEFORE A's m (15..22). Identity order puts
    /// Inner.m first; the edge must target A.m regardless. Symbols
    /// carry no scope_context, so this exercises the
    /// innermost-enclosing-class fallback arm.
    #[test]
    fn super_call_skips_nested_class_member() {
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        let mk = |id: u32, name: &str, kind: SymbolKind, start: u32, end: u32| {
            let mut sym = make_symbol(id, name, 1, python);
            sym.kind = kind;
            sym.range = Range::new(start, 0, end, 1);
            sym
        };
        cache.insert(mk(1, "A", SymbolKind::Class, 5, 25));
        cache.insert(mk(2, "Inner", SymbolKind::Class, 7, 12));
        cache.insert(mk(3, "m", SymbolKind::Method, 8, 11));
        cache.insert(mk(4, "m", SymbolKind::Method, 15, 22));
        cache.insert(mk(5, "B", SymbolKind::Class, 35, 55));
        cache.insert(mk(6, "m", SymbolKind::Method, 40, 50));

        let extends = make_unresolved(5, "A", 1, RelationKind::Extends);
        let mut super_call = make_instance_call(6, "m", 1, "super()");
        super_call.to_range = Some(Range::new(45, 8, 45, 20));

        let stage = make_stage(Arc::clone(&cache));
        let mut context = make_context(1, python, vec![], vec![extends, super_call]);
        context.scope = Box::new(MapScope(
            [("A".to_string(), SymbolId::new(1).unwrap())]
                .into_iter()
                .collect(),
        ));

        let (batch, _) = stage.resolve(&context);
        let call = batch
            .relationships
            .iter()
            .find(|r| r.kind == RelationKind::Calls)
            .expect("super() call must resolve to the parent's direct member");
        assert_eq!(
            call.to_id,
            SymbolId::new(4).unwrap(),
            "target is A.m, not Inner.m (first in identity order)"
        );
    }

    /// Same shape with parser-populated ClassMember scope: the
    /// class_name arm decides without the geometric walk.
    #[test]
    fn super_call_member_check_uses_class_member_scope() {
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        let mk =
            |id: u32, name: &str, kind: SymbolKind, start: u32, end: u32, class: Option<&str>| {
                let mut sym = make_symbol(id, name, 1, python);
                sym.kind = kind;
                sym.range = Range::new(start, 0, end, 1);
                if let Some(class) = class {
                    sym.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
                        class_name: Some(class.into()),
                    });
                }
                sym
            };
        cache.insert(mk(1, "A", SymbolKind::Class, 5, 25, None));
        cache.insert(mk(2, "Inner", SymbolKind::Class, 7, 12, None));
        cache.insert(mk(3, "m", SymbolKind::Method, 8, 11, Some("Inner")));
        cache.insert(mk(4, "m", SymbolKind::Method, 15, 22, Some("A")));
        cache.insert(mk(5, "B", SymbolKind::Class, 35, 55, None));
        cache.insert(mk(6, "m", SymbolKind::Method, 40, 50, Some("B")));

        let extends = make_unresolved(5, "A", 1, RelationKind::Extends);
        let mut super_call = make_instance_call(6, "m", 1, "super()");
        super_call.to_range = Some(Range::new(45, 8, 45, 20));

        let stage = make_stage(Arc::clone(&cache));
        let mut context = make_context(1, python, vec![], vec![extends, super_call]);
        context.scope = Box::new(MapScope(
            [("A".to_string(), SymbolId::new(1).unwrap())]
                .into_iter()
                .collect(),
        ));

        let (batch, _) = stage.resolve(&context);
        let call = batch
            .relationships
            .iter()
            .find(|r| r.kind == RelationKind::Calls)
            .expect("super() call must resolve via ClassMember.class_name");
        assert_eq!(
            call.to_id,
            SymbolId::new(4).unwrap(),
            "class_name == parent selects A.m; Inner.m rejected by name"
        );
    }

    #[test]
    fn super_call_with_unresolvable_parent_fails_closed() {
        let python = LanguageId::new("python");
        let (cache, rels) = super_call_fixture();
        let stage = make_stage(Arc::clone(&cache));
        // NoOpScope: the parent name resolves to nothing.
        let context = make_context(1, python, vec![], rels);

        let (batch, _) = stage.resolve(&context);
        assert!(
            !batch
                .relationships
                .iter()
                .any(|r| r.kind == RelationKind::Calls),
            "unresolvable parent chain produces no edge, not a guess"
        );
    }

    #[test]
    fn unknown_receiver_instance_call_fails_closed_cross_file() {
        // some_vec.len() where some_vec's type is neither inferred nor
        // indexed must not attach to a same-name user method elsewhere.
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        let mut method = make_symbol(2, "len", 2, LanguageId::new("rust"));
        method.kind = SymbolKind::Method;
        cache.insert(method);

        let stage = make_stage(cache);
        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap()],
            vec![make_instance_call(1, "len", 1, "some_vec")],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 0, "unknown receiver must fail closed");
        assert!(batch.is_empty());
    }

    #[test]
    fn unknown_receiver_instance_call_fails_closed_same_file() {
        // The local-first ladder is the strongest absorber: a same-file
        // same-name method must not win on file locality alone when the
        // receiver's type is unknown.
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        let mut local = make_symbol(2, "get", 1, LanguageId::new("rust"));
        local.kind = SymbolKind::Method;
        cache.insert(local);
        let mut remote = make_symbol(3, "get", 2, LanguageId::new("rust"));
        remote.kind = SymbolKind::Method;
        cache.insert(remote);

        let stage = make_stage(cache);
        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap(), SymbolId::new(2).unwrap()],
            vec![make_instance_call(1, "get", 1, "settings_map")],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 0, "same-file candidate must not absorb");
        assert!(batch.is_empty());
    }

    #[test]
    fn self_receiver_instance_call_still_resolves() {
        // self.helper(): the enclosing type is known without signature
        // inference; the self form must keep resolving through locality.
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "caller", 1, LanguageId::new("rust")));
        let mut method = make_symbol(2, "helper", 1, LanguageId::new("rust"));
        method.kind = SymbolKind::Method;
        cache.insert(method);

        let stage = make_stage(cache);
        let context = make_context(
            1,
            LanguageId::new("rust"),
            vec![SymbolId::new(1).unwrap(), SymbolId::new(2).unwrap()],
            vec![make_instance_call(1, "helper", 1, "self")],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "self receiver must keep resolving");
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn inferred_receiver_type_still_resolves_cross_file() {
        // fn process(w: Wrapper) { w.len() }: signature inference names the
        // receiver type, so the call resolves to Wrapper::len cross-file.
        let rust = LanguageId::new("rust");
        let cache = Arc::new(SymbolLookupCache::new());
        let mut caller = make_symbol(1, "process", 1, rust);
        caller.signature = Some("fn process(w: Wrapper)".into());
        cache.insert(caller);
        let mut method = make_symbol(2, "len", 2, rust);
        method.kind = SymbolKind::Method;
        method.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("Wrapper".into()),
        });
        cache.insert(method);

        let behaviors: HashMap<LanguageId, StdArc<dyn LanguageBehavior>> = HashMap::from([(
            rust,
            StdArc::new(crate::parsing::rust::RustBehavior::new()) as StdArc<dyn LanguageBehavior>,
        )]);
        let stage = ResolveStage::new(cache, behaviors);
        let context = make_context(
            1,
            rust,
            vec![SymbolId::new(1).unwrap()],
            vec![make_instance_call(1, "len", 1, "w")],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "inferred receiver type must resolve");
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn resolution_pick_is_insertion_order_independent() {
        // Candidate vectors fill in parse-completion order (thread
        // interleaving); the pick among equal-tier candidates must not
        // depend on it.
        let rust = LanguageId::new("rust");
        let build = |ids: &[u32]| {
            let cache = Arc::new(SymbolLookupCache::new());
            cache.insert(make_symbol(1, "caller", 1, rust));
            for &id in ids {
                // id N lives in file N (files 2 and 3, both cross-file)
                cache.insert(make_symbol(id, "format", id, rust));
            }
            let stage = make_stage(cache);
            let context = make_context(
                1,
                rust,
                vec![SymbolId::new(1).unwrap()],
                vec![make_unresolved(1, "format", 1, RelationKind::Calls)],
            );
            let (batch, _) = stage.resolve(&context);
            batch.relationships.first().map(|r| r.to_id)
        };

        assert_eq!(
            build(&[2, 3]),
            build(&[3, 2]),
            "pick must not depend on candidate insertion order"
        );
    }

    #[test]
    fn import_path_first_match_is_insertion_order_independent() {
        // Tier 2 scans candidates for a bidirectional module-path match;
        // two symbols can both match one import path. The winner must not
        // depend on insertion order.
        let rust = LanguageId::new("rust");
        let build = |order: &[(u32, &str)]| {
            let cache = Arc::new(SymbolLookupCache::new());
            cache.insert(make_symbol(1, "caller", 1, rust));
            for &(id, module_path) in order {
                let mut sym = make_symbol(id, "helper", id, rust);
                sym.module_path = Some(module_path.into());
                sym.file_path = format!("src/f{id}.rs").into();
                cache.insert(sym);
            }
            let stage = make_stage(cache);
            let mut context = make_context(
                1,
                rust,
                vec![SymbolId::new(1).unwrap()],
                vec![make_unresolved(1, "helper", 1, RelationKind::Calls)],
            );
            context.imports = vec![Import {
                path: "app::util::helper".to_string(),
                alias: None,
                file_id: FileId::new(1).unwrap(),
                is_glob: false,
                is_type_only: false,
            }];
            let (batch, _) = stage.resolve(&context);
            batch.relationships.first().map(|r| r.to_id)
        };

        let a = build(&[(2, "app::util"), (3, "util")]);
        let b = build(&[(3, "util"), (2, "app::util")]);
        assert!(a.is_some(), "import-path match must resolve");
        assert_eq!(
            a, b,
            "import-path winner must not depend on insertion order"
        );
    }

    #[test]
    fn resolution_pick_is_id_assignment_independent() {
        // Ids are session-scoped, assigned in collect-arrival order — the
        // same tree hands the same symbol different ids across runs. The
        // pick must land on the same symbol IDENTITY either way.
        let rust = LanguageId::new("rust");
        let winner_path = |id_for_alpha: u32, id_for_beta: u32| {
            let cache = Arc::new(SymbolLookupCache::new());
            cache.insert(make_symbol(1, "caller", 1, rust));
            for (id, path) in [(id_for_alpha, "src/alpha.rs"), (id_for_beta, "src/beta.rs")] {
                let mut sym = make_symbol(id, "format", id, rust);
                sym.file_path = path.into();
                cache.insert(sym);
            }
            let stage = make_stage(Arc::clone(&cache));
            let context = make_context(
                1,
                rust,
                vec![SymbolId::new(1).unwrap()],
                vec![make_unresolved(1, "format", 1, RelationKind::Calls)],
            );
            let (batch, _) = stage.resolve(&context);
            let to_id = batch.relationships.first().map(|r| r.to_id)?;
            cache.get(to_id).map(|s| s.file_path)
        };

        assert_eq!(
            winner_path(2, 3),
            winner_path(3, 2),
            "pick must land on the same symbol identity regardless of id assignment"
        );
    }

    #[test]
    fn python_self_aliases_pass_untyped_locals_fail_closed() {
        // cls.get() resolves through the python self vocabulary;
        // prefix_settings.get() (untyped local, the story-15 phantom class)
        // does not.
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "run_example", 1, python));
        let mut method = make_symbol(2, "get", 1, python);
        method.kind = SymbolKind::Method;
        cache.insert(method);

        let behaviors: HashMap<LanguageId, StdArc<dyn LanguageBehavior>> = HashMap::from([(
            python,
            StdArc::new(crate::parsing::python::PythonBehavior::new())
                as StdArc<dyn LanguageBehavior>,
        )]);
        let stage = ResolveStage::new(cache, behaviors);
        let context = make_context(
            1,
            python,
            vec![SymbolId::new(1).unwrap(), SymbolId::new(2).unwrap()],
            vec![
                make_instance_call(1, "get", 1, "cls"),
                make_instance_call(1, "get", 1, "prefix_settings"),
            ],
        );

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "cls resolves, untyped local does not");
        assert_eq!(batch.relationships[0].to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn constructor_bound_local_resolves_via_binding_last_wins() {
        // m = Other(...); m = Model(...); m.dump() — the last binding before
        // the call names the type; the same-name method on Other loses.
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "use_model", 1, python));
        for (id, class) in [(2u32, "Model"), (3u32, "Other")] {
            let mut method = make_symbol(id, "dump", 1, python);
            method.kind = SymbolKind::Method;
            method.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
                class_name: Some(class.into()),
            });
            cache.insert(method);
        }

        let behaviors: HashMap<LanguageId, StdArc<dyn LanguageBehavior>> = HashMap::from([(
            python,
            StdArc::new(crate::parsing::python::PythonBehavior::new())
                as StdArc<dyn LanguageBehavior>,
        )]);
        let stage = ResolveStage::new(cache, behaviors);
        let mut context = make_context(
            1,
            python,
            vec![
                SymbolId::new(1).unwrap(),
                SymbolId::new(2).unwrap(),
                SymbolId::new(3).unwrap(),
            ],
            vec![make_instance_call(1, "dump", 1, "m")],
        );
        context.variable_bindings = vec![
            crate::indexing::pipeline::VariableBinding {
                name: "m".into(),
                type_name: "Other".into(),
                range: Range::new(2, 4, 2, 20),
            },
            crate::indexing::pipeline::VariableBinding {
                name: "m".into(),
                type_name: "Model".into(),
                range: Range::new(4, 4, 4, 20),
            },
        ];

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 1, "constructor-typed local resolves");
        assert_eq!(
            batch.relationships[0].to_id,
            SymbolId::new(2).unwrap(),
            "last binding before the call wins"
        );
    }

    fn make_class(id: u32, name: &str, file_id: u32, lang: LanguageId) -> Symbol {
        let mut sym = make_symbol(id, name, file_id, lang);
        sym.kind = SymbolKind::Class;
        sym
    }

    fn make_extends(
        from_id: u32,
        from_name: &str,
        to_name: &str,
        file_id: u32,
    ) -> UnresolvedRelationship {
        UnresolvedRelationship {
            from_id: Some(SymbolId::new(from_id).unwrap()),
            from_name: StdArc::from(from_name),
            to_name: StdArc::from(to_name),
            file_id: FileId::new(file_id).unwrap(),
            kind: RelationKind::Extends,
            metadata: None,
            to_range: None,
        }
    }

    fn python_stage_with_class_scope(
        cache: Arc<SymbolLookupCache>,
    ) -> (ResolveStage, Box<MapScope>) {
        let python = LanguageId::new("python");
        let behaviors: HashMap<LanguageId, StdArc<dyn LanguageBehavior>> = HashMap::from([(
            python,
            StdArc::new(crate::parsing::python::PythonBehavior::new())
                as StdArc<dyn LanguageBehavior>,
        )]);
        let scope = Box::new(MapScope(std::collections::HashMap::from([
            ("Model".to_string(), SymbolId::new(10).unwrap()),
            ("Base".to_string(), SymbolId::new(11).unwrap()),
        ])));
        (ResolveStage::new(cache, behaviors), scope)
    }

    fn model_binding() -> crate::indexing::pipeline::VariableBinding {
        crate::indexing::pipeline::VariableBinding {
            name: "m".into(),
            type_name: "Model".into(),
            range: Range::new(3, 4, 3, 20),
        }
    }

    #[test]
    fn inherited_member_resolves_through_extends_chain() {
        // m = Model(); m.dump() where only Base defines dump and Model
        // extends Base: the identity chain (scope-resolved Model -> its
        // Extends edge -> scope-resolved Base) names Base.dump; the
        // same-name member on an unrelated class stays excluded.
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "use_model", 1, python));
        for (id, class) in [(2u32, "Base"), (3u32, "Rogue")] {
            let mut method = make_symbol(id, "dump", 1, python);
            method.kind = SymbolKind::Method;
            method.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
                class_name: Some(class.into()),
            });
            cache.insert(method);
        }
        cache.insert(make_class(10, "Model", 1, python));
        cache.insert(make_class(11, "Base", 1, python));

        let (stage, scope) = python_stage_with_class_scope(Arc::clone(&cache));
        let context = ResolutionContext {
            file_id: FileId::new(1).unwrap(),
            language_id: python,
            imports: vec![],
            local_symbols: vec![
                SymbolId::new(1).unwrap(),
                SymbolId::new(2).unwrap(),
                SymbolId::new(3).unwrap(),
            ],
            scope,
            unresolved_rels: vec![
                make_instance_call(1, "dump", 1, "m"),
                make_extends(10, "Model", "Base", 1),
            ],
            variable_bindings: vec![model_binding()],
        };

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(
            stats.calls_resolved, 1,
            "inherited member resolves via chain"
        );
        let call = batch
            .relationships
            .iter()
            .find(|r| r.kind == RelationKind::Calls)
            .expect("calls edge present");
        assert_eq!(
            call.to_id,
            SymbolId::new(2).unwrap(),
            "Base.dump wins over the unrelated same-name member"
        );
    }

    #[test]
    fn inherited_member_resolves_cross_file_without_import() {
        // m = Model(); m.model_dump() in a file that never imports the
        // member: the typed-receiver global lookup walks the identity chain
        // and finds the parent-file member; the same-name method on an
        // off-chain class stays excluded.
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "test_repr", 1, python));
        let mut base_dump = make_symbol(2, "model_dump", 2, python);
        base_dump.kind = SymbolKind::Method;
        base_dump.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("Base".into()),
        });
        cache.insert(base_dump);
        let mut rogue_dump = make_symbol(3, "model_dump", 3, python);
        rogue_dump.kind = SymbolKind::Method;
        rogue_dump.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("Rogue".into()),
        });
        cache.insert(rogue_dump);
        cache.insert(make_class(10, "Model", 1, python));
        cache.insert(make_class(11, "Base", 2, python));

        let (stage, scope) = python_stage_with_class_scope(Arc::clone(&cache));
        let context = ResolutionContext {
            file_id: FileId::new(1).unwrap(),
            language_id: python,
            imports: vec![],
            local_symbols: vec![SymbolId::new(1).unwrap()],
            scope,
            unresolved_rels: vec![
                make_instance_call(1, "model_dump", 1, "m"),
                make_extends(10, "Model", "Base", 1),
            ],
            variable_bindings: vec![model_binding()],
        };

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(
            stats.calls_resolved, 1,
            "cross-file inherited member resolves"
        );
        let call = batch
            .relationships
            .iter()
            .find(|r| r.kind == RelationKind::Calls)
            .expect("calls edge present");
        assert_eq!(call.to_id, SymbolId::new(2).unwrap());
    }

    #[test]
    fn own_override_shadows_inherited_member() {
        // Both Model and Base define dump; Model extends Base; m = Model()
        // resolves to Model.dump (distance 0 beats the live chain).
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "use_model", 1, python));
        for (id, class) in [(2u32, "Model"), (3u32, "Base")] {
            let mut method = make_symbol(id, "dump", 1, python);
            method.kind = SymbolKind::Method;
            method.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
                class_name: Some(class.into()),
            });
            cache.insert(method);
        }
        cache.insert(make_class(10, "Model", 1, python));
        cache.insert(make_class(11, "Base", 1, python));

        let (stage, scope) = python_stage_with_class_scope(Arc::clone(&cache));
        let context = ResolutionContext {
            file_id: FileId::new(1).unwrap(),
            language_id: python,
            imports: vec![],
            local_symbols: vec![
                SymbolId::new(1).unwrap(),
                SymbolId::new(2).unwrap(),
                SymbolId::new(3).unwrap(),
            ],
            scope,
            unresolved_rels: vec![
                make_instance_call(1, "dump", 1, "m"),
                make_extends(10, "Model", "Base", 1),
            ],
            variable_bindings: vec![model_binding()],
        };

        let (batch, stats) = stage.resolve(&context);

        assert_eq!(stats.calls_resolved, 1);
        let call = batch
            .relationships
            .iter()
            .find(|r| r.kind == RelationKind::Calls)
            .expect("calls edge present");
        assert_eq!(
            call.to_id,
            SymbolId::new(2).unwrap(),
            "the concrete type's own override shadows the inherited member"
        );
    }

    #[test]
    fn nested_scope_binding_does_not_leak_to_outer_caller() {
        // A binding inside a nested def is not evidence for the enclosing
        // caller's receiver; without usable evidence the call fails closed.
        let python = LanguageId::new("python");
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(make_symbol(1, "use_model", 1, python));
        let mut inner = make_symbol(4, "inner", 1, python);
        inner.range = Range::new(2, 0, 4, 1);
        cache.insert(inner);
        let mut method = make_symbol(2, "dump", 1, python);
        method.kind = SymbolKind::Method;
        method.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: Some("Model".into()),
        });
        cache.insert(method);

        let behaviors: HashMap<LanguageId, StdArc<dyn LanguageBehavior>> = HashMap::from([(
            python,
            StdArc::new(crate::parsing::python::PythonBehavior::new())
                as StdArc<dyn LanguageBehavior>,
        )]);
        let stage = ResolveStage::new(cache, behaviors);
        let mut context = make_context(
            1,
            python,
            vec![
                SymbolId::new(1).unwrap(),
                SymbolId::new(2).unwrap(),
                SymbolId::new(4).unwrap(),
            ],
            vec![make_instance_call(1, "dump", 1, "m")],
        );
        context.variable_bindings = vec![crate::indexing::pipeline::VariableBinding {
            name: "m".into(),
            type_name: "Model".into(),
            range: Range::new(3, 4, 3, 20),
        }];

        let (_, stats) = stage.resolve(&context);

        assert_eq!(stats.resolved, 0, "nested-scope binding must not leak");
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
