//! 文節位置の学習ストア (`segmentations.toml`)。
//!
//! ユーザーが Shift+←→ で文節を切り直して確定したとき、その「切り方」
//! (読み全体 → 各文節の読み) を覚える。同じ読みが来たら
//! [`crate::conversion::ConversionEngine`] がこの切り方を強制再適用する
//! (`convert_segmented_forced`)。= **使うほど自分の区切りに寄る**。
//!
//! 変換の学習 (読み→表層, [`super::CorrectionStore`]) とは別軸の学習で、
//! 「どこで区切るか」だけを扱う。churn-free (同じ入力から同じ出力)。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{NukoError, Result};

/// 1 件の文節境界の学習。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentationEntry {
    /// 読み全体 (かな)。
    pub reading: String,
    /// 各文節の読み (連結すると `reading` に一致する)。
    pub segments: Vec<String>,
}

/// 文節境界の学習ストア。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentationStore {
    /// 学習した切り方の一覧 (toml では `[[segmentation]]` の配列)。
    #[serde(default, rename = "segmentation")]
    pub entries: Vec<SegmentationEntry>,
}

impl SegmentationStore {
    /// `segmentations.toml` を読む。ファイルが無ければ空。
    ///
    /// # エラー
    /// ファイルはあるが TOML パースに失敗した場合。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("segmentations.toml のパースに失敗: {e}")))
    }

    /// `segmentations.toml` に保存する (人間可読・読み昇順で churn-free)。
    ///
    /// # エラー
    /// 直列化・書き込みに失敗した場合。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sorted = self.clone();
        sorted.entries.sort_by(|a, b| a.reading.cmp(&b.reading));
        let content = toml::to_string_pretty(&sorted)
            .map_err(|e| NukoError::Learning(format!("segmentations.toml の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// その読みちょうどに学習した切り方があれば、各文節の読みを返す。
    #[must_use]
    pub fn lookup(&self, reading: &str) -> Option<&[String]> {
        self.entries
            .iter()
            .find(|e| e.reading == reading)
            .map(|e| e.segments.as_slice())
    }

    /// 切り方を学習する (同じ読みは**上書き** = 最新の切り方を優先)。
    ///
    /// 連結が `reading` に一致しない / 2 文節未満 / 空文節を含む場合は**学習しない**
    /// (壊れた切り方を保存しないための安全弁)。学習したら `true`。
    pub fn learn(&mut self, reading: &str, segments: &[String]) -> bool {
        if segments.len() < 2 {
            return false; // 1 文節は「切り方」ではない
        }
        if segments.iter().any(String::is_empty) {
            return false;
        }
        if segments.concat() != reading {
            return false; // 連結不一致 = 壊れた切り方
        }
        let entry = SegmentationEntry {
            reading: reading.to_string(),
            segments: segments.to_vec(),
        };
        if let Some(existing) = self.entries.iter_mut().find(|e| e.reading == reading) {
            if existing.segments == entry.segments {
                return false; // 変化なし (churn を出さない)
            }
            existing.segments = entry.segments;
        } else {
            self.entries.push(entry);
        }
        true
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

/// 各文節の読みから、読み全体に対する**バイトオフセット範囲列**を作る。
///
/// `convert_segmented_forced` にそのまま渡せる `force_ranges`。
/// 連結が `total_len` に一致しないときは `None` (呼び出し側で無視)。
#[must_use]
pub fn ranges_from_segment_readings(
    segments: &[String],
    total_len: usize,
) -> Option<Vec<std::ops::Range<usize>>> {
    let mut ranges = Vec::with_capacity(segments.len());
    let mut off = 0usize;
    for s in segments {
        if s.is_empty() {
            return None;
        }
        let end = off + s.len();
        ranges.push(off..end);
        off = end;
    }
    if off == total_len {
        Some(ranges)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn learn_and_lookup() {
        let mut store = SegmentationStore::default();
        assert!(store.learn("ざびさんと", &segs(&["ざび", "さん", "と"])));
        assert_eq!(
            store.lookup("ざびさんと"),
            Some(segs(&["ざび", "さん", "と"]).as_slice())
        );
        assert!(store.lookup("べつ").is_none());
    }

    #[test]
    fn learn_rejects_broken_splits() {
        let mut store = SegmentationStore::default();
        // 連結不一致
        assert!(!store.learn("ざびさんと", &segs(&["ざび", "さん"])));
        // 1 文節
        assert!(!store.learn("ざび", &segs(&["ざび"])));
        // 空文節
        assert!(!store.learn("ab", &segs(&["a", ""])));
        assert!(store.is_empty());
    }

    #[test]
    fn learn_overwrites_and_is_churn_free() {
        let mut store = SegmentationStore::default();
        assert!(store.learn("ざびさんと", &segs(&["ざ", "びさんと"])));
        // 同じ読みを別の切り方で上書き
        assert!(store.learn("ざびさんと", &segs(&["ざび", "さんと"])));
        assert_eq!(store.len(), 1, "上書き (重複しない)");
        assert_eq!(
            store.lookup("ざびさんと"),
            Some(segs(&["ざび", "さんと"]).as_slice())
        );
        // 同じ切り方の再学習は churn を出さない
        assert!(!store.learn("ざびさんと", &segs(&["ざび", "さんと"])));
    }

    #[test]
    fn ranges_from_readings() {
        // ざび(6)+さん(6)+と(3) = 15 bytes
        let s = segs(&["ざび", "さん", "と"]);
        let total = "ざびさんと".len();
        assert_eq!(
            ranges_from_segment_readings(&s, total),
            Some(vec![0..6, 6..12, 12..15])
        );
        // 長さ不一致は None
        assert_eq!(ranges_from_segment_readings(&s, total + 1), None);
    }
}
