//! 変換エンジンモジュール
//!
//! かな→漢字変換の中核機能を提供します。

pub mod backend;
mod candidate;
mod context;
mod engine;
mod segment;

pub use candidate::{Candidate, CandidateList, CandidateSource};
pub use context::ConversionContext;
pub use engine::ConversionEngine;
pub use segment::{Segment, SegmentedConversion};
