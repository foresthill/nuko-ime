//! 手動スコア調整ストア (`manual_score.json`) — **常時ON**。
//!
//! ユーザーが候補を見ながら `⌥→`(上げる)/`⌥←`(下げる) で**手動**に順位を微調整する (#228)。
//! 自動学習 (頻度/訂正/王冠) とは別系統の「明示的な手のかけ方」。読み×表層ごとに符号付きバイアスを
//! 貯め、次回以降の変換で候補スコアへ加点/減点する。
//!
//! phrase / retype / date_format と同じく **ローカル・非共有・churn-free** (読み×表層昇順で決定的保存)。
//! 外部送信しない。1 回の `⌥` で [`MANUAL_SCORE_STEP`] 動かし、累積は ±[`MANUAL_SCORE_MAX`] にクランプする。
//!
//! # 「手動は学習より強い」設計 (2026-11-23 方針転換)
//!
//! 当初は「手動は学習を越えない微調整」にしていた (±100k クランプ) が、森岡さんが実機で
//! テストして **「弱すぎて使えない」** と結論: レガシーの暴走学習 (旧「累積」モードで膨らんだ
//! `厚意 478010` / `様 500000` 等) や、かな fallback (`よう -780000`) との桁差を ±100k では
//! 越えられず「一生変えられない」。**明示的な手のかけ方 (⌥←→) は、推論でしかない自動学習より
//! 優先されるべき** というユーザーの意思に合わせ、クランプを [`MANUAL_SCORE_MAX`] = ±2,000,000
//! まで引き上げた (学習/訂正/王冠の全帯を越えられる = 下げれば確実に沈み、上げれば確実に1位)。
//!
//! 刻み ([`MANUAL_SCORE_STEP_SMALL`]〜[`MANUAL_SCORE_STEP_HUGE`]) は runtime で選択でき、矢印の
//! **キーリピート**で押しっぱなしにすれば蓄積して一気に動く (「ゲームみたいにバーっと」)。
//! 「下げるために確定」は不要 = ⌥← のライブ下げだけで沈む。churn-free は維持
//! (読み×表層昇順で決定的保存)。関連: [[nuko-ime-crown-learning-mode]] / [[nuko-ime-manual-score-feature]]。

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{NukoError, Result};

/// `⌥` 1 回で動かすスコア量の既定値 (= [`MANUAL_SCORE_STEP_LARGE`])。runtime で下記 4 段階に切替可。
/// controller は state の設定を読む (無ければこの既定)。
pub const MANUAL_SCORE_STEP: i32 = MANUAL_SCORE_STEP_LARGE;

/// 刻み「小」: 微調整。libakaza 帯 (~数百〜千差) の中で数回で並べ替わる。
pub const MANUAL_SCORE_STEP_SMALL: i32 = 2_000;
/// 刻み「中」: 1 回で libakaza 帯をまたぐ。
pub const MANUAL_SCORE_STEP_MEDIUM: i32 = 20_000;
/// 刻み「大」(既定): 数回で学習帯 (200k) をまたぐ。押しっぱなしで一気に。
pub const MANUAL_SCORE_STEP_LARGE: i32 = 100_000;
/// 刻み「特大」: 1〜数回で王冠/訂正/レガシー暴走学習 (〜500k) も越える。
pub const MANUAL_SCORE_STEP_HUGE: i32 = 500_000;

/// 手動バイアスの累積上限 (絶対値)。**明示操作は自動学習より強い**方針 (上記モジュール doc) に
/// 従い、学習 (200k) / 訂正 (300k) / 王冠・レガシー暴走 (〜500k) / かな fallback (〜-800k) の
/// 全帯を越えられる大きさにする。下げれば確実に最下位へ、上げれば確実に1位へ到達できる。
pub const MANUAL_SCORE_MAX: i32 = 2_000_000;

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

    /// 読み (と任意で表層) の調整を忘れる。除去件数を返す。
    ///
    /// `surface` が `None` ならその読みの全表層、`Some(s)` ならその 1 件だけ。
    /// 観察ログ `filter_out_reading` / `RetypeStore::forget` と対で「忘れる」操作から呼ぶ。
    pub fn forget(&mut self, reading: &str, surface: Option<&str>) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            // MSRV 1.75 のため is_none_or は使わない (1.82 安定化)。
            let surface_hit = match surface {
                Some(s) => e.surface == s,
                None => true,
            };
            let hit = e.reading == reading && surface_hit;
            !hit
        });
        before - self.entries.len()
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
    fn forget_removes_by_reading_and_optional_surface() {
        let mut s = ManualScoreStore::new();
        s.nudge("ほう", "方", MANUAL_SCORE_STEP);
        s.nudge("ほう", "法", -MANUAL_SCORE_STEP);
        s.nudge("よう", "用", MANUAL_SCORE_STEP);
        // 表層指定 = その 1 件だけ。
        assert_eq!(s.forget("ほう", Some("方")), 1);
        assert_eq!(s.bias("ほう", "方"), 0);
        assert_eq!(s.bias("ほう", "法"), -MANUAL_SCORE_STEP, "他表層は残る");
        // 表層 None = その読みの残り全部。
        assert_eq!(s.forget("ほう", None), 1);
        assert_eq!(s.bias("ほう", "法"), 0);
        assert_eq!(s.bias("よう", "用"), MANUAL_SCORE_STEP, "別読みは残る");
        // 無い読みは 0 件。
        assert_eq!(s.forget("ない", None), 0);
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
