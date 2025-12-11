//! 変換エンジンモジュール
//!
//! かな→漢字変換の中核機能を提供します。

mod candidate;
mod context;
mod engine;

pub use candidate::{Candidate, CandidateList};
pub use context::ConversionContext;
pub use engine::ConversionEngine;
