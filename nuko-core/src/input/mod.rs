//! 入力処理モジュール
//!
//! ローマ字からかなへの変換、かな処理を提供します。

mod kana;
mod romaji;

pub use kana::{KanaType, to_halfwidth_katakana, to_hiragana, to_katakana};
pub use romaji::RomajiConverter;
