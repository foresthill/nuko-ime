//! 頻度学習

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// 頻度情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequencyEntry {
    /// 表層形
    pub surface: String,
    /// 読み
    pub reading: String,
    /// 使用回数
    pub count: u32,
    /// 最終使用時刻（UNIXタイムスタンプ）
    pub last_used: u64,
    /// 文脈ハッシュ（オプション）
    pub context_hash: Option<u64>,
}

impl FrequencyEntry {
    /// 新しいエントリを作成
    #[must_use]
    pub fn new(surface: impl Into<String>, reading: impl Into<String>) -> Self {
        Self {
            surface: surface.into(),
            reading: reading.into(),
            count: 1,
            last_used: current_timestamp(),
            context_hash: None,
        }
    }

    /// 使用回数を増やす
    pub fn increment(&mut self) {
        self.count = self.count.saturating_add(1);
        self.last_used = current_timestamp();
    }

    /// スコアを計算
    ///
    /// 使用回数と最終使用時刻を考慮したスコアを返します。
    ///
    /// ## スコアレンジ設計 (2026-06-09 改訂)
    ///
    /// 旧仕様 (= `base_score + 1000`) では学習スコア ~1,010 程度で、
    /// 同じ `engine.convert` 経路で `LIBAKAZA_PRIORITY_BOOST = 100_000` を
    /// 受ける libakaza 候補に**桁違いに負けて**いた。
    ///
    /// 結果として、ユーザーが「進行」を何度確定しても、libakaza が出す
    /// 「信仰」(score ~99,900) が常に top に来るバグ発生
    /// (実機検証 2026-06-09: 「あと無限回繰り返すのでしょうか?」報告)。
    ///
    /// 修正: 学習データは libakaza の BOOST より高い領域 (200_000+) で
    /// スコアリングし、**ユーザーが 1 回でも選んだら必ず top に来る**ようにする。
    /// 頻度・時間減衰は同じレンジ内で相対順位を決める。
    #[must_use]
    pub fn score(&self) -> i32 {
        const LEARNING_PRIORITY_BOOST: i32 = 200_000;

        let now = current_timestamp();
        let age_days = (now.saturating_sub(self.last_used)) / (24 * 60 * 60);

        // 頻度部: 使用回数 × 10
        let frequency_part = (self.count as i32).saturating_mul(10);

        // 時間減衰: 1 日ごとに 1 点減少 (最大 30 点)。長期間使わないものは
        // 同じ学習データ内で下位に落ちるが、それでも libakaza には勝つ。
        let decay = std::cmp::min(age_days as i32, 30);

        LEARNING_PRIORITY_BOOST + frequency_part - decay
    }
}

/// 現在のUNIXタイムスタンプを取得
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
    fn test_frequency_entry() {
        let mut entry = FrequencyEntry::new("日本", "にほん");
        assert_eq!(entry.count, 1);

        entry.increment();
        assert_eq!(entry.count, 2);
    }

    #[test]
    fn test_score_calculation() {
        let entry = FrequencyEntry::new("日本", "にほん");
        let score = entry.score();
        // 学習データは libakaza BOOST (100_000) を超える領域でスコアリングされる
        assert!(
            score > 100_000,
            "学習スコアは libakaza BOOST より高くあるべき"
        );
        assert!(
            score > 200_000,
            "1 回の使用でも 200_000 以上 (LEARNING_PRIORITY_BOOST)"
        );
    }

    #[test]
    fn test_score_grows_with_count() {
        let mut entry = FrequencyEntry::new("進行", "しんこう");
        let score_1 = entry.score();
        for _ in 0..10 {
            entry.increment();
        }
        let score_11 = entry.score();
        assert!(score_11 > score_1, "使用回数が増えるとスコアも上がる");
    }
}
