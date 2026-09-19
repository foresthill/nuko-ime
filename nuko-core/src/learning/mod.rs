//! 学習モジュール
//!
//! ユーザーの入力パターンを学習し、変換精度を向上させます。

mod frequency;
mod manager;
pub mod observation;

pub use manager::LearningManager;
pub use observation::{ObservationEvent, ObservationLog};
