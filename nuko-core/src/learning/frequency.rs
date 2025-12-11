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
    #[must_use]
    pub fn score(&self) -> i32 {
        let now = current_timestamp();
        let age_days = (now.saturating_sub(self.last_used)) / (24 * 60 * 60);

        // 基本スコア: 使用回数 × 10
        let base_score = (self.count as i32) * 10;

        // 時間減衰: 1日ごとに1点減少（最大30点）
        let decay = std::cmp::min(age_days as i32, 30);

        base_score - decay + 1000 // 学習データは高優先
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
        assert!(score > 1000); // 基本スコア + 学習ボーナス
    }
}
