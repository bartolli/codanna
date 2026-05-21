//! Svelte language parser implementation

pub mod behavior;
pub mod definition;
pub mod parser;

pub use behavior::SvelteBehavior;
pub use definition::SvelteLanguage;
pub(crate) use definition::register;
pub use parser::SvelteParser;
