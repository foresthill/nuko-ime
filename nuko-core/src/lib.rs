//! # nuko-core
//!
//! ぬこIMEのコアエンジンライブラリ。
//!
//! このクレートは以下の機能を提供します：
//! - ローマ字→かな変換
//! - かな→漢字変換（形態素解析ベース）
//! - ユーザー辞書管理
//! - 学習機能（使用頻度・文脈）
//!
//! ## 使用例
//!
//! ```rust,ignore
//! use nuko_core::prelude::*;
//!
//! let engine = ConversionEngine::new()?;
//! let candidates = engine.convert("にほんご")?;
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]

pub mod conversion;
pub mod dictionary;
pub mod error;
pub mod input;
pub mod learning;

/// よく使う型をまとめてインポートするためのプレリュード
pub mod prelude {
    pub use crate::conversion::{Candidate, ConversionEngine};
    pub use crate::dictionary::{DictionaryManager, UserDictionary};
    pub use crate::error::{NukoError, Result};
    pub use crate::input::RomajiConverter;
    pub use crate::learning::LearningManager;
}

/// クレートのバージョン
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// クレート名
pub const NAME: &str = env!("CARGO_PKG_NAME");
