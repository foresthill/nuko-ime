//! 学習モジュール
//!
//! ユーザーの入力パターンを学習し、変換精度を向上させます。

pub mod context;
pub mod correction;
pub mod date_format;
pub mod dream_history;
pub mod dreaming;
mod frequency;
mod manager;
pub mod manual_score;
pub mod observation;
pub mod phrase;
pub mod retype;
pub mod segmentation;

pub use context::{ContextFrequencyEntry, ContextFrequencyStore, CONTEXT_MATCH_MARGIN};
pub use correction::{extract_corrections, CorrectionStore, Preference};
pub use date_format::{DateFormatEntry, DateFormatStore};
pub use dreaming::{
    build_prompt, digest_observations, merge_proposals, parse_proposal, DreamProposal,
    DreamProvider, MockProvider, ObservationDigest, ProposedPreference,
};
pub use frequency::LearningStrength;
pub use manager::LearningManager;
pub use manual_score::{
    ManualScoreEntry, ManualScoreStore, MANUAL_SCORE_MAX, MANUAL_SCORE_STEP,
    MANUAL_SCORE_STEP_HUGE, MANUAL_SCORE_STEP_LARGE, MANUAL_SCORE_STEP_MEDIUM,
    MANUAL_SCORE_STEP_SMALL,
};
pub use observation::{
    filter_out_reading, prune_for_cap, ObservationEvent, ObservationLog, OBS_CAP_HIGH_WATERMARK,
    OBS_CAP_KEEP_RECENT_COMMITS,
};
pub use phrase::{should_record_phrase, PhraseEntry, PhraseStore, MIN_PHRASE_READING_CHARS};
pub use retype::{RetypeEntry, RetypeStore};
pub use segmentation::{ranges_from_segment_readings, SegmentationEntry, SegmentationStore};
