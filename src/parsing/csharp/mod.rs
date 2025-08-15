//! C# language support module

mod behavior;
mod definition;
mod parser;

pub use behavior::CSharpBehavior;
pub use parser::{CSharpParseError, CSharpParser};

pub(crate) use definition::register;