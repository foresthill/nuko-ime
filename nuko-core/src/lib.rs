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
// MVP 段階で過度に厳しい pedantic lint を緩和。
// 安定後に個別対応する場合はここから外す。
#![allow(
    clippy::missing_errors_doc,        // Result を返す関数全部に # Errors 必須は早すぎる
    clippy::must_use_candidate,        // #[must_use] 候補が多すぎてノイズ
    clippy::unused_self,               // API 拡張余地のため self を保持しているケースを許容
    clippy::unnecessary_wraps,         // 将来エラーを返す予定の Result<T> を保持
    clippy::cast_possible_truncation,  // 範囲限定の usize→i32 等
    clippy::cast_possible_wrap,        // 同上
    clippy::cast_sign_loss,            // 同上
    clippy::redundant_closure_for_method_calls, // 可読性優先で残すケースを許容
    clippy::items_after_statements,     // ヘルパー定義位置の柔軟性を保つ
    clippy::map_unwrap_or               // map(...).unwrap_or(...) も意図が明確なら許容
)]

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
