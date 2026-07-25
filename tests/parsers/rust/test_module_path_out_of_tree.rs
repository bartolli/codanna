//! Regression tests for module-path computation when the indexed root lies
//! outside the workspace root (out-of-tree indexing).
//!
//! Before the fix, `compute_module_path` stripped file paths against
//! `settings.workspace_root` only; out-of-tree files got `module_path = None`,
//! which disabled `is_same_module` and dropped every cross-file private-symbol
//! resolution (wiki: story-bug-rust-parent-module-method-resolution).

use codanna::config::Settings;
use codanna::indexing::pipeline::types::{CallerContext, FileContent, SymbolLookupCache};
use codanna::indexing::pipeline::{ParseStage, init_parser_cache};
use codanna::parsing::{LanguageId, PipelineSymbolCache, ResolveResult};
use codanna::types::{FileId, Range, SymbolId};
use codanna::{Symbol, SymbolKind, Visibility};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn write_fixture(root: &Path) {
    let widget = root.join("src/widget");
    fs::create_dir_all(&widget).expect("create fixture dirs");
    fs::write(
        widget.join("mod.rs"),
        "mod paint;\nmod render;\n\npub struct Widget { n: u32 }\n\nimpl Widget {\n    fn helper(&self) -> u32 { self.n }\n}\n",
    )
    .expect("write mod.rs");
    fs::write(
        widget.join("render.rs"),
        "use super::Widget;\n\nimpl Widget {\n    pub fn render(&self) -> u32 { self.helper() }\n}\n",
    )
    .expect("write render.rs");
    fs::write(
        widget.join("paint.rs"),
        "use super::Widget;\n\nimpl Widget {\n    pub fn paint(&self) -> u32 { self.render() }\n}\n",
    )
    .expect("write paint.rs");
}

fn out_of_tree_settings(workspace: &Path) -> Arc<Settings> {
    Arc::new(Settings {
        workspace_root: Some(workspace.to_path_buf()),
        ..Settings::default()
    })
}

fn content_for(path: &Path) -> FileContent {
    let content = fs::read_to_string(path).expect("read fixture file");
    FileContent::new(path.to_path_buf(), content, "test-hash".to_string())
}

#[test]
fn test_out_of_tree_walk_root_populates_module_path() {
    let repo = TempDir::new().expect("repo dir");
    let workspace = TempDir::new().expect("workspace dir");
    write_fixture(repo.path());

    let settings = out_of_tree_settings(workspace.path());
    init_parser_cache(settings.clone());
    let stage = ParseStage::new(settings.clone()).with_module_root(Some(repo.path().to_path_buf()));

    let parsed = stage
        .parse(content_for(&repo.path().join("src/widget/mod.rs")))
        .expect("parse mod.rs");
    assert_eq!(parsed.module_path.as_deref(), Some("crate::widget"));

    let parsed = stage
        .parse(content_for(&repo.path().join("src/widget/render.rs")))
        .expect("parse render.rs");
    assert_eq!(parsed.module_path.as_deref(), Some("crate::widget::render"));

    // Without a module root or registered indexed path, the degraded
    // pre-fix behavior remains: no module path.
    let bare_stage = ParseStage::new(settings);
    let parsed = bare_stage
        .parse(content_for(&repo.path().join("src/widget/mod.rs")))
        .expect("parse mod.rs without root");
    assert_eq!(parsed.module_path, None);
}

#[test]
fn test_out_of_tree_indexed_path_fallback_populates_module_path() {
    let repo = TempDir::new().expect("repo dir");
    let workspace = TempDir::new().expect("workspace dir");
    write_fixture(repo.path());

    let settings = Arc::new(Settings {
        workspace_root: Some(workspace.path().to_path_buf()),
        indexed_paths_cache: vec![repo.path().to_path_buf()],
        ..Settings::default()
    });

    init_parser_cache(settings.clone());
    // No walk root: the List-source path (incremental runs) relies on
    // registered indexed paths.
    let stage = ParseStage::new(settings);

    let parsed = stage
        .parse(content_for(&repo.path().join("src/widget/paint.rs")))
        .expect("parse paint.rs");
    assert_eq!(parsed.module_path.as_deref(), Some("crate::widget::paint"));
}

/// Pins the downstream effect: with module paths populated, a private callee
/// in an ancestor module resolves cross-file; with `None` module paths it is
/// invisible (the pre-fix symptom).
#[test]
fn test_private_ancestor_callee_resolution_requires_module_paths() {
    let lang = LanguageId::new("rust");
    let mod_file = FileId::new(1).unwrap();
    let render_file = FileId::new(2).unwrap();

    let make_helper = |module_path: Option<&str>| {
        let mut sym = Symbol::new(
            SymbolId::new(1).unwrap(),
            "helper",
            SymbolKind::Method,
            mod_file,
            Range::new(6, 0, 6, 40),
        );
        sym.language_id = Some(lang);
        sym.visibility = Visibility::Private;
        sym.module_path = module_path.map(Into::into);
        sym
    };

    // Populated module paths: caller in crate::widget::render sees the
    // private helper defined in ancestor module crate::widget.
    let cache = SymbolLookupCache::new();
    cache.insert(make_helper(Some("crate::widget")));
    let caller = CallerContext::new(
        render_file,
        Some("crate::widget::render".into()),
        lang,
        "::",
    );
    let result = cache.resolve("helper", &caller, Some(&Range::new(4, 0, 4, 30)), &[]);
    assert!(
        matches!(result, ResolveResult::Found(id) if id == SymbolId::new(1).unwrap()),
        "private ancestor-module callee must resolve when module paths are populated, got {result:?}"
    );

    // None module paths: same-module check cannot fire, Public-only gate
    // hides the private callee.
    let cache = SymbolLookupCache::new();
    cache.insert(make_helper(None));
    let caller = CallerContext::new(render_file, None, lang, "::");
    let result = cache.resolve("helper", &caller, Some(&Range::new(4, 0, 4, 30)), &[]);
    assert!(
        matches!(result, ResolveResult::NotFound),
        "private cross-file callee without module paths stays unresolved, got {result:?}"
    );
}
