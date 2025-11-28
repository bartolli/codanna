//! JavaScript language parser implementation

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod jsconfig;
pub mod parser;
pub mod resolution;

pub use behavior::JavaScriptBehavior;
pub use definition::JavaScriptLanguage;
pub use jsconfig::{JsConfig, PathAliasResolver};
pub use parser::JavaScriptParser;
pub use resolution::{JavaScriptInheritanceResolver, JavaScriptResolutionContext};

// Re-export for registry registration
pub(crate) use definition::register;
