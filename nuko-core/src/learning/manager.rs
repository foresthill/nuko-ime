//! 学習マネージャー

use super::frequency::FrequencyEntry;
use crate::conversion::{Candidate, ConversionContext};
use crate::error::{NukoError, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// 学習マネージャー
///
/// ユーザーの入力パターンを学習し、変換候補のスコアリングに使用します。
pub struct LearningManager {
    /// 頻度データ（読み → エントリリスト）
    frequency_data: Arc<RwLock<HashMap<String, Vec<FrequencyEntry>>>>,
    /// 保存先パス
    path: Option<std::path::PathBuf>,
    /// 変更フラグ
    dirty: Arc<RwLock<bool>>,
}

impl LearningManager {
    /// 新しい学習マネージャーを作成
    pub fn new() -> Result<Self> {
        Ok(Self {
            frequency_data: Arc::new(RwLock::new(HashMap::new())),
            path: None,
            dirty: Arc::new(RwLock::new(false)),
        })
    }

    /// ファイルから読み込み
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self {
                frequency_data: Arc::new(RwLock::new(HashMap::new())),
                path: Some(path.to_path_buf()),
                dirty: Arc::new(RwLock::new(false)),
            });
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| NukoError::Learning(format!("学習データの読み込みに失敗: {e}")))?;

        let entries: Vec<FrequencyEntry> = serde_json::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("学習データのパースに失敗: {e}")))?;

        let mut map: HashMap<String, Vec<FrequencyEntry>> = HashMap::new();
        for entry in entries {
            map.entry(entry.reading.clone()).or_default().push(entry);
        }

        Ok(Self {
            frequency_data: Arc::new(RwLock::new(map)),
            path: Some(path.to_path_buf()),
            dirty: Arc::new(RwLock::new(false)),
        })
    }

    /// ファイルに保存
    pub fn save(&self) -> Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| NukoError::Learning("保存先が設定されていません".to_string()))?;

        let data = self.frequency_data.read();
        let all_entries: Vec<&FrequencyEntry> = data.values().flatten().collect();

        let content = serde_json::to_string_pretty(&all_entries)
            .map_err(|e| NukoError::Learning(format!("シリアライズに失敗: {e}")))?;

        // ディレクトリがなければ作成
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)?;

        *self.dirty.write() = false;
        Ok(())
    }

    /// 保存先パスを設定
    pub fn set_path(&mut self, path: impl AsRef<Path>) {
        self.path = Some(path.as_ref().to_path_buf());
    }

    /// 変換結果を記録
    pub fn record(&mut self, candidate: &Candidate, _context: &ConversionContext) -> Result<()> {
        let mut data = self.frequency_data.write();

        let entries = data.entry(candidate.reading.clone()).or_default();

        // 既存エントリを検索
        if let Some(entry) = entries.iter_mut().find(|e| e.surface == candidate.surface) {
            entry.increment();
        } else {
            // 新規エントリを追加
            entries.push(FrequencyEntry::new(&candidate.surface, &candidate.reading));
        }

        *self.dirty.write() = true;
        Ok(())
    }

    /// 学習データから候補を取得
    pub fn get_candidates(
        &self,
        reading: &str,
        _context: &ConversionContext,
    ) -> Result<Vec<Candidate>> {
        let data = self.frequency_data.read();

        let candidates = data
            .get(reading)
            .map(|entries| {
                entries
                    .iter()
                    .map(|e| Candidate::new(&e.surface, &e.reading).with_score(e.score()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(candidates)
    }

    /// 学習データをクリア
    pub fn clear(&mut self) -> Result<()> {
        let mut data = self.frequency_data.write();
        data.clear();
        *self.dirty.write() = true;
        Ok(())
    }

    /// 変更があるかどうか
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        *self.dirty.read()
    }

    /// エントリ数を取得
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.frequency_data.read().values().map(|v| v.len()).sum()
    }
}

impl Default for LearningManager {
    fn default() -> Self {
        Self::new().expect("Failed to create learning manager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_retrieve() {
        let mut manager = LearningManager::new().unwrap();
        let context = ConversionContext::default();
        let candidate = Candidate::new("日本", "にほん");

        manager.record(&candidate, &context).unwrap();

        let candidates = manager.get_candidates("にほん", &context).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].surface, "日本");
    }

    #[test]
    fn test_frequency_increment() {
        let mut manager = LearningManager::new().unwrap();
        let context = ConversionContext::default();
        let candidate = Candidate::new("日本", "にほん");

        // 3回記録
        manager.record(&candidate, &context).unwrap();
        manager.record(&candidate, &context).unwrap();
        manager.record(&candidate, &context).unwrap();

        let candidates = manager.get_candidates("にほん", &context).unwrap();
        assert_eq!(candidates.len(), 1);
        // スコアが増加していることを確認
        assert!(candidates[0].score > 1000);
    }
}
