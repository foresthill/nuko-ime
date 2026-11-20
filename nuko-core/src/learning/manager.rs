//! 学習マネージャー

use super::correction;
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
    /// - `Medium`: 固定ステップ (`MEDIUM_STEP`) だけブースト (数回で逆転)。
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
            // Weak = 頻度のみ。Crown は `apply_crown_boost` で別処理 (engine が strength で
            // 振り分ける) ので、ここに来たら no-op = 剥奪ロジック抜きの部分適用を避ける。
            LearningStrength::Weak | LearningStrength::Crown => {}
            LearningStrength::Medium => entry.add_boost(MEDIUM_STEP),
            LearningStrength::Strong => {
                if let Some(top) = rival_top {
                    entry.boost_to_at_least(top, OVERTAKE_MARGIN);
                }
            }
        }
        *self.dirty.write() = true;
    }

    /// **王冠ブースト**を与える (`LearningStrength::Crown`。`record` の後に呼ぶ)。
    ///
    /// 「確定した候補が一気に 1 位へ来る」思想を保ちつつ**暴走を断つ**ための中核。
    /// 同じ読みについて:
    /// 1. **他表層の王冠を剥奪** (`demote` で boost を 0 に戻す)。王冠を持てるのは
    ///    「最後に確定した 1 つ」だけ = 交互確定でも両方が競り上がらない。
    /// 2. 剥奪後の**他表層の最大 score (base)** を目標に、選んだ表層へ `set_crown` で
    ///    上限付きブーストを与える (訂正tier `correction::CORRECTION_BOOST` 未満にクランプ)。
    ///
    /// 相手が居なければブーストは 0 (頻度ベース 200_000 で libakaza には勝つ)。
    /// `rival_top` を引数に取らない = 高価な `convert` 不要で、相手の“ブースト済み”値を
    /// 追わない (= ratchet しない) のが `apply_selection_boost(Strong)` との決定的な違い。
    pub fn apply_crown_boost(&mut self, candidate: &Candidate) {
        let mut data = self.frequency_data.write();
        let Some(entries) = data.get_mut(&candidate.reading) else {
            return;
        };

        // 1. 他表層の王冠を剥奪し、剥奪後の最大 base score を測る。
        let mut rival_base_top: Option<i32> = None;
        for e in entries.iter_mut() {
            if e.surface != candidate.surface {
                e.demote(); // boost = 0 (王冠剥奪)。count は保持。
                let s = e.score();
                rival_base_top = Some(rival_base_top.map_or(s, |m| m.max(s)));
            }
        }

        // 2. 選んだ表層へ王冠を設定 (相手が居るときだけ)。
        if let Some(target) = rival_base_top {
            if let Some(entry) = entries.iter_mut().find(|e| e.surface == candidate.surface) {
                entry.set_crown(target, OVERTAKE_MARGIN, correction::CORRECTION_BOOST);
            }
        }

        *self.dirty.write() = true;
    }

    /// 指定の読み＋表層を**降格**する (追い越しブーストを除去し頻度ベースへ戻す)。
    ///
    /// 打ち直し訂正 / 明示的な降格操作で呼ぶ。該当エントリの `FrequencyEntry::demote` を
    /// 実行し、膨張した boost を 0 に戻す。`count` は保持。グローバルな再ランクはせず、
    /// 名指しした (reading, surface) だけ下げるので他の変換は不変。
    ///
    /// 戻り値: 実際に降格した (エントリが存在し boost が 0 でなかった) なら `true`。
    pub fn demote(&mut self, reading: &str, surface: &str) -> bool {
        let mut data = self.frequency_data.write();
        let Some(entries) = data.get_mut(reading) else {
            return false;
        };
        let Some(entry) = entries.iter_mut().find(|e| e.surface == surface) else {
            return false;
        };
        let changed = entry.demote();
        if changed {
            *self.dirty.write() = true;
        }
        changed
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
        manager.set_strength(super::LearningStrength::Strong); // 既定は Crown なので明示
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

    #[test]
    fn demote_removes_overtake_boost_back_to_frequency_base() {
        // 追い越しで膨張した表層を降格 → 頻度ベースへ戻り、名指しした表層だけ下がる。
        let mut manager = LearningManager::new().unwrap();
        manager.set_strength(super::LearningStrength::Strong); // 累積モードの暴走を模擬
        let ctx = ConversionContext::default();
        let you = Candidate::new("用", "よう");
        manager.record(&you, &ctx).unwrap();
        manager.apply_selection_boost(&you, Some(800_000)); // 暴走を模擬
        let inflated = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        assert!(inflated > 300_000, "降格前は訂正tierを超える (={inflated})");

        assert!(manager.demote("よう", "用"), "降格できたら true");
        let after = manager.get_candidates("よう", &ctx).unwrap()[0].score;
        assert!(after < 300_000, "降格後は訂正tier未満 (={after})");

        assert!(!manager.demote("よう", "用"), "再降格は no-op (既に 0)");
        assert!(
            !manager.demote("よう", "無い表層"),
            "存在しない表層は false"
        );
        assert!(!manager.demote("ない読み", "用"), "存在しない読みは false");
    }

    #[test]
    fn crown_makes_committed_top_over_learned_rival() {
        // 既定(Crown)。用 を多用(高count)した後に 様 を確定 → 様 が 1 位へ。
        let mut m = LearningManager::new().unwrap();
        let ctx = ConversionContext::default();
        let you = Candidate::new("用", "よう");
        let sama = Candidate::new("様", "よう");
        for _ in 0..20 {
            m.record(&you, &ctx).unwrap(); // 用 count=20 → base 200_200
        }
        m.record(&sama, &ctx).unwrap(); // 様 count=1 → base 200_010
        m.apply_crown_boost(&sama); // 様 を確定(王冠)

        let cands = m.get_candidates("よう", &ctx).unwrap();
        let top = cands.iter().max_by_key(|c| c.score).unwrap();
        assert_eq!(top.surface, "様", "確定した 様 が 1 位");
        assert!(top.score < 300_000, "訂正tier未満 (={})", top.score);
    }

    #[test]
    fn crown_strips_previous_crown_and_does_not_ratchet() {
        // 用/様 を交互に確定しても、王冠は「最後の 1 つ」だけ = 青天井に競り上がらない。
        let mut m = LearningManager::new().unwrap();
        let ctx = ConversionContext::default();
        let you = Candidate::new("用", "よう");
        let sama = Candidate::new("様", "よう");
        m.record(&you, &ctx).unwrap();
        m.record(&sama, &ctx).unwrap();

        let mut last_top_score = 0;
        for i in 0..10 {
            let pick = if i % 2 == 0 { &sama } else { &you };
            m.record(pick, &ctx).unwrap();
            m.apply_crown_boost(pick);
            let cands = m.get_candidates("よう", &ctx).unwrap();
            let top = cands.iter().max_by_key(|c| c.score).unwrap();
            assert_eq!(top.surface, pick.surface, "最後に確定した方が 1 位");
            assert!(
                top.score < 300_000,
                "交互確定でも訂正tier未満に収まる (i={i}, score={})",
                top.score
            );
            last_top_score = top.score;
        }
        // 10 回交互でも 250k 近辺に収まる (旧 Strong は 500k 超へ暴走していた)。
        assert!(
            last_top_score < 260_000,
            "10 回交互でも ~250k で頭打ち (={last_top_score})"
        );
    }

    #[test]
    fn crown_keeps_other_surfaces_boost_zero() {
        // 剥奪された他表層は boost=0 (頻度ベース) に戻っている。
        let mut m = LearningManager::new().unwrap();
        let ctx = ConversionContext::default();
        let you = Candidate::new("用", "よう");
        let sama = Candidate::new("様", "よう");
        m.record(&you, &ctx).unwrap();
        m.apply_crown_boost(&you); // 用 に王冠
        m.record(&sama, &ctx).unwrap();
        m.apply_crown_boost(&sama); // 様 に王冠 → 用 の王冠は剥奪

        let data = m.frequency_data.read();
        let entries = data.get("よう").unwrap();
        let you_entry = entries.iter().find(|e| e.surface == "用").unwrap();
        assert_eq!(you_entry.boost, 0, "剥奪された 用 は boost=0");
    }
}
