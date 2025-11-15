//! リッチターミナルディスプレイユーティリティ
//!
//! 強化されたCLI出力のためのスタイル付きテーブル、プログレスバー、
//! フォーマット済み出力を提供します。プロフェッショナルなコマンドライン体験を実現します。
//!
//! # 主要なコンポーネント
//!
//! - スタイル付きテーブル
//! - プログレスバー
//! - テーマサポート
//! - ヘルプテキストフォーマット
//!
//! # 使用例
//!
//! ```no_run
//! // use codanna::display::theme::Theme;
//! // let theme = Theme::default();
//! // println!("{}", theme.success("成功しました！"));
//! ```

pub mod help;
pub mod progress;
pub mod tables;
pub mod theme;

pub use help::{create_help_text, format_command_description, format_help_section};
pub use progress::{ProgressTracker, create_progress_bar, create_spinner};
pub use tables::{TableBuilder, create_benchmark_table, create_summary_table};
pub use theme::{THEME, Theme};
