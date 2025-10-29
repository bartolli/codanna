//! Godot GDScript language parser implementation

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use audit::GdscriptParserAudit;
pub use behavior::GdscriptBehavior;
pub use definition::GdscriptLanguage;
pub use parser::GdscriptParser;
pub use resolution::{GdscriptInheritanceResolver, GdscriptResolutionContext};

// Re-export for registry registration
pub(crate) use definition::register;
