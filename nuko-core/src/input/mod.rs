//! 入力処理モジュール
//!
//! ローマ字からかなへの変換、かな処理を提供します。

mod kana;
mod mozc_table;
mod romaji;

pub use kana::{to_halfwidth_katakana, to_hiragana, to_katakana, KanaType};
pub use romaji::RomajiConverter;
