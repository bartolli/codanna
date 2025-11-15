//! パーシングモジュール
//!
//! 複数のプログラミング言語のパーサーと言語固有の動作を提供します。
//!
//! # サポート言語
//!
//! - Rust
//! - Python
//! - TypeScript / JavaScript
//! - Go
//! - Kotlin
//! - C / C++
//! - C#
//! - PHP
//! - GDScript
//!
//! # 主要なコンポーネント
//!
//! - [`LanguageParser`]: パーサートレイト
//! - [`LanguageBehavior`]: 言語固有の動作
//! - [`ParserFactory`]: パーサーのファクトリ
//! - [`LanguageRegistry`]: 言語登録システム
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::parsing::{RustParser, LanguageParser};
//!
//! let parser = RustParser;
//! let source = "fn main() {}";
//! // let symbols = parser.parse(source, file_id);
//! ```

pub mod behavior_state;
pub mod c;
pub mod context;
pub mod cpp;
pub mod csharp;
pub mod factory;
pub mod gdscript;
pub mod go;
pub mod import;
pub mod kotlin;
pub mod language;
pub mod language_behavior;
pub mod method_call;
pub mod parser;
pub mod php;
pub mod python;
pub mod registry;
pub mod resolution;
pub mod rust;
pub mod typescript;

pub use c::{CBehavior, CParser};
pub use context::{ParserContext, ScopeType};
pub use cpp::{CppBehavior, CppParser};
pub use csharp::{CSharpBehavior, CSharpParser};
pub use factory::{ParserFactory, ParserWithBehavior};
pub use gdscript::{GdscriptBehavior, GdscriptParser};
pub use go::{GoBehavior, GoParser};
pub use import::Import;
pub use kotlin::{KotlinBehavior, KotlinParser};
pub use language::Language;
pub use language_behavior::{LanguageBehavior, LanguageMetadata};
pub use method_call::MethodCall;
pub use parser::{
    HandledNode, LanguageParser, NodeTracker, NodeTrackingState, safe_substring_window,
    safe_truncate_str, truncate_for_display,
};
pub use php::{PhpBehavior, PhpParser};
pub use python::{PythonBehavior, PythonParser};
pub use registry::{LanguageDefinition, LanguageId, LanguageRegistry, RegistryError, get_registry};
pub use resolution::{
    GenericInheritanceResolver, GenericResolutionContext, InheritanceResolver, ResolutionScope,
    ScopeLevel,
};
pub use rust::{RustBehavior, RustParser};
pub use typescript::{TypeScriptBehavior, TypeScriptParser};
