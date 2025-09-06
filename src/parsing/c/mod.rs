//! C language parser implementation

pub mod behavior;
pub mod definition;
pub mod parser;

pub use behavior::CBehavior;
pub use definition::CLanguage;
pub use parser::CParser;

// Re-export for registry registration
pub(crate) use definition::register;