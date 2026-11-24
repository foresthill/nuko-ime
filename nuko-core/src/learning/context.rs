//! 文脈つき候補学習 (context-aware frequency, #228 残件③ / A)。
//!
//! 「どうしようかな」の中で `しよう` を選んでも、別の文脈(仕様書など)で `仕様` を 24 回
//! 確定した Layer0 学習が**文脈を持たない**ために適用され、`どう仕様かな` になってしまう
//! (2026-11、森岡さん報告・`learning.json` の `context_hash` が全件 null と実測確認)。
//!
//! これを直すには「**直前の文節**が何か」で候補の選好を分ける軸が要る (森岡さん「多軸の
//! スコアがいるね」)。本ストアは `(読み, 表層, 直前文節の表層)` ごとに確定回数を churn-free
//! な JSON (`context_freq.json`) に溜める。`FrequencyEntry::context_hash` を使わず**独立
//! ストア**にするのは、王冠/boost (`FrequencyEntry`) の機構と絡めず加法的に載せるため
//! (既存の `ManualScoreStore` / `CorrectionStore` / `RetypeStore` と同じ分離方針)。
//!
//! スコアへの反映は `apply_segment_frequency` (conversion::engine) が行う:
//! segmented 変換で各文節の直前文節表層が文脈に一致する候補を、**その文節の他候補の最上位の
//! すぐ上** (= 確実に 1 位) へ持ち上げる。ただし訂正tier (`correction::CORRECTION_BOOST`)
//! **未満**にクランプするので、打ち直し訂正・強い手動スコアは文脈より上に残る (階層を保つ)。
//!
//! 外部送信しない (ローカルのみ)。心の構造でいう「個人的無意識」= 共有しない個人学習
//! (docs/LEARNING_ARCHITECTURE.md §1.5)。

use crate::error::{NukoError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::SystemTime;

/// 文脈一致候補を持ち上げるときの余裕 (同点負けを防ぐ)。王冠の `OVERTAKE_MARGIN` と同じ値。
pub const CONTEXT_MATCH_MARGIN: i32 = 100;

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 1 件の文脈つき頻度エントリ。`(reading, surface, prev)` で一意。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextFrequencyEntry {
    /// 読み (この文節のかな)
    pub reading: String,
    /// 表層 (選んだ候補)
    pub surface: String,
    /// 直前文節の表層 (= 文脈)
    pub prev: String,
    /// この文脈で選んだ回数
    pub count: u32,
    /// 最終使用時刻 (UNIX 秒)
    pub last_used: u64,
}

/// 文脈つき頻度ストア (`context_freq.json`)。churn-free (読み/表層/文脈で安定ソートして保存)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextFrequencyStore {
    entries: Vec<ContextFrequencyEntry>,
}

impl ContextFrequencyStore {
    /// 空ストア。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `context_freq.json` を読む。無ければ空。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("context_freq.json のパースに失敗: {e}")))
    }

    /// `context_freq.json` に保存する (読み→表層→文脈で昇順＝churn-free)。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let mut sorted = self.clone();
        sorted.entries.sort_by(|a, b| {
            (&a.reading, &a.surface, &a.prev).cmp(&(&b.reading, &b.surface, &b.prev))
        });
        let content = serde_json::to_string_pretty(&sorted)
            .map_err(|e| NukoError::Learning(format!("context_freq.json の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// `(reading, surface, prev)` の確定回数を 1 増やす (無ければ新規)。空キーは無視。
    /// 新しい回数を返す。
    pub fn record(&mut self, reading: &str, surface: &str, prev: &str) -> u32 {
        if reading.is_empty() || surface.is_empty() || prev.is_empty() {
            return 0;
        }
        let now = current_timestamp();
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.reading == reading && e.surface == surface && e.prev == prev)
        {
            e.count = e.count.saturating_add(1);
            e.last_used = now;
            e.count
        } else {
            self.entries.push(ContextFrequencyEntry {
                reading: reading.to_string(),
                surface: surface.to_string(),
                prev: prev.to_string(),
                count: 1,
                last_used: now,
            });
            1
        }
    }

    /// `(reading, surface, prev)` の確定回数 (無ければ 0)。
    #[must_use]
    pub fn count_for(&self, reading: &str, surface: &str, prev: &str) -> u32 {
        self.entries
            .iter()
            .find(|e| e.reading == reading && e.surface == surface && e.prev == prev)
            .map_or(0, |e| e.count)
    }

    /// 文脈 `prev` のもとで読み `reading` を最も多く確定した表層 (と回数)。無ければ `None`。
    /// 複数候補のうち count 最大を選ぶ (同数なら最後に見つかったものではなく最初＝安定)。
    #[must_use]
    pub fn best_surface(&self, reading: &str, prev: &str) -> Option<(&str, u32)> {
        self.entries
            .iter()
            .filter(|e| e.reading == reading && e.prev == prev)
            .max_by_key(|e| e.count)
            .map(|e| (e.surface.as_str(), e.count))
    }

    /// 調整が 1 件も無いか (空なら apply 側は完全に無変化)。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 件数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 全消去。除去件数を返す (「学習を忘れる」用)。
    pub fn forget_all(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }

    /// 読み (と任意で表層) の文脈学習を忘れる。除去件数を返す。
    /// `surface` が `None` ならその読みの全表層・全文脈、`Some(s)` ならその表層の全文脈。
    pub fn forget(&mut self, reading: &str, surface: Option<&str>) -> usize {
        let before = self.entries.len();
        // MSRV 1.75 には Option::is_none_or が無い (1.82 安定) ので match で書く。
        self.entries.retain(|e| {
            let surface_matches = match surface {
                None => true,
                Some(s) => s == e.surface,
            };
            !(e.reading == reading && surface_matches)
        });
        before - self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_count() {
        let mut s = ContextFrequencyStore::new();
        assert_eq!(s.record("しよう", "しよう", "どう"), 1);
        assert_eq!(s.record("しよう", "しよう", "どう"), 2);
        assert_eq!(s.count_for("しよう", "しよう", "どう"), 2);
        // 別文脈は別カウント。
        assert_eq!(s.count_for("しよう", "しよう", "この"), 0);
        // 別表層も別。
        assert_eq!(s.count_for("しよう", "仕様", "どう"), 0);
    }

    #[test]
    fn empty_keys_ignored() {
        let mut s = ContextFrequencyStore::new();
        assert_eq!(s.record("", "しよう", "どう"), 0);
        assert_eq!(s.record("しよう", "", "どう"), 0);
        assert_eq!(s.record("しよう", "しよう", ""), 0);
        assert!(s.is_empty());
    }

    #[test]
    fn best_surface_picks_highest_count() {
        let mut s = ContextFrequencyStore::new();
        s.record("しよう", "しよう", "どう");
        s.record("しよう", "しよう", "どう");
        s.record("しよう", "仕様", "どう"); // 1 回だけ
        let (surf, c) = s.best_surface("しよう", "どう").unwrap();
        assert_eq!(surf, "しよう");
        assert_eq!(c, 2);
        assert!(s.best_surface("しよう", "別").is_none());
    }

    #[test]
    fn forget_by_reading_and_surface() {
        let mut s = ContextFrequencyStore::new();
        s.record("しよう", "しよう", "どう");
        s.record("しよう", "仕様", "この");
        s.record("ほう", "方", "いい");
        assert_eq!(s.forget("しよう", Some("しよう")), 1);
        assert_eq!(s.count_for("しよう", "しよう", "どう"), 0);
        assert_eq!(s.count_for("しよう", "仕様", "この"), 1, "別表層は残る");
        assert_eq!(s.forget("しよう", None), 1, "読み全体");
        assert_eq!(s.len(), 1, "ほう/方 だけ残る");
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("nuko_ctx_{}", current_timestamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("context_freq.json");
        let mut s = ContextFrequencyStore::new();
        s.record("しよう", "しよう", "どう");
        s.record("しよう", "しよう", "どう");
        s.save(&path).unwrap();
        let loaded = ContextFrequencyStore::load(&path).unwrap();
        assert_eq!(loaded.count_for("しよう", "しよう", "どう"), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
