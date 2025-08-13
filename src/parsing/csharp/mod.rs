//! C# language support module

mod behavior;
mod parser;

pub use behavior::CSharpBehavior;
pub use parser::{CSharpParseError, CSharpParser};