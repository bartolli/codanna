//! Phase 2 orchestration: two-pass relationship resolution.

use super::{
    ContextStage, Phase2Stats, Pipeline, PipelineError, PipelineResult, ResolveStage,
    SymbolLookupCache, UnresolvedRelationship, WriteStage,
};
use crate::RelationKind;
use crate::parsing::ParserFactory;
use crate::storage::DocumentIndex;
use std::sync::Arc;
use std::time::Instant;

impl Pipeline {
    /// Run Phase 2: Resolve relationships using two-pass strategy.
    ///
    /// [PIPELINE API] This resolves all pending relationships from Phase 1:
    /// 1. Pass 1: Resolve Defines relationships (class→method, module→function)
    /// 2. Commit barrier: Defines are now queryable
    /// 3. Pass 2: Resolve Calls (can reference Defines)
    ///
    /// # Arguments
    /// * `unresolved` - Pending relationships from Phase 1
    /// * `symbol_cache` - SymbolLookupCache populated by Phase 1
    /// * `index` - DocumentIndex for reading imports and writing relationships
    ///
    /// # Returns
    /// Phase2Stats with resolution counts
    pub fn run_phase2(
        &self,
        unresolved: Vec<UnresolvedRelationship>,
        symbol_cache: Arc<SymbolLookupCache>,
        index: Arc<DocumentIndex>,
    ) -> PipelineResult<Phase2Stats> {
        self.run_phase2_with_progress(unresolved, symbol_cache, index, None)
    }

    /// Run Phase 2 with optional progress bar.
    pub fn run_phase2_with_progress(
        &self,
        unresolved: Vec<UnresolvedRelationship>,
        symbol_cache: Arc<SymbolLookupCache>,
        index: Arc<DocumentIndex>,
        progress: Option<Arc<crate::io::status_line::ProgressBar>>,
    ) -> PipelineResult<Phase2Stats> {
        let start = Instant::now();
        let total_relationships = unresolved.len();

        if unresolved.is_empty() {
            return Ok(Phase2Stats {
                total_relationships: 0,
                defines_resolved: 0,
                calls_resolved: 0,
                other_resolved: 0,
                unresolved: 0,
                elapsed: start.elapsed(),
            });
        }

        // Create stages
        let factory = Arc::new(ParserFactory::new(Arc::clone(&self.settings)));
        let context_stage = ContextStage::new(
            Arc::clone(&symbol_cache),
            Arc::clone(&index),
            factory,
            Arc::clone(&self.settings),
        );
        let mut write_stage = WriteStage::new(Arc::clone(&index));

        // Split relationships by kind
        let (defines, others): (Vec<_>, Vec<_>) = unresolved
            .into_iter()
            .partition(|rel| rel.kind == RelationKind::Defines);

        let mut stats = Phase2Stats {
            total_relationships,
            ..Default::default()
        };

        // Pass 1: Resolve Defines
        tracing::info!(
            target: "pipeline",
            "Phase 2 Pass 1: Resolving {} Defines relationships",
            defines.len()
        );
        if !defines.is_empty() {
            let contexts = context_stage.build_contexts(defines);
            let behaviors = context_stage.behaviors();
            let resolve_stage = ResolveStage::new(Arc::clone(&symbol_cache), behaviors);

            for ctx in contexts {
                let rel_count = ctx.unresolved_rels.len() as u64;
                let (batch, resolve_stats) = resolve_stage.resolve(&ctx);
                stats.defines_resolved += resolve_stats.defines_resolved;
                write_stage.write(batch);

                // Update progress bar
                if let Some(ref prog) = progress {
                    prog.set_progress(prog.current() + rel_count);
                    prog.add_extra1(resolve_stats.defines_resolved as u64);
                    let skipped = rel_count.saturating_sub(resolve_stats.defines_resolved as u64);
                    prog.add_extra2(skipped);
                }
            }

            // BARRIER: Commit Defines so Pass 2 can query them
            write_stage
                .commit()
                .map_err(|e| PipelineError::Index(crate::IndexError::General(e.to_string())))?;
        }

        // Pass 2: Resolve Calls and other relationships
        tracing::info!(
            target: "pipeline",
            "Phase 2 Pass 2: Resolving {} Calls/other relationships",
            others.len()
        );
        if !others.is_empty() {
            // Sequencing invariant: populate per-language InheritanceResolvers from
            // Extends relationships BEFORE build_contexts(others) consumes the vec
            // and BEFORE any Calls resolution in this pass fires resolve_static_call.
            let inheritance_resolvers = context_stage.build_inheritance_resolvers(&others);
            let contexts = context_stage.build_contexts(others);
            let behaviors = context_stage.behaviors();
            let resolve_stage = ResolveStage::new(Arc::clone(&symbol_cache), behaviors)
                .with_inheritance_resolvers(inheritance_resolvers);

            for ctx in contexts {
                let rel_count = ctx.unresolved_rels.len() as u64;
                let (batch, resolve_stats) = resolve_stage.resolve(&ctx);
                stats.calls_resolved += resolve_stats.calls_resolved;
                stats.other_resolved += resolve_stats.resolved - resolve_stats.calls_resolved;
                write_stage.write(batch);

                // Update progress bar
                if let Some(ref prog) = progress {
                    prog.set_progress(prog.current() + rel_count);
                    prog.add_extra1(resolve_stats.resolved as u64);
                    let skipped = rel_count.saturating_sub(resolve_stats.resolved as u64);
                    prog.add_extra2(skipped);
                }
            }

            // Final commit
            write_stage
                .flush()
                .map_err(|e| PipelineError::Index(crate::IndexError::General(e.to_string())))?;
        }

        stats.unresolved = stats.total_relationships
            - stats.defines_resolved
            - stats.calls_resolved
            - stats.other_resolved;
        stats.elapsed = start.elapsed();

        tracing::info!(
            target: "pipeline",
            "Phase 2 complete: resolved {}/{} ({} Defines, {} Calls, {} other) in {:?}",
            stats.defines_resolved + stats.calls_resolved + stats.other_resolved,
            stats.total_relationships,
            stats.defines_resolved,
            stats.calls_resolved,
            stats.other_resolved,
            stats.elapsed
        );

        Ok(stats)
    }

    /// Run Phase 2 behind an optional LINKS progress bar.
    ///
    /// With progress requested and work pending, renders the LINKS bar and
    /// prints its final state after the StatusLine drops. Otherwise delegates
    /// straight to `run_phase2`.
    pub(super) fn run_phase2_maybe_bar(
        &self,
        unresolved: Vec<UnresolvedRelationship>,
        symbol_cache: Arc<SymbolLookupCache>,
        index: Arc<DocumentIndex>,
        show_progress: bool,
    ) -> PipelineResult<Phase2Stats> {
        use crate::io::status_line::{
            ProgressBar, ProgressBarOptions, ProgressBarStyle, StatusLine,
        };

        if !show_progress || unresolved.is_empty() {
            return self.run_phase2(unresolved, symbol_cache, index);
        }

        let options = ProgressBarOptions::default()
            .with_style(ProgressBarStyle::VerticalSolid)
            .with_width(28)
            .with_label("LINKS")
            .show_rate(false); // Rate not meaningful for relationships
        let phase2_bar = Arc::new(ProgressBar::with_options(
            unresolved.len() as u64,
            "relationships",
            "resolved",
            "skipped",
            options,
        ));
        let phase2_status = StatusLine::new(Arc::clone(&phase2_bar));

        let stats = self.run_phase2_with_progress(
            unresolved,
            symbol_cache,
            index,
            Some(phase2_bar.clone()),
        )?;

        drop(phase2_status);
        eprintln!("{phase2_bar}");
        Ok(stats)
    }
}
