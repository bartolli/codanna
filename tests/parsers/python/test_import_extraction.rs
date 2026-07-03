//! Python import extraction: absolute, relative, aliased, glob forms.
//!
//! Relative imports keep their leading dots in the extracted path; the
//! parse stage normalizes them to absolute form against the file's module
//! path before persistence.

use codanna::FileId;
use codanna::parsing::LanguageParser;
use codanna::parsing::python::PythonParser;

fn extract(code: &str) -> Vec<codanna::parsing::Import> {
    let mut parser = PythonParser::new().expect("Failed to create parser");
    parser.find_imports(code, FileId::new(1).unwrap())
}

#[test]
fn test_absolute_from_import() {
    let imports = extract("from pkg.a import helper\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, "pkg.a.helper");
    assert_eq!(imports[0].alias, None);
    assert!(!imports[0].is_glob);
}

#[test]
fn test_relative_from_import() {
    let imports = extract("from .a import helper\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, ".a.helper");
    assert_eq!(imports[0].alias, None);
}

#[test]
fn test_relative_from_import_aliased() {
    let imports = extract("from .a import helper as h\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, ".a.helper");
    assert_eq!(imports[0].alias.as_deref(), Some("h"));
}

#[test]
fn test_relative_dot_only_import() {
    // "from . import a" - pure-dots base joins without an extra dot
    let imports = extract("from . import a\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, ".a");
}

#[test]
fn test_relative_parent_import() {
    let imports = extract("from ..sub.mod import thing\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, "..sub.mod.thing");
}

#[test]
fn test_relative_parent_dot_only_import() {
    let imports = extract("from .. import x\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, "..x");
}

#[test]
fn test_relative_glob_import() {
    let imports = extract("from .a import *\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, ".a");
    assert!(imports[0].is_glob);
}

#[test]
fn test_multiple_names_from_relative_import() {
    let imports = extract("from .main import BaseModel, Field\n");
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].path, ".main.BaseModel");
    assert_eq!(imports[1].path, ".main.Field");
}

#[test]
fn test_simple_import_unchanged() {
    let imports = extract("import os\nimport pkg.a\n");
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].path, "os");
    assert_eq!(imports[1].path, "pkg.a");
}
