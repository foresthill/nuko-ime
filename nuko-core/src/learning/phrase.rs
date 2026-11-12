//! フレーズ予測ストア (`phrases.json`) — #187
//!
//! ユーザーが**文全体を確定**したとき「読み全体 → 表層全体」を覚え、次回から
//! **前方一致**で候補に出す。= 定型句を短い前置きから予測する。
//! 例: 一度 `ご確認いただけますと幸いです` を確定 → 次回 `ごかくにん`
//! (末尾ん = 単一 n で打てる) で候補に出す。ローマ字の「ん＋母音」境界を、
//! 全文を打ち切る前に候補選択で回避する狙い。
//!
//! Layer0 頻度学習 ([`super::LearningManager`], 文節単位・完全一致) とは**別軸**。
//! こちらは**読み全体・前方一致**。個人データ・非共有。
//!
//! 設計: `docs/spikes/2026-11-12-phrase-prediction.md`

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{NukoError, Result};

/// 記録・予測の最小読み長 (かな**文字数**)。短すぎる語は Layer0 に任せ、
/// フレーズ (= 文相当) だけを対象にするための下限。
pub const MIN_PHRASE_READING_CHARS: usize = 6;

/// この確定をフレーズとして記録すべきか (#187 §5 のゲート、純粋関数)。
///
/// 条件 (すべて満たすとき `true`):
/// - **複数文節** (`segment_count >= 2`)。単一文節・単語は Layer 0 頻度学習に任せる。
/// - 読み・表層がどちらも非空。
/// - 読み ≠ 表層 (かな素通りはフレーズにしない)。
/// - 読みが [`MIN_PHRASE_READING_CHARS`] 文字以上 (= 文相当)。
///
/// datetime 等の**一過性**表層の除外は呼び出し側の責務 (controller 側で既に除外済み)。
#[must_use]
pub fn should_record_phrase(reading: &str, surface: &str, segment_count: usize) -> bool {
    segment_count >= 2
        && !reading.is_empty()
        && !surface.is_empty()
        && reading != surface
        && reading.chars().count() >= MIN_PHRASE_READING_CHARS
}

/// 1 件のフレーズ学習。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhraseEntry {
    /// 読み全体 (かな)。前方一致の検索キー。
    pub reading: String,
    /// 表層全体 (確定した文)。
    pub surface: String,
    /// 確定回数。
    pub count: u32,
    /// 最終使用時刻 (UNIX 秒)。recency 用 (v1 は保存のみ、スコアは count 主体)。
    #[serde(default)]
    pub last_used: u64,
}

/// フレーズ予測ストア (`phrases.json`、人間可読・個人・非共有)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhraseStore {
    /// 学習したフレーズ一覧。
    entries: Vec<PhraseEntry>,
}

impl PhraseStore {
    /// 空のストアを作る。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `phrases.json` を読む。ファイルが無ければ空。
    ///
    /// 形式は `learning.json` と同じ「エントリ配列」(トップレベル JSON 配列)。
    ///
    /// # エラー
    /// ファイルはあるが JSON パースに失敗した場合。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<PhraseEntry> = serde_json::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("phrases.json のパースに失敗: {e}")))?;
        Ok(Self { entries })
    }

    /// `phrases.json` に保存する (読み昇順で churn-free)。
    ///
    /// # エラー
    /// 直列化・書き込みに失敗した場合。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| a.reading.cmp(&b.reading).then(a.surface.cmp(&b.surface)));
        let content = serde_json::to_string_pretty(&sorted)
            .map_err(|e| NukoError::Learning(format!("phrases.json の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// フレーズを 1 件記録する (同じ `(reading, surface)` は count++)。
    ///
    /// 以下は**記録しない** (フレーズと見なさない安全弁) → `false`:
    /// - 読み・表層が空
    /// - 読みが [`MIN_PHRASE_READING_CHARS`] 文字未満 (= 文相当でない短い語)
    ///
    /// 記録・更新したら `true`。読み全体・表層全体の**完全一致のみ**を数える
    /// (接尾流用・分割流用はしない。過学習の轍を踏まない。PR #106)。
    pub fn record(&mut self, reading: &str, surface: &str) -> bool {
        if reading.is_empty() || surface.is_empty() {
            return false;
        }
        if reading.chars().count() < MIN_PHRASE_READING_CHARS {
            return false;
        }
        let now = current_timestamp();
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.reading == reading && e.surface == surface)
        {
            e.count = e.count.saturating_add(1);
            e.last_used = now;
        } else {
            self.entries.push(PhraseEntry {
                reading: reading.to_string(),
                surface: surface.to_string(),
                count: 1,
                last_used: now,
            });
        }
        true
    }

    /// `prefix` に**前方一致**するフレーズを、count 降順で最大 `max` 件返す。
    ///
    /// `prefix` 空 / `max == 0` のときは空。count 同値は recency (last_used 降順)、
    /// さらに読み昇順で決定的に並べる (churn-free)。
    #[must_use]
    pub fn predict(&self, prefix: &str, max: usize) -> Vec<PhraseEntry> {
        if prefix.is_empty() || max == 0 {
            return Vec::new();
        }
        let mut hits: Vec<PhraseEntry> = self
            .entries
            .iter()
            .filter(|e| e.reading.starts_with(prefix))
            .cloned()
            .collect();
        hits.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then(b.last_used.cmp(&a.last_used))
                .then(a.reading.cmp(&b.reading))
                .then(a.surface.cmp(&b.surface))
        });
        hits.truncate(max);
        hits
    }

    /// 件数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空か。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 現在の UNIX タイムスタンプ (秒)。
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOKAKUNIN: &str = "ごかくにんいただけますとさいわいです";
    const GOKAKUNIN_SURFACE: &str = "ご確認いただけますと幸いです";

    #[test]
    fn record_and_predict_by_short_prefix() {
        let mut store = PhraseStore::new();
        assert!(store.record(GOKAKUNIN, GOKAKUNIN_SURFACE));
        // 末尾ん = 単一 n で打てる短い前置きで引ける (= 境界回避の肝)。
        let hits = store.predict("ごかくにん", 9);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].surface, GOKAKUNIN_SURFACE);
    }

    #[test]
    fn predict_non_matching_prefix_is_empty() {
        let mut store = PhraseStore::new();
        store.record(GOKAKUNIN, GOKAKUNIN_SURFACE);
        assert!(store.predict("おせわに", 9).is_empty());
        assert!(store.predict("", 9).is_empty(), "空 prefix は空");
        assert!(store.predict("ごかくにん", 0).is_empty(), "max=0 は空");
    }

    #[test]
    fn record_rejects_too_short_reading() {
        let mut store = PhraseStore::new();
        // 5 文字 (< MIN_PHRASE_READING_CHARS=6) は記録しない。
        assert!(!store.record("にほんご", "日本語")); // 4 文字
        assert!(!store.record("", "x"));
        assert!(!store.record("あいうえおか", "")); // 表層空
        assert!(store.is_empty());
        // 6 文字ちょうどは記録する。
        assert!(store.record("あいうえおか", "阿為卯絵尾加"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn count_increments_on_same_phrase() {
        let mut store = PhraseStore::new();
        store.record(GOKAKUNIN, GOKAKUNIN_SURFACE);
        store.record(GOKAKUNIN, GOKAKUNIN_SURFACE);
        assert_eq!(store.len(), 1, "同一フレーズは重複しない");
        assert_eq!(store.predict("ごかくにん", 9)[0].count, 2);
    }

    #[test]
    fn predict_orders_by_count_desc() {
        let mut store = PhraseStore::new();
        // 同じ prefix「おせわに」に 2 つのフレーズ。count で順位が決まる。
        store.record("おせわになっております", "お世話になっております");
        store.record("おせわになりました", "お世話になりました");
        store.record("おせわになりました", "お世話になりました"); // count=2
        let hits = store.predict("おせわに", 9);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].surface, "お世話になりました", "count 高い方が先頭");
        assert_eq!(hits[1].surface, "お世話になっております");
    }

    #[test]
    fn predict_respects_max() {
        let mut store = PhraseStore::new();
        store.record("おせわになっております", "お世話になっております");
        store.record("おせわになりました", "お世話になりました");
        assert_eq!(store.predict("おせわに", 1).len(), 1);
    }

    #[test]
    fn save_load_roundtrip() {
        let mut store = PhraseStore::new();
        store.record(GOKAKUNIN, GOKAKUNIN_SURFACE);
        store.record("おせわになっております", "お世話になっております");
        let path =
            std::env::temp_dir().join(format!("nuko-phrase-test-{}.json", std::process::id()));
        store.save(&path).unwrap();
        let loaded = PhraseStore::load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(
            loaded.predict("ごかくにん", 9)[0].surface,
            GOKAKUNIN_SURFACE
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let path = std::env::temp_dir().join("nuko-phrase-does-not-exist-xyz.json");
        let _ = std::fs::remove_file(&path);
        let store = PhraseStore::load(&path).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn should_record_phrase_gate() {
        // 複数文節・長い読み・読み≠表層 → 記録する。
        assert!(should_record_phrase(GOKAKUNIN, GOKAKUNIN_SURFACE, 5));
        // 単一文節は記録しない (Layer 0 に任せる)。
        assert!(!should_record_phrase(GOKAKUNIN, GOKAKUNIN_SURFACE, 1));
        assert!(!should_record_phrase(GOKAKUNIN, GOKAKUNIN_SURFACE, 0));
        // 読み == 表層 (かな素通り) は記録しない。
        assert!(!should_record_phrase("あいうえおか", "あいうえおか", 3));
        // 短い読み (< MIN) は記録しない。
        assert!(!should_record_phrase("にほんご", "日本語", 3));
        // 空は記録しない。
        assert!(!should_record_phrase("", "x", 3));
        assert!(!should_record_phrase("あいうえおか", "", 3));
    }
}
