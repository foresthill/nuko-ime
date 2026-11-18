//! 日付/時刻フォーマット選好ストア (`date_format.json`) — **常時ON**。
//!
//! リテラル日付 (`2026/11/18`) は一過性なので頻度学習しない (古い日付が固定される害を防ぐ、
//! [`super::super::conversion::datetime::is_datetime_surface`] の既定)。だが「どの**形式**を
//! 好むか」(`ymd_slash` 等) はユーザーの安定した選好なので、ここに貯めて次回以降その形式を
//! 日付候補グループの上位に出す。リテラルでなく形式を覚えるので古い日付問題は再発しない。
//!
//! phrase / retype と同じく **ローカル・非共有・churn-free** (format 昇順で決定的保存)。
//! 外部送信は一切しない。format id は
//! [`classify_datetime_format`](super::super::conversion::datetime::classify_datetime_format) が与える。

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{NukoError, Result};

/// フォーマット選好 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateFormatEntry {
    /// フォーマット id (例 `ymd_slash`)。選好の検索キー。
    pub format: String,
    /// その形式を選んだ回数。多いほど上位に出す。
    pub count: u32,
    /// 最終更新時刻 (UNIX 秒)。recency 用 (v1 は保存のみ)。
    #[serde(default)]
    pub last_used: u64,
}

/// 日付/時刻フォーマット選好ストア (`date_format.json`、人間可読・個人・非共有)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateFormatStore {
    entries: Vec<DateFormatEntry>,
}

impl DateFormatStore {
    /// 空のストアを作る。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `date_format.json` を読む。無ければ空。
    ///
    /// # エラー
    /// ファイルはあるが JSON パースに失敗した場合。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<DateFormatEntry> = serde_json::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("date_format.json のパースに失敗: {e}")))?;
        Ok(Self { entries })
    }

    /// `date_format.json` に保存する (format 昇順で churn-free)。
    ///
    /// # エラー
    /// 直列化・書き込みに失敗した場合。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| a.format.cmp(&b.format));
        let content = serde_json::to_string_pretty(&sorted)
            .map_err(|e| NukoError::Learning(format!("date_format.json の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 形式を 1 件記録する (count++)。空 id は無視して `false`。
    pub fn record(&mut self, format: &str) -> bool {
        if format.is_empty() {
            return false;
        }
        let now = current_timestamp();
        if let Some(e) = self.entries.iter_mut().find(|e| e.format == format) {
            e.count = e.count.saturating_add(1);
            e.last_used = now;
        } else {
            self.entries.push(DateFormatEntry {
                format: format.to_string(),
                count: 1,
                last_used: now,
            });
        }
        true
    }

    /// その形式の選好回数 (無ければ 0)。候補スコアへの加点に使う。
    #[must_use]
    pub fn count(&self, format: &str) -> u32 {
        self.entries
            .iter()
            .find(|e| e.format == format)
            .map_or(0, |e| e.count)
    }

    /// 全選好を忘れる。除去件数を返す (「学習を忘れる」用)。
    pub fn forget_all(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
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

    #[test]
    fn record_counts_and_is_queryable() {
        let mut s = DateFormatStore::new();
        assert!(s.record("ymd_slash"));
        assert!(s.record("ymd_slash"));
        assert!(s.record("ymd_kanji"));
        assert_eq!(s.count("ymd_slash"), 2);
        assert_eq!(s.count("ymd_kanji"), 1);
        assert_eq!(s.count("reiwa_dot"), 0, "未選好は 0");
        assert!(!s.record(""), "空 id は記録しない");
    }

    #[test]
    fn forget_all_clears() {
        let mut s = DateFormatStore::new();
        s.record("ymd_slash");
        s.record("ymd_kanji");
        assert_eq!(s.forget_all(), 2);
        assert!(s.is_empty());
        assert_eq!(s.count("ymd_slash"), 0);
    }

    #[test]
    fn save_load_roundtrip() {
        let mut s = DateFormatStore::new();
        s.record("ymd_slash");
        s.record("ymd_slash");
        s.record("hm_ja");
        let path =
            std::env::temp_dir().join(format!("nuko-dateformat-test-{}.json", std::process::id()));
        s.save(&path).unwrap();
        let loaded = DateFormatStore::load(&path).unwrap();
        assert_eq!(loaded.count("ymd_slash"), 2);
        assert_eq!(loaded.count("hm_ja"), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let path = std::env::temp_dir().join("nuko-dateformat-missing-xyz.json");
        let _ = std::fs::remove_file(&path);
        assert!(DateFormatStore::load(&path).unwrap().is_empty());
    }
}
