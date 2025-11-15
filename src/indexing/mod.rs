//! インデックス作成モジュール
//!
//! コードベースのインデックス化、ファイル監視、進捗追跡などの機能を提供します。
//!
//! # 主要なコンポーネント
//!
//! - [`SimpleIndexer`]: メインのインデクサー実装
//! - [`FileWalker`]: ファイルシステムの探索
//! - [`FileSystemWatcher`]: ファイル変更の監視
//! - [`IndexTransaction`]: トランザクション管理
//! - [`ConfigFileWatcher`]: 設定ファイルの監視
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::indexing::SimpleIndexer;
//! use codanna::Settings;
//!
//! let settings = Settings::default();
//! let indexer = SimpleIndexer::new(&settings);
//! ```

pub mod config_watcher;
pub mod file_info;
pub mod fs_watcher;
pub mod progress;
pub mod simple;
pub mod transaction;
pub mod walker;

#[cfg(test)]
pub mod import_resolution_proof;

pub use config_watcher::ConfigFileWatcher;
pub use file_info::{FileInfo, calculate_hash, get_utc_timestamp};
pub use fs_watcher::{FileSystemWatcher, WatchError};
pub use progress::IndexStats;
pub use simple::SimpleIndexer;
pub use transaction::{FileTransaction, IndexTransaction};
pub use walker::FileWalker;
