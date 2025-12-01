//! Swift language support for Codanna
//!
//! Provides parsing, behavior, and resolution for Swift source files.

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::SwiftBehavior;
pub use definition::SwiftLanguage;
pub use parser::SwiftParser;
pub use resolution::{SwiftInheritanceResolver, SwiftResolutionContext};

pub(crate) use definition::register;
