//! Clojure language parser implementation

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::ClojureBehavior;
pub use definition::ClojureLanguage;
pub use parser::ClojureParser;
pub use resolution::ClojureResolutionContext;

// Re-export for registry registration
pub(crate) use definition::register;
