//! 手動スコア調整ストア (`manual_score.json`) — **常時ON**。
//!
//! ユーザーが候補を見ながら `⌥→`(上げる)/`⌥←`(下げる) で**手動**に順位を微調整する (#228)。
//! 自動学習 (頻度/訂正/王冠) とは別系統の「明示的な手のかけ方」。読み×表層ごとに符号付きバイアスを
//! 貯め、次回以降の変換で候補スコアへ加点/減点する。
//!
//! phrase / retype / date_format と同じく **ローカル・非共有・churn-free** (読み×表層昇順で決定的保存)。
//! 外部送信しない。1 回の `⌥` で [`MANUAL_SCORE_STEP`] 動かし、累積は ±[`MANUAL_SCORE_MAX`] にクランプ
//! する (暴走防止 = 王冠モデル [[nuko-ime-crown-learning-mode]] の教訓)。クランプにより
//! 訂正(correction)/教える(crown) の tier (≥300k/200k) は手動微調整では越えられない ⇒ 「一気に1位」は
//! 教える(⇧⏎)、「じわっと調整」は ⌥←→、と役割が分かれる。

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{NukoError, Result};

/// `⌥` 1 回で動かすスコア量。候補帯 (libakaza ~数百〜千差) の中で 1〜数回で並べ替わる程度。
/// 「一気に1位」ではなく微調整なので中程度。controller から参照する。
pub const MANUAL_SCORE_STEP: i32 = 2_000;

/// 手動バイアスの累積上限 (絶対値)。これ以上は増えない (暴走防止)。
/// 学習 (200k) / 訂正 (300k) 帯より十分小さく、手動微調整が自動学習の決定を越えないようにする。
const MANUAL_SCORE_MAX: i32 = 100_000;

/// 手動スコア調整 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualScoreEntry {
    /// 読み (ひらがな)。検索キー。
    pub reading: String,
    /// 表層。検索キー。
    pub surface: String,
    /// 累積バイアス (正=上げ、負=下げ)。±[`MANUAL_SCORE_MAX`] にクランプ。
    pub bias: i32,
    /// 最終更新時刻 (UNIX 秒)。recency 用 (v1 は保存のみ)。
    #[serde(default)]
    pub last_used: u64,
}

/// 手動スコア調整ストア (`manual_score.json`、人間可読・個人・非共有)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualScoreStore {
    entries: Vec<ManualScoreEntry>,
}

impl ManualScoreStore {
    /// 空のストアを作る。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `manual_score.json` を読む。無ければ空。
    ///
    /// # エラー
    /// ファイルはあるが JSON パースに失敗した場合。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<ManualScoreEntry> = serde_json::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("manual_score.json のパースに失敗: {e}")))?;
        Ok(Self { entries })
    }

    /// `manual_score.json` に保存する (読み×表層 昇順で churn-free)。
    ///
    /// # エラー
    /// 直列化・書き込みに失敗した場合。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| (&a.reading, &a.surface).cmp(&(&b.reading, &b.surface)));
        let content = serde_json::to_string_pretty(&sorted)
            .map_err(|e| NukoError::Learning(format!("manual_score.json の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 読み×表層のバイアスを `delta` 動かす (累積)。±[`MANUAL_SCORE_MAX`] にクランプ。
    /// 新しいバイアスを返す。空の読み/表層は無視して現状維持 (0) を返す。
    pub fn nudge(&mut self, reading: &str, surface: &str, delta: i32) -> i32 {
        if reading.is_empty() || surface.is_empty() {
            return 0;
        }
        let now = current_timestamp();
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.reading == reading && e.surface == surface)
        {
            e.bias = e
                .bias
                .saturating_add(delta)
                .clamp(-MANUAL_SCORE_MAX, MANUAL_SCORE_MAX);
            e.last_used = now;
            e.bias
        } else {
            let bias = delta.clamp(-MANUAL_SCORE_MAX, MANUAL_SCORE_MAX);
            self.entries.push(ManualScoreEntry {
                reading: reading.to_string(),
                surface: surface.to_string(),
                bias,
                last_used: now,
            });
            bias
        }
    }

    /// 読み×表層のバイアス (無ければ 0)。候補スコアへの加点/減点に使う。
    #[must_use]
    pub fn bias(&self, reading: &str, surface: &str) -> i32 {
        self.entries
            .iter()
            .find(|e| e.reading == reading && e.surface == surface)
            .map_or(0, |e| e.bias)
    }

    /// 調整が 1 件も無いか (空なら convert 側は完全に無変化)。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 件数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 全調整を忘れる。除去件数を返す (「学習を忘れる」用)。
    pub fn forget_all(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
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

    #[test]
    fn nudge_accumulates_and_is_queryable() {
        let mut s = ManualScoreStore::new();
        assert_eq!(
            s.nudge("にんい", "任意", MANUAL_SCORE_STEP),
            MANUAL_SCORE_STEP
        );
        assert_eq!(
            s.nudge("にんい", "任意", MANUAL_SCORE_STEP),
            MANUAL_SCORE_STEP * 2
        );
        assert_eq!(s.bias("にんい", "任意"), MANUAL_SCORE_STEP * 2);
        assert_eq!(s.bias("にんい", "忍意"), 0, "未調整は 0");
    }

    #[test]
    fn nudge_down_is_negative() {
        let mut s = ManualScoreStore::new();
        assert_eq!(
            s.nudge("ほう", "方", -MANUAL_SCORE_STEP),
            -MANUAL_SCORE_STEP
        );
        assert!(s.bias("ほう", "方") < 0);
    }

    #[test]
    fn bias_is_clamped_to_max() {
        let mut s = ManualScoreStore::new();
        for _ in 0..1000 {
            s.nudge("あ", "亜", MANUAL_SCORE_STEP);
        }
        assert_eq!(s.bias("あ", "亜"), MANUAL_SCORE_MAX, "上限でクランプ");
        for _ in 0..2000 {
            s.nudge("あ", "亜", -MANUAL_SCORE_STEP);
        }
        assert_eq!(s.bias("あ", "亜"), -MANUAL_SCORE_MAX, "下限でクランプ");
    }

    #[test]
    fn empty_keys_are_ignored() {
        let mut s = ManualScoreStore::new();
        assert_eq!(s.nudge("", "任意", 100), 0);
        assert_eq!(s.nudge("にんい", "", 100), 0);
        assert!(s.is_empty());
    }

    #[test]
    fn forget_all_clears() {
        let mut s = ManualScoreStore::new();
        s.nudge("にんい", "任意", MANUAL_SCORE_STEP);
        s.nudge("ほう", "方", MANUAL_SCORE_STEP);
        assert_eq!(s.forget_all(), 2);
        assert!(s.is_empty());
        assert_eq!(s.bias("にんい", "任意"), 0);
    }

    #[test]
    fn save_load_roundtrip() {
        let mut s = ManualScoreStore::new();
        s.nudge("にんい", "任意", MANUAL_SCORE_STEP);
        s.nudge("にんい", "任意", MANUAL_SCORE_STEP);
        s.nudge("ほう", "方", -MANUAL_SCORE_STEP);
        let path =
            std::env::temp_dir().join(format!("nuko-manualscore-test-{}.json", std::process::id()));
        s.save(&path).unwrap();
        let loaded = ManualScoreStore::load(&path).unwrap();
        assert_eq!(loaded.bias("にんい", "任意"), MANUAL_SCORE_STEP * 2);
        assert_eq!(loaded.bias("ほう", "方"), -MANUAL_SCORE_STEP);
        let _ = std::fs::remove_file(&path);
    }
}
