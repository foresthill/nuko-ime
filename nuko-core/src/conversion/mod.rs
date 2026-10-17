//! 変換エンジンモジュール
//!
//! かな→漢字変換の中核機能を提供します。

pub mod backend;
mod candidate;
mod context;
mod engine;
pub mod extend_clause;
pub mod kyujitai;
mod segment;

pub use candidate::{Candidate, CandidateList, CandidateSource};
pub use context::ConversionContext;
pub use engine::{nn_alternate_readings, ConversionEngine};
pub use kyujitai::{kyujitai_variant, to_kyujitai};
pub use segment::{Segment, SegmentedConversion};
