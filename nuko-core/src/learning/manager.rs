//! 学習マネージャー

use super::frequency::{FrequencyEntry, LearningStrength, MEDIUM_STEP, OVERTAKE_MARGIN};
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
    /// 学習の強さ (選択時にスコアをどれだけ上げるか)。既定 Strong。
    strength: LearningStrength,
}

impl LearningManager {
    /// 新しい学習マネージャーを作成
    pub fn new() -> Result<Self> {
        Ok(Self {
            frequency_data: Arc::new(RwLock::new(HashMap::new())),
            path: None,
            dirty: Arc::new(RwLock::new(false)),
            strength: LearningStrength::default(),
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
                strength: LearningStrength::default(),
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
            strength: LearningStrength::default(),
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

    /// 保存先パスが設定されているか
    #[must_use]
    pub fn has_path(&self) -> bool {
        self.path.is_some()
    }

    /// 学習の強さを設定する (選択時にスコアをどれだけ上げるか)。
    pub fn set_strength(&mut self, strength: LearningStrength) {
        self.strength = strength;
    }

    /// 現在の学習の強さ。
    #[must_use]
    pub fn strength(&self) -> LearningStrength {
        self.strength
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

    /// 学習の強さに応じて、選んだ候補のスコアを引き上げる (`record` の後に呼ぶ)。
    ///
    /// `rival_top` は**同じ読みで選んだ候補以外の最大“合成”スコア** (libakaza /
    /// Layer2 込み) を渡す。strength 別:
    /// - `Weak`: 何もしない (従来どおり `record` の +10 のみ)。
    /// - `Medium`: 固定ステップ ([`MEDIUM_STEP`]) だけブースト (数回で逆転)。
    /// - `Strong`: `rival_top` を恒久的に追い越す (1 回でトップ)。
    ///
    /// いずれも**単調** (ブーストは下げない) なので「選んだのに順位が戻る」ことがない。
    pub fn apply_selection_boost(&mut self, candidate: &Candidate, rival_top: Option<i32>) {
        if self.strength == LearningStrength::Weak {
            return;
        }
        let mut data = self.frequency_data.write();
        let Some(entries) = data.get_mut(&candidate.reading) else {
            return;
        };
        let Some(entry) = entries.iter_mut().find(|e| e.surface == candidate.surface) else {
            return;
        };
        match self.strength {
            LearningStrength::Weak => {}
            LearningStrength::Medium => entry.add_boost(MEDIUM_STEP),
            LearningStrength::Strong => {
                if let Some(top) = rival_top {
                    entry.boost_to_at_least(top, OVERTAKE_MARGIN);
                }
            }
        }
        *self.dirty.write() = true;
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

    #[test]
    fn strong_overtakes_layer2_backed_rival_in_one_selection() {
        // 実機の「様 が Layer2 で底上げされた 用(~300k) に勝てない」を再現し、
        // Strong の追い越しで 1 回の選択で逆転することを確認する。
        let mut manager = LearningManager::new().unwrap();
        let ctx = ConversionContext::default();
        let sama = Candidate::new("様", "よう");

        manager.record(&sama, &ctx).unwrap(); // Layer0 のみ ~200_010
        let before = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        assert!(before < 300_691, "追い越し前は rival(300_691) に負けている");

        // rival(用) の合成スコアを渡して追い越し (Strong 既定)。
        manager.apply_selection_boost(&sama, Some(300_691));
        let after = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        assert!(after > 300_691, "1 回で rival を追い越す (after={after})");

        // 単調: もう一度同じ rival で適用しても下がらない。
        manager.apply_selection_boost(&sama, Some(300_691));
        let again = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        assert_eq!(after, again, "再適用で順位が戻らない (単調)");
    }

    #[test]
    fn weak_strength_does_not_boost() {
        let mut manager = LearningManager::new().unwrap();
        manager.set_strength(super::LearningStrength::Weak);
        let ctx = ConversionContext::default();
        let c = Candidate::new("様", "よう");
        manager.record(&c, &ctx).unwrap();
        let before = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        manager.apply_selection_boost(&c, Some(300_691)); // Weak は無視
        let after = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        assert_eq!(before, after, "Weak はブーストしない (従来どおり)");
    }
}
