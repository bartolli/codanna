//! C++ language parser implementation

pub mod behavior;
pub mod definition;
pub mod parser;

pub use behavior::CppBehavior;
pub use definition::CppLanguage;
pub use parser::CppParser;

// Re-export for registry registration
pub(crate) use definition::register;