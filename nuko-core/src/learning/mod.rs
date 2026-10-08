//! 学習モジュール
//!
//! ユーザーの入力パターンを学習し、変換精度を向上させます。

pub mod correction;
pub mod dreaming;
mod frequency;
mod manager;
pub mod observation;
pub mod segmentation;

pub use correction::{extract_corrections, CorrectionStore, Preference};
pub use dreaming::{
    build_prompt, digest_observations, merge_proposals, parse_proposal, DreamProposal,
    DreamProvider, MockProvider, ObservationDigest, ProposedPreference,
};
pub use manager::LearningManager;
pub use observation::{ObservationEvent, ObservationLog};
pub use segmentation::{ranges_from_segment_readings, SegmentationEntry, SegmentationStore};
