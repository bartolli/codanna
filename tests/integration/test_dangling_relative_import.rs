//! A relative import whose target file is absent from the index is typed
//! negative evidence: the binding owns its name, and no name-keyed tier
//! may bind a same-name export from another file to it.
//!
//! Three surfaces, all Tantivy-free:
//! - the relative-specifier lookup's three outcomes, including that only
//!   a cache built from every file row may answer `NoIndexedFile`;
//! - the TypeScript and JavaScript context builders registering the
//!   `Dangling` origin and installing nothing into scope, while a
//!   same-stem sibling or a directory below the resolved path keeps the
//!   lookup `Unknown`;
//! - the shared resolver failing closed on a dangling-owned receiver-less
//!   name while a local definition and receiver-bearing rows keep
//!   resolving.

use codanna::config::Settings;
use codanna::indexing::pipeline::types::{
    CallerContext, ResolutionContext, ResolvedBatch, SymbolLookupCache, UnresolvedRelationship,
    VariableBinding,
};
use codanna::indexing::pipeline::{ResolveStage, ResolveStats};
use codanna::parsing::javascript::JavaScriptResolutionContext;
use codanna::parsing::resolution::{
    FilePresence, ImportBinding, ImportOrigin, RelativeImportLookup, ResolutionScope,
};
use codanna::parsing::{
    Import, LanguageBehavior, LanguageId, ParserFactory, PipelineSymbolCache, ResolveResult,
};
use codanna::relationship::RelationshipMetadata;
use codanna::symbol::ScopeContext;
use codanna::types::{FileId, Range, SymbolId};
use codanna::{RelationKind, Symbol, SymbolKind, Visibility};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const TS_EXTENSIONS: [&str; 4] = ["ts", "tsx", "mts", "cts"];
const JS_EXTENSIONS: [&str; 4] = ["js", "jsx", "mjs", "cjs"];

fn ts() -> LanguageId {
    LanguageId::new("typescript")
}

fn js() -> LanguageId {
    LanguageId::new("javascript")
}

fn behavior(lang: LanguageId) -> Arc<dyn LanguageBehavior> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    Arc::from(factory.create_behavior_from_registry(lang))
}

fn behaviors() -> HashMap<LanguageId, Arc<dyn LanguageBehavior>> {
    let mut map = HashMap::new();
    for lang in [ts(), js()] {
        map.insert(lang, behavior(lang));
    }
    map
}

fn symbol(
    id: u32,
    name: &str,
    kind: SymbolKind,
    file: u32,
    path: &str,
    lang: LanguageId,
) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        kind,
        FileId::new(file).unwrap(),
        Range::new(1, 0, 3, 1),
    )
    .with_file_path(path)
    .with_language_id(lang)
    .with_visibility(Visibility::Public);
    sym.scope_context = Some(ScopeContext::Module);
    sym
}

fn method(id: u32, name: &str, file: u32, path: &str, class: &str, lang: LanguageId) -> Symbol {
    let mut sym = symbol(id, name, SymbolKind::Method, file, path, lang);
    sym.scope_context = Some(ScopeContext::ClassMember {
        class_name: Some(class.to_string().into()),
    });
    sym
}

fn complete_cache(files: &[&str]) -> SymbolLookupCache {
    SymbolLookupCache::with_indexed_files(files.iter().map(PathBuf::from))
}

/// `import { <local> } from '<path>'` as the TypeScript and JavaScript
/// parsers emit it: the local binding rides `alias`, `name` only when the
/// imported member differs from it.
fn named_import(local: &str, path: &str) -> Import {
    Import {
        path: path.into(),
        file_id: FileId::new(1).unwrap(),
        name: None,
        alias: Some(local.into()),
        is_glob: false,
        is_type_only: false,
    }
}

fn lookup(
    behavior: &dyn LanguageBehavior,
    cache: &dyn PipelineSymbolCache,
    specifier: &str,
    extensions: &[&str],
) -> RelativeImportLookup {
    behavior.resolve_relative_import(
        cache,
        "sharedTarget",
        specifier,
        "repo_a/caller.ts",
        extensions,
    )
}

// ---- lookup classification ----

#[test]
fn walk_scoped_cache_answers_unknown_not_absent() {
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_b/target.ts",
        ts(),
    ));

    assert_eq!(
        cache.file_presence(Path::new("repo_a/target.ts")),
        FilePresence::Unknown,
        "a cache holding only its walk's files cannot prove a path absent"
    );
    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./target", &TS_EXTENSIONS),
        RelativeImportLookup::Unknown
    );
}

#[test]
fn complete_cache_answers_presence_from_file_rows() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_a/empty.ts"]);
    assert_eq!(
        cache.file_presence(Path::new("repo_a/empty.ts")),
        FilePresence::Present,
        "a file row with zero symbols is present"
    );
    assert_eq!(
        cache.file_presence(Path::new("repo_a/target.ts")),
        FilePresence::Absent
    );
}

#[test]
fn every_candidate_path_absent_is_no_indexed_file() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_b/target.ts"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_b/target.ts",
        ts(),
    ));

    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./target", &TS_EXTENSIONS),
        RelativeImportLookup::NoIndexedFile,
        "a same-name export at a non-candidate path is not the target"
    );
}

#[test]
fn present_file_without_the_name_is_unknown() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_a/target.ts", "repo_b/target.ts"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        3,
        "repo_b/target.ts",
        ts(),
    ));

    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./target", &TS_EXTENSIONS),
        RelativeImportLookup::Unknown,
        "an indexed file that does not define the name is not negative evidence"
    );
}

#[test]
fn exactly_one_match_at_a_candidate_path_binds() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_a/target.ts", "repo_b/target.ts"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_a/target.ts",
        ts(),
    ));
    cache.insert(symbol(
        3,
        "sharedTarget",
        SymbolKind::Function,
        3,
        "repo_b/target.ts",
        ts(),
    ));

    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./target", &TS_EXTENSIONS),
        RelativeImportLookup::Bound(SymbolId::new(2).unwrap())
    );
}

#[test]
fn more_than_one_match_is_unknown() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_a/target.ts", "repo_a/target.tsx"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_a/target.ts",
        ts(),
    ));
    cache.insert(symbol(
        3,
        "sharedTarget",
        SymbolKind::Function,
        3,
        "repo_a/target.tsx",
        ts(),
    ));

    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./target", &TS_EXTENSIONS),
        RelativeImportLookup::Unknown
    );
}

#[test]
fn non_relative_specifier_is_unknown() {
    let cache = complete_cache(&["repo_a/caller.ts"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));

    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "lodash", &TS_EXTENSIONS),
        RelativeImportLookup::Unknown
    );
}

// ---- structural arms: same-stem siblings and directories ----

// Extension-substituted siblings share the file name up to its first
// dot, so such a sibling keeps the lookup out of negative evidence.
#[test]
fn sibling_sharing_first_dot_stem_is_unknown() {
    for (lang, exts, sibling, specifier) in [
        (ts(), &TS_EXTENSIONS, "repo_a/dep.ts", "./dep.js"),
        (js(), &JS_EXTENSIONS, "repo_a/dep.jsx", "./dep.js"),
        (ts(), &TS_EXTENSIONS, "repo_a/dep.tsx", "./dep.jsx"),
        (ts(), &TS_EXTENSIONS, "repo_a/dep.d.ts", "./dep.js"),
        (ts(), &TS_EXTENSIONS, "repo_a/dep.test.ts", "./dep"),
        (ts(), &TS_EXTENSIONS, "repo_a/app.core.ts", "./app.core.js"),
        (ts(), &TS_EXTENSIONS, "repo_a/dep.mts", "./dep.mjs"),
    ] {
        let cache = complete_cache(&["repo_a/caller.ts", sibling]);
        assert_eq!(
            lookup(behavior(lang).as_ref(), &cache, specifier, exts),
            RelativeImportLookup::Unknown,
            "{specifier} beside {sibling}"
        );
    }
}

#[test]
fn directory_below_resolved_path_is_unknown() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_a/widgets/index.vue"]);
    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./widgets", &TS_EXTENSIONS),
        RelativeImportLookup::Unknown,
        "a file below the resolved path may be its directory index"
    );
    let cache = complete_cache(&["repo_a/caller.ts", "shared/util/x.ts"]);
    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "../shared", &TS_EXTENSIONS),
        RelativeImportLookup::Unknown
    );
}

#[test]
fn no_sibling_and_no_directory_is_no_indexed_file() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_a/other.ts", "repo_b/dep.ts"]);
    assert_eq!(
        lookup(behavior(ts()).as_ref(), &cache, "./dep", &TS_EXTENSIONS),
        RelativeImportLookup::NoIndexedFile,
        "a same-stem file in another directory is no sibling"
    );
}

#[test]
fn structural_queries_answer_from_the_complete_view() {
    let cache = complete_cache(&[
        "repo_a/dep.d.ts",
        "repo_a/widgets/index.vue",
        "repo_b/dep.ts",
    ]);
    assert_eq!(
        cache.sibling_stem_present(Path::new("repo_a/dep.js")),
        FilePresence::Present
    );
    assert_eq!(
        cache.sibling_stem_present(Path::new("repo_a/other.js")),
        FilePresence::Absent
    );
    assert_eq!(
        cache.directory_present(Path::new("repo_a/widgets")),
        FilePresence::Present
    );
    assert_eq!(
        cache.directory_present(Path::new("repo_a/widgets/index.vue")),
        FilePresence::Absent,
        "a file is not a directory"
    );

    let walk_scoped = SymbolLookupCache::new();
    assert_eq!(
        walk_scoped.sibling_stem_present(Path::new("repo_a/dep.js")),
        FilePresence::Unknown
    );
    assert_eq!(
        walk_scoped.directory_present(Path::new("repo_a/widgets")),
        FilePresence::Unknown
    );
}

// ---- builders ----

fn build_scope(
    lang: LanguageId,
    cache: &SymbolLookupCache,
    local: &str,
    specifier: &str,
    extensions: &[&str],
) -> Box<dyn ResolutionScope> {
    let (scope, _enhanced) = behavior(lang).build_resolution_context_with_pipeline_cache(
        FileId::new(1).unwrap(),
        &[named_import(local, specifier)],
        cache,
        extensions,
    );
    scope
}

#[test]
fn ts_builder_registers_dangling_and_installs_nothing() {
    let cache = complete_cache(&["repo_a/caller.ts", "repo_b/target.ts"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.ts",
        ts(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_b/target.ts",
        ts(),
    ));

    let scope = build_scope(ts(), &cache, "sharedTarget", "./target", &TS_EXTENSIONS);
    let binding = scope
        .import_binding("sharedTarget")
        .expect("binding registered");
    assert_eq!(binding.origin, ImportOrigin::Dangling);
    assert_eq!(binding.resolved_symbol, None);
    assert_eq!(
        scope.resolve("sharedTarget"),
        None,
        "a dangling import installs nothing into scope"
    );
}

#[test]
fn js_builder_registers_dangling_and_installs_nothing() {
    let cache = complete_cache(&["repo_a/caller.js", "repo_b/target.js"]);
    cache.insert(symbol(
        1,
        "entry",
        SymbolKind::Function,
        1,
        "repo_a/caller.js",
        js(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_b/target.js",
        js(),
    ));

    let scope = build_scope(js(), &cache, "sharedTarget", "./target", &JS_EXTENSIONS);
    let binding = scope
        .import_binding("sharedTarget")
        .expect("binding registered");
    assert_eq!(binding.origin, ImportOrigin::Dangling);
    assert_eq!(binding.resolved_symbol, None);
    assert_eq!(scope.resolve("sharedTarget"), None);
}

#[test]
fn builders_on_a_walk_scoped_cache_keep_the_external_origin() {
    for (lang, caller, target, exts) in [
        (ts(), "repo_a/caller.ts", "repo_b/target.ts", &TS_EXTENSIONS),
        (js(), "repo_a/caller.js", "repo_b/target.js", &JS_EXTENSIONS),
    ] {
        let cache = SymbolLookupCache::new();
        cache.insert(symbol(1, "entry", SymbolKind::Function, 1, caller, lang));
        cache.insert(symbol(
            2,
            "sharedTarget",
            SymbolKind::Function,
            2,
            target,
            lang,
        ));

        let scope = build_scope(lang, &cache, "sharedTarget", "./target", exts);
        let binding = scope
            .import_binding("sharedTarget")
            .expect("binding registered");
        assert_eq!(
            binding.origin,
            ImportOrigin::External,
            "{lang:?}: absence unprovable, so the binding keeps the pre-existing origin"
        );
    }
}

/// Build the scope for `import { f } from '<specifier>'` in
/// `repo_a/caller.<ext>` over a complete cache whose file rows are
/// `files`, with `f` defined at `defined_at` when given.
fn substituted_binding(
    lang: LanguageId,
    exts: &[&str],
    specifier: &str,
    files: &[&str],
    defined_at: Option<&str>,
) -> ImportBinding {
    let caller = files[0];
    let cache = complete_cache(files);
    cache.insert(symbol(1, "entry", SymbolKind::Function, 1, caller, lang));
    if let Some(path) = defined_at {
        cache.insert(symbol(2, "f", SymbolKind::Function, 2, path, lang));
    }
    let (scope, _) = behavior(lang).build_resolution_context_with_pipeline_cache(
        FileId::new(1).unwrap(),
        &[named_import("f", specifier)],
        &cache,
        exts,
    );
    scope.import_binding("f").expect("binding registered")
}

// A substituted specifier is negative evidence only once every twin
// source path is provably absent. With a twin present the shape stays
// unmodeled: the binding keeps the pre-existing origin and no scope
// entry (binding through the substitution is out of scope).
#[test]
fn ts_js_specifier_with_ts_twin_present_is_not_dangling() {
    let binding = substituted_binding(
        ts(),
        &TS_EXTENSIONS,
        "./dep.js",
        &["repo_a/caller.ts", "repo_a/dep.ts"],
        Some("repo_a/dep.ts"),
    );
    assert_eq!(binding.origin, ImportOrigin::External);
    assert_eq!(binding.resolved_symbol, None);
}

#[test]
fn ts_js_specifier_with_no_twin_is_dangling() {
    let binding = substituted_binding(
        ts(),
        &TS_EXTENSIONS,
        "./dep.js",
        &["repo_a/caller.ts"],
        None,
    );
    assert_eq!(binding.origin, ImportOrigin::Dangling);
}

#[test]
fn js_specifier_with_js_target_present_binds() {
    let binding = substituted_binding(
        js(),
        &JS_EXTENSIONS,
        "./dep.js",
        &["repo_a/caller.js", "repo_a/dep.js"],
        Some("repo_a/dep.js"),
    );
    assert_eq!(binding.origin, ImportOrigin::Internal);
    assert_eq!(binding.resolved_symbol, Some(SymbolId::new(2).unwrap()));
}

#[test]
fn js_specifier_with_nothing_present_is_dangling() {
    let binding = substituted_binding(
        js(),
        &JS_EXTENSIONS,
        "./dep.js",
        &["repo_a/caller.js"],
        None,
    );
    assert_eq!(binding.origin, ImportOrigin::Dangling);
}

#[test]
fn mjs_and_cjs_specifiers_with_their_twins_present_are_not_dangling() {
    for (specifier, twin) in [
        ("./dep.mjs", "repo_a/dep.mts"),
        ("./dep.cjs", "repo_a/dep.cts"),
    ] {
        let binding = substituted_binding(
            ts(),
            &TS_EXTENSIONS,
            specifier,
            &["repo_a/caller.ts", twin],
            Some(twin),
        );
        assert_eq!(
            binding.origin,
            ImportOrigin::External,
            "{specifier} with {twin} indexed"
        );
    }
}

// The stem rule enumerates no substitutions: `dep.ts` shares `dep` with
// `./dep.mjs`, so the shape stays unmodeled rather than dangling.
#[test]
fn mjs_specifier_with_ts_sibling_is_not_dangling() {
    let binding = substituted_binding(
        ts(),
        &TS_EXTENSIONS,
        "./dep.mjs",
        &["repo_a/caller.ts", "repo_a/dep.ts"],
        Some("repo_a/dep.ts"),
    );
    assert_eq!(binding.origin, ImportOrigin::External);
    assert_eq!(binding.resolved_symbol, None);
}

// A hand-written declaration beside an unindexed build output: the
// twin path is `dep.d.ts` (the specifier's extension replaced by
// `d.ts`), and its presence keeps the shape unmodeled.
#[test]
fn ts_js_specifier_with_only_declaration_twin_present_is_not_dangling() {
    let binding = substituted_binding(
        ts(),
        &TS_EXTENSIONS,
        "./dep.js",
        &["repo_a/caller.ts", "repo_a/dep.d.ts"],
        Some("repo_a/dep.d.ts"),
    );
    assert_eq!(binding.origin, ImportOrigin::External);
    assert_eq!(binding.resolved_symbol, None);
}

// ---- shared resolver ----

fn relation(
    kind: RelationKind,
    to_name: &str,
    metadata: Option<RelationshipMetadata>,
) -> UnresolvedRelationship {
    UnresolvedRelationship {
        from_id: Some(SymbolId::new(1).unwrap()),
        from_name: "origin".into(),
        to_name: to_name.into(),
        file_id: FileId::new(1).unwrap(),
        kind,
        metadata,
        to_range: Some(Range::new(12, 1, 12, 20)),
    }
}

fn dangling_binding(name: &str) -> ImportBinding {
    ImportBinding {
        import: named_import(name, &format!("./{name}")),
        exposed_name: name.to_string(),
        origin: ImportOrigin::Dangling,
        resolved_symbol: None,
    }
}

fn resolve(
    cache: Arc<SymbolLookupCache>,
    scope: JavaScriptResolutionContext,
    rel: UnresolvedRelationship,
    variable_bindings: Vec<VariableBinding>,
) -> (ResolvedBatch, ResolveStats) {
    let stage = ResolveStage::new(Arc::clone(&cache), behaviors());
    let context = ResolutionContext {
        file_id: FileId::new(1).unwrap(),
        language_id: js(),
        imports: vec![],
        local_symbols: vec![SymbolId::new(1).unwrap()],
        scope: Box::new(scope),
        unresolved_rels: vec![rel],
        variable_bindings,
        this_barrier_spans: vec![],
    };
    stage.resolve(&context)
}

fn two_file_cache() -> Arc<SymbolLookupCache> {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(symbol(
        1,
        "origin",
        SymbolKind::Function,
        1,
        "repo_a/caller.js",
        js(),
    ));
    cache.insert(symbol(
        2,
        "sharedTarget",
        SymbolKind::Function,
        2,
        "repo_b/target.js",
        js(),
    ));
    cache
}

#[test]
fn dangling_owned_bare_call_fails_closed_before_the_global_tiers() {
    // Control: without the binding the ladder binds the unique same-language
    // candidate by name alone - the row this fix withholds.
    let (batch, _) = resolve(
        two_file_cache(),
        JavaScriptResolutionContext::new(FileId::new(1).unwrap()),
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(
        batch.len(),
        1,
        "control: name-only pick without negative evidence"
    );

    let mut scope = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
    scope.register_import_binding(dangling_binding("sharedTarget"));
    let (batch, stats) = resolve(
        two_file_cache(),
        scope,
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(batch.len(), 0, "a dangling-owned name never binds by name");
    assert_eq!(stats.total_processed, 1);
}

#[test]
fn dangling_owned_extends_fails_closed() {
    let cache = Arc::new(SymbolLookupCache::new());
    cache.insert(symbol(
        1,
        "origin",
        SymbolKind::Class,
        1,
        "repo_a/caller.js",
        js(),
    ));
    cache.insert(symbol(
        2,
        "Base",
        SymbolKind::Class,
        2,
        "repo_b/base.js",
        js(),
    ));

    let (batch, _) = resolve(
        Arc::clone(&cache),
        JavaScriptResolutionContext::new(FileId::new(1).unwrap()),
        relation(RelationKind::Extends, "Base", None),
        vec![],
    );
    assert_eq!(
        batch.len(),
        1,
        "control: Extends binds the unique candidate"
    );

    let mut scope = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
    scope.register_import_binding(dangling_binding("Base"));
    let (batch, _) = resolve(
        cache,
        scope,
        relation(RelationKind::Extends, "Base", None),
        vec![],
    );
    assert_eq!(
        batch.len(),
        0,
        "every relation kind honors the negative evidence"
    );
}

// F3b: the ladder's same-language arm is unreachable for a dangling-owned
// name. Two candidates, exactly one same-language: today's arm resolves
// it (control); a `Dangling` binding fails closed before the ladder.
#[test]
fn dangling_owned_name_never_reaches_the_same_language_arm() {
    let two_language_cache = || {
        let cache = Arc::new(SymbolLookupCache::new());
        cache.insert(symbol(
            1,
            "origin",
            SymbolKind::Function,
            1,
            "repo_a/caller.js",
            js(),
        ));
        cache.insert(symbol(
            2,
            "sharedTarget",
            SymbolKind::Function,
            2,
            "repo_b/target.js",
            js(),
        ));
        cache.insert(symbol(
            3,
            "sharedTarget",
            SymbolKind::Function,
            3,
            "repo_c/target.py",
            LanguageId::new("python"),
        ));
        cache
    };
    let (control, _) = resolve(
        two_language_cache(),
        JavaScriptResolutionContext::new(FileId::new(1).unwrap()),
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(
        control.len(),
        1,
        "control: the one same-language candidate resolves"
    );
    assert_eq!(control.relationships[0].to_id, SymbolId::new(2).unwrap());

    let mut scope = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
    scope.register_import_binding(dangling_binding("sharedTarget"));
    let (batch, stats) = resolve(
        two_language_cache(),
        scope,
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(
        batch.len(),
        0,
        "a dangling-owned name never reaches the ladder"
    );
    assert_eq!(stats.total_processed, 1);
}

// F3b, `Ambiguous` path: two same-language `Public` candidates in
// different modules make the cache answer `Ambiguous`, so the control
// enters `disambiguate` and resolves through its exactly-one-same-module
// arm (the candidate whose module nests under the caller's). A `Dangling`
// binding fails closed before the ladder.
#[test]
fn dangling_owned_name_never_reaches_disambiguate() {
    let ambiguous_cache = || {
        let cache = Arc::new(SymbolLookupCache::new());
        let mut origin = symbol(1, "origin", SymbolKind::Function, 1, "app/main.js", js());
        origin.module_path = Some("app.main".into());
        cache.insert(origin);
        let mut vendor = symbol(
            2,
            "sharedTarget",
            SymbolKind::Function,
            2,
            "vendor/a.js",
            js(),
        );
        vendor.module_path = Some("vendor.a".into());
        cache.insert(vendor);
        let mut local = symbol(
            3,
            "sharedTarget",
            SymbolKind::Function,
            3,
            "app/main/util.js",
            js(),
        );
        local.module_path = Some("app.main.util".into());
        cache.insert(local);
        cache
    };

    // Entry condition: the cache hands both candidates to `disambiguate`.
    let caller = CallerContext {
        file_id: FileId::new(1).unwrap(),
        module_path: Some("app.main".into()),
        language_id: js(),
        separator: ".",
    };
    let result = ambiguous_cache().resolve("sharedTarget", &caller, None, &[]);
    assert!(
        matches!(&result, ResolveResult::Ambiguous(ids) if ids.len() == 2),
        "two same-language candidates must reach disambiguate: {result:?}"
    );

    let (control, _) = resolve(
        ambiguous_cache(),
        JavaScriptResolutionContext::new(FileId::new(1).unwrap()),
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(control.len(), 1, "control: the same-module arm resolves");
    assert_eq!(
        control.relationships[0].to_id,
        SymbolId::new(3).unwrap(),
        "exactly-one-same-module arm picks app.main.util"
    );

    let mut scope = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
    scope.register_import_binding(dangling_binding("sharedTarget"));
    let (batch, stats) = resolve(
        ambiguous_cache(),
        scope,
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(
        batch.len(),
        0,
        "a dangling-owned name never reaches disambiguate"
    );
    assert_eq!(stats.total_processed, 1);
}

#[test]
fn local_definition_wins_over_a_dangling_binding() {
    let cache = two_file_cache();
    cache.insert(symbol(
        3,
        "sharedTarget",
        SymbolKind::Function,
        1,
        "repo_a/caller.js",
        js(),
    ));

    let mut scope = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
    scope.register_import_binding(dangling_binding("sharedTarget"));
    scope.add_symbol(
        "sharedTarget".into(),
        SymbolId::new(3).unwrap(),
        codanna::parsing::ScopeLevel::Module,
    );
    let (batch, _) = resolve(
        cache,
        scope,
        relation(RelationKind::Calls, "sharedTarget", None),
        vec![],
    );
    assert_eq!(
        batch.len(),
        1,
        "the file's own definition is independent evidence"
    );
    assert_eq!(batch.relationships[0].to_id, SymbolId::new(3).unwrap());
}

fn widget_cache() -> Arc<SymbolLookupCache> {
    let cache = Arc::new(SymbolLookupCache::new());
    // The caller spans the binding site (line 3) and the call sites (line 12).
    let mut origin = symbol(
        1,
        "origin",
        SymbolKind::Function,
        1,
        "repo_a/caller.js",
        js(),
    );
    origin.range = Range::new(1, 0, 20, 1);
    cache.insert(origin);
    cache.insert(symbol(
        2,
        "Widget",
        SymbolKind::Class,
        2,
        "repo_b/widget.js",
        js(),
    ));
    cache.insert(method(3, "render", 2, "repo_b/widget.js", "Widget", js()));
    cache.insert(method(4, "create", 2, "repo_b/widget.js", "Widget", js()));
    cache
}

#[test]
fn receiver_bearing_rows_keep_their_own_evidence() {
    let static_call = RelationshipMetadata::new()
        .at_position(5, 9)
        .with_receiver("Widget")
        .static_call(true);
    let typed_receiver = RelationshipMetadata::new()
        .at_position(4, 2)
        .with_receiver("w")
        .static_call(false);
    let binding = VariableBinding {
        name: "w".into(),
        type_name: "Widget".into(),
        range: Range::new(3, 2, 3, 24),
    };

    for (label, rel, bindings, expected) in [
        (
            "static",
            relation(RelationKind::Calls, "create", Some(static_call)),
            vec![],
            4,
        ),
        (
            "typed receiver",
            relation(RelationKind::Calls, "render", Some(typed_receiver)),
            vec![binding],
            3,
        ),
    ] {
        let (control, _) = resolve(
            widget_cache(),
            JavaScriptResolutionContext::new(FileId::new(1).unwrap()),
            rel.clone(),
            bindings.clone(),
        );
        assert_eq!(control.len(), 1, "{label}: control resolves");

        let mut scope = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
        scope.register_import_binding(dangling_binding("Widget"));
        let (batch, _) = resolve(widget_cache(), scope, rel, bindings);
        assert_eq!(
            batch.len(),
            1,
            "{label}: receiver-bearing rows are not gated"
        );
        assert_eq!(
            batch.relationships[0].to_id,
            SymbolId::new(expected).unwrap()
        );
    }
}
