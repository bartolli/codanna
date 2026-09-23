//! Import statement representation
//!
//! This module defines the Import struct used by language parsers
//! to represent import statements extracted from source files.

use crate::FileId;

/// Represents an import statement in a file
#[derive(Debug, Clone)]
pub struct Import {
    /// The path being imported (e.g., "std::collections::HashMap")
    pub path: String,
    /// The alias if any (e.g., "use foo::Bar as Baz")
    pub alias: Option<String>,
    /// The imported member when it differs from the local binding
    /// (e.g., `import { foo as bar }` carries `Some("foo")`); `None`
    /// when the local binding is the member's own name
    pub name: Option<String>,
    /// Location in the file where this import appears
    pub file_id: FileId,
    /// Whether this is a glob import (e.g., "use foo::*")
    pub is_glob: bool,
    /// Whether this is a type-only import (TypeScript: `import type { Foo }`)
    pub is_type_only: bool,
}
