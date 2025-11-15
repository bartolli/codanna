//! 入出力処理モジュール
//!
//! CLIとツール統合のための入出力処理を提供します。
//!
//! # 主な機能
//!
//! - 統一された出力フォーマット（テキスト、JSON）
//! - 一貫性のあるエラーハンドリングと終了コード
//! - 将来: IDE統合のためのJSON-RPC 2.0サポート
//!
//! # 主要なコンポーネント
//!
//! - [`OutputFormat`]: 出力フォーマット（Text, JSON）
//! - [`OutputManager`]: 出力管理
//! - [`ExitCode`]: 終了コード
//! - [`UnifiedOutput`]: 統一出力スキーマ
//!
//! # 使用例
//!
//! ```
//! use codanna::io::{OutputFormat, OutputManager};
//!
//! let mut output = OutputManager::new(OutputFormat::Json);
//! // output.success("操作が成功しました");
//! ```

pub mod args;
pub mod exit_code;
pub mod format;
pub mod guidance;
pub mod guidance_engine;
pub mod input;
pub mod output;
pub mod parse;
pub mod schema;
pub mod status_line;
#[cfg(test)]
mod test;

pub use exit_code::ExitCode;
pub use format::{ErrorDetails, JsonResponse, OutputFormat, ResponseMeta};
pub use output::OutputManager;
pub use schema::{EntityType, OutputData, OutputStatus, UnifiedOutput, UnifiedOutputBuilder};
pub use status_line::{ProgressBar, ProgressBarOptions, ProgressBarStyle, Spinner, SpinnerOptions};
// Future: pub use input::{JsonRpcRequest, JsonRpcResponse};
