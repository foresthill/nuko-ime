//! # nuko-ui
//!
//! ぬこIMEのUIコンポーネント。
//!
//! このクレートは以下のUIを提供します：
//! - 候補ウィンドウ（変換候補の表示）
//! - 設定画面（IME設定のGUI）
//!
//! ## 使用例
//!
//! ```rust,ignore
//! use nuko_ui::candidate_window::CandidateWindow;
//!
//! let window = CandidateWindow::new();
//! window.show(&candidates)?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::unused_self,
    clippy::unnecessary_wraps,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::redundant_closure_for_method_calls,
    clippy::map_unwrap_or,
    clippy::items_after_statements,
    clippy::doc_markdown
)]

pub mod candidate_window;
pub mod settings;
pub mod theme;

/// よく使う型をまとめてインポートするためのプレリュード
pub mod prelude {
    pub use crate::candidate_window::CandidateWindow;
    pub use crate::settings::SettingsApp;
    pub use crate::theme::Theme;
}

/// クレートのバージョン
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
