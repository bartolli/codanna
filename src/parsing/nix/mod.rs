pub mod audit;
mod behavior;
mod definition;
mod parser;
mod resolution;

pub use behavior::NixBehavior;
pub(crate) use definition::register;
pub use definition::NixLanguage;
pub use parser::NixParser;
pub use resolution::{NixInheritanceResolver, NixResolutionContext};
