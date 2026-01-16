//! Language-specific project resolution providers
//!
//! Each language implements the ProjectResolutionProvider trait to handle
//! project configuration files and path resolution rules.

pub mod go;
pub mod java;
pub mod javascript;
pub mod kotlin;
pub mod php;
pub mod python;
pub mod swift;
pub mod typescript;

pub use go::GoProvider;
pub use java::JavaProvider;
pub use javascript::JavaScriptProvider;
pub use kotlin::KotlinProvider;
pub use php::PhpProvider;
pub use python::PythonProvider;
pub use swift::SwiftProvider;
pub use typescript::TypeScriptProvider;
