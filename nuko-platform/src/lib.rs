//! # nuko-platform
//!
//! ぬこIMEのOS統合層。
//!
//! このクレートは各プラットフォーム固有のIME APIとの統合を提供します：
//! - Windows: Text Services Framework (`TSF`)
//! - macOS: Input Method Kit
//! - Linux: `IBus` / `Fcitx5`
//!
//! ## 使用例
//!
//! ```rust,ignore
//! use nuko_platform::prelude::*;
//!
//! let ime = NukoIME::new()?;
//! ime.register()?;
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
    clippy::items_after_statements
)]

pub mod config;
pub mod error;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

/// よく使う型をまとめてインポートするためのプレリュード
pub mod prelude {
    pub use crate::config::Config;
    pub use crate::error::{PlatformError, Result};

    #[cfg(target_os = "windows")]
    pub use crate::windows::NukoIME;

    #[cfg(target_os = "macos")]
    pub use crate::macos::NukoIME;

    #[cfg(target_os = "linux")]
    pub use crate::linux::NukoIME;
}

/// クレートのバージョン
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
