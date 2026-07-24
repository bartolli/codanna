//! Python language parser implementation

/// Receiver names python binds to the enclosing instance or class. The
/// behavior vouches these as explicit-alias receivers; the parser reads
/// the same list to decide which defs own their own `self`.
pub(crate) const SELF_ALIASES: &[&str] = &["self", "cls"];

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::PythonBehavior;
pub use definition::PythonLanguage;
pub use parser::PythonParser;
pub use resolution::{PythonInheritanceResolver, PythonResolutionContext};

// Re-export for registry registration
pub(crate) use definition::register;
