//! Java language parser implementation

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::JavaBehavior;
pub use definition::JavaLanguage;
pub use parser::JavaParser;
pub use resolution::{JavaInheritanceResolver, JavaResolutionContext};

// Re-export for registry registration
pub(crate) use definition::register;
