//! Language-specific project resolution providers
//!
//! Each language implements the ProjectResolutionProvider trait to handle
//! project configuration files and path resolution rules.

pub mod java;
pub mod javascript;
pub mod typescript;

pub use java::JavaProvider;
pub use javascript::JavaScriptProvider;
pub use typescript::TypeScriptProvider;
