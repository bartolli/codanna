pub mod audit;
mod behavior;
mod definition;
mod parser;
mod resolution;

pub use behavior::NixBehavior;
pub use definition::NixLanguage;
pub(crate) use definition::register;
pub use parser::NixParser;
pub use resolution::{NixInheritanceResolver, NixResolutionContext};
