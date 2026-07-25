//! Import-binding discipline at the resolution-context builders.
//!
//! A bound import is identity-grade: the receiver anchor and chain walk
//! trust it. The per-language builder loops resolved a binding from the
//! FIRST candidate passing a suffix match (insertion order = file
//! processing order), so a vendored twin that processes first captured
//! the binding while the structurally exact copy existed. Discipline:
//! exact module match outranks suffix; suffix binds only an exactly-one
//! survivor; anything else stays unresolved (External beats wrong-copy).

use codanna::config::Settings;
use codanna::indexing::pipeline::types::SymbolLookupCache;
use codanna::parsing::{Import, LanguageBehavior, LanguageId, ParserFactory};
use codanna::types::{FileId, Range, SymbolId};
use codanna::{Symbol, SymbolKind, Visibility};
use std::sync::Arc;

fn behavior_for(lang: LanguageId) -> Box<dyn LanguageBehavior> {
    let settings = Settings::load().expect("Failed to load settings");
    let factory = ParserFactory::new(Arc::new(settings));
    factory.create_behavior_from_registry(lang)
}

fn symbol(
    id: u32,
    name: &str,
    file: u32,
    module: &str,
    lang: LanguageId,
    file_path: &str,
) -> Symbol {
    let mut sym = Symbol::new(
        SymbolId::new(id).unwrap(),
        name,
        SymbolKind::Class,
        FileId::new(file).unwrap(),
        Range::new(1, 0, 3, 1),
    );
    sym.language_id = Some(lang);
    sym.visibility = Visibility::Public;
    sym.module_path = Some(module.into());
    sym.file_path = file_path.into();
    sym
}

fn import(path: &str, alias: Option<&str>, file: u32) -> Import {
    Import {
        path: path.into(),
        file_id: FileId::new(file).unwrap(),
        alias: alias.map(String::from),
        is_glob: false,
        is_type_only: false,
    }
}

/// Binding for `name` after running the builder over `imports`.
/// Extensions matter for the languages that compute module paths from
/// file paths (javascript strips them before deriving the module).
fn bind(
    lang: LanguageId,
    cache: &SymbolLookupCache,
    imports: &[Import],
    name: &str,
) -> Option<SymbolId> {
    let extensions: &[&str] = match lang.as_str() {
        "javascript" => &["js", "mjs", "cjs"],
        "typescript" => &["ts", "tsx"],
        _ => &["py"],
    };
    let behavior = behavior_for(lang);
    let (context, _) = behavior.build_resolution_context_with_pipeline_cache(
        FileId::new(9).unwrap(),
        imports,
        cache,
        extensions,
    );
    context.import_binding(name).and_then(|b| b.resolved_symbol)
}

fn python() -> LanguageId {
    LanguageId::new("python")
}

#[test]
fn python_binding_prefers_exact_module_over_earlier_suffix_match() {
    let cache = SymbolLookupCache::new();
    // Vendored twin inserted first: candidate order must not decide.
    cache.insert(symbol(
        1,
        "Foo",
        1,
        "avendor.lib.foo",
        python(),
        "avendor/lib/foo.py",
    ));
    cache.insert(symbol(2, "Foo", 2, "lib.foo", python(), "lib/foo.py"));

    let imports = [import("lib.foo.Foo", None, 9)];
    assert_eq!(
        bind(python(), &cache, &imports, "Foo"),
        Some(SymbolId::new(2).unwrap()),
        "from lib.foo import Foo must bind the structurally exact copy"
    );
}

#[test]
fn python_binding_fails_closed_on_suffix_ambiguity() {
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(
        1,
        "Foo",
        1,
        "avendor.lib.foo",
        python(),
        "avendor/lib/foo.py",
    ));
    cache.insert(symbol(
        2,
        "Foo",
        2,
        "bvendor.lib.foo",
        python(),
        "bvendor/lib/foo.py",
    ));

    let imports = [import("lib.foo.Foo", None, 9)];
    assert_eq!(
        bind(python(), &cache, &imports, "Foo"),
        None,
        "two suffix-only twins and no exact copy: the binding stays \
         unresolved (an External classification beats a wrong-copy pick)"
    );
}

fn typescript() -> LanguageId {
    LanguageId::new("typescript")
}

#[test]
fn typescript_binding_prefers_exact_module_over_earlier_suffix_match() {
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(
        1,
        "Foo",
        1,
        "avendor.lib.foo",
        typescript(),
        "avendor/lib/foo.ts",
    ));
    cache.insert(symbol(2, "Foo", 2, "lib.foo", typescript(), "lib/foo.ts"));

    let imports = [import("lib/foo", Some("Foo"), 9)];
    assert_eq!(
        bind(typescript(), &cache, &imports, "Foo"),
        Some(SymbolId::new(2).unwrap()),
        "import {{ Foo }} from 'lib/foo' must bind the structurally exact copy"
    );
}

#[test]
fn typescript_binding_fails_closed_on_suffix_ambiguity() {
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(
        1,
        "Foo",
        1,
        "avendor.lib.foo",
        typescript(),
        "avendor/lib/foo.ts",
    ));
    cache.insert(symbol(
        2,
        "Foo",
        2,
        "bvendor.lib.foo",
        typescript(),
        "bvendor/lib/foo.ts",
    ));

    let imports = [import("lib/foo", Some("Foo"), 9)];
    assert_eq!(bind(typescript(), &cache, &imports, "Foo"), None);
}

fn javascript() -> LanguageId {
    LanguageId::new("javascript")
}

#[test]
fn javascript_binding_prefers_exact_module_over_earlier_suffix_match() {
    // JS matches on module paths computed from file_path (path-based
    // fallback when no jsconfig rules are loaded).
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(
        1,
        "Foo",
        1,
        "avendor.lib.foo",
        javascript(),
        "avendor/lib/foo.js",
    ));
    cache.insert(symbol(2, "Foo", 2, "lib.foo", javascript(), "lib/foo.js"));

    let imports = [import("lib/foo", Some("Foo"), 9)];
    assert_eq!(
        bind(javascript(), &cache, &imports, "Foo"),
        Some(SymbolId::new(2).unwrap()),
    );
}

#[test]
fn javascript_binding_fails_closed_on_suffix_ambiguity() {
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(
        1,
        "Foo",
        1,
        "avendor.lib.foo",
        javascript(),
        "avendor/lib/foo.js",
    ));
    cache.insert(symbol(
        2,
        "Foo",
        2,
        "bvendor.lib.foo",
        javascript(),
        "bvendor/lib/foo.js",
    ));

    let imports = [import("lib/foo", Some("Foo"), 9)];
    assert_eq!(bind(javascript(), &cache, &imports, "Foo"), None);
}

fn rust() -> LanguageId {
    LanguageId::new("rust")
}

#[test]
fn default_builder_ambiguous_import_stays_unresolved() {
    // Default builder (rust rides it): an import whose name resolves
    // Ambiguous through the cache must not bind ids.first(). Here the
    // import is external (no module matches serde), and two internal
    // same-name Public symbols reach tier 3 as an ambiguous pair.
    let cache = SymbolLookupCache::new();
    cache.insert(symbol(1, "Serialize", 1, "liba", rust(), "liba/src/lib.rs"));
    cache.insert(symbol(2, "Serialize", 2, "libb", rust(), "libb/src/lib.rs"));

    let imports = [import("serde::Serialize", None, 9)];
    assert_eq!(
        bind(rust(), &cache, &imports, "Serialize"),
        None,
        "an ambiguous cache resolution must leave the binding unresolved, \
         not first-pick an arbitrary internal symbol for an external import"
    );
}
