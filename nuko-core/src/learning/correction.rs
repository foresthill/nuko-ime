//! Layer 2: 訂正学習 (Correction / Preference)
//!
//! 観察ログ(Layer 1、[`super::observation`])から **決定論的に** 個人の変換選好を
//! 抽出し、`corrections.toml`(人間可読・編集可)に保存する。変換時は該当候補へ
//! bias を加える。**AI 不使用・冪等(churn-free)** — 同じ観察ログからは常に同じ
//! corrections が出る。
//!
//! ここは Layer 3(AI dreaming)が研ぎ直す土台でもある。AI はこの store を
//! 「決定論的に再計算」するだけで、提案キューは作らない。
//!
//! 詳細設計: [`docs/LEARNING_ARCHITECTURE.md`](../../../docs/LEARNING_ARCHITECTURE.md)。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::observation::ObservationEvent;
use crate::error::{NukoError, Result};

/// 選好一致時に候補へ加える bias の基準値。
///
/// frequency 学習の boost(~200_000)を上回るようにして、**確定した個人選好が
/// 頻度・libakaza より優先**されるようにする。重みは僅かな tiebreak。
/// (スケールの厳密なチューニングは変換への配線時に詰める)
const CORRECTION_BOOST: i32 = 300_000;

/// 個人の変換選好 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preference {
    /// 読み(かな)
    pub reading: String,
    /// 優先する表層
    pub prefer: String,
    /// 重み(観測回数由来。bias の tiebreak に使う)
    #[serde(default)]
    pub weight: u32,
    /// 観測回数(透明性: どれだけの根拠で学習したか)
    #[serde(default)]
    pub seen: u32,
}

/// 訂正学習の結果全体(= `corrections.toml` の中身)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionStore {
    /// 選好の一覧(toml では `[[preference]]` の配列)。
    #[serde(default, rename = "preference")]
    pub preferences: Vec<Preference>,
}

impl CorrectionStore {
    /// `corrections.toml` を読む。ファイルが無ければ空の store。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("corrections.toml のパースに失敗: {e}")))
    }

    /// `corrections.toml` に保存する(人間可読)。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| NukoError::Learning(format!("corrections.toml の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// `reading` の候補 `surface` に対する bias を返す(一致する選好が無ければ 0)。
    ///
    /// 変換時に候補スコアへ加算する用途。
    #[must_use]
    pub fn bias(&self, reading: &str, surface: &str) -> i32 {
        for p in &self.preferences {
            if p.reading == reading && p.prefer == surface {
                // 基準 boost + 僅かな重み tiebreak (上限あり)
                return CORRECTION_BOOST + (p.weight as i32).saturating_mul(10).min(50_000);
            }
        }
        0
    }

    /// 選好が無いか。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.preferences.is_empty()
    }

    /// 選好の件数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.preferences.len()
    }
}

/// 観察ログから選好を **決定論的に** 抽出する(Layer 2 の心臓)。
///
/// - `min_seen` 回以上コミットされた `(reading → surface)` だけを選好化する
///   (偶発的な 1 回の確定をルールにしない)。
/// - 1 つの `reading` に対しては **最多コミットの surface** を採用。同数は
///   surface 名の昇順で安定させる(= 同じ入力から常に同じ出力 = churn-free)。
///
/// 返る `preferences` は reading 昇順で安定。`corrections.toml` はバイト単位で
/// 再現するので、dreaming(Layer 3)や再抽出で差分 churn を生まない。
///
/// 注: 現状は Commit イベントの多数決のみ。将来 `picked`(既定候補を上書きしたか)
/// や Correction イベント、文脈(前語)を強いシグナルとして加える。
#[must_use]
pub fn extract_corrections(events: &[ObservationEvent], min_seen: u32) -> CorrectionStore {
    // reading -> surface -> count (BTreeMap で決定論的順序)
    let mut tally: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for ev in events {
        if let ObservationEvent::Commit {
            reading, surface, ..
        } = ev
        {
            if reading.is_empty() || surface.is_empty() {
                continue;
            }
            *tally
                .entry(reading.clone())
                .or_default()
                .entry(surface.clone())
                .or_default() += 1;
        }
    }

    let mut preferences = Vec::new();
    for (reading, surfaces) in tally {
        // count 降順、同数は surface 昇順で安定
        let mut ranked: Vec<(&String, &u32)> = surfaces.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        if let Some((surface, &count)) = ranked.first() {
            if count >= min_seen {
                preferences.push(Preference {
                    reading,
                    prefer: (*surface).clone(),
                    weight: count,
                    seen: count,
                });
            }
        }
    }

    CorrectionStore { preferences }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(reading: &str, surface: &str) -> ObservationEvent {
        ObservationEvent::commit(reading, surface)
    }

    #[test]
    fn extract_makes_preference_above_threshold() {
        // 「きしゃ→記者」を 3 回、「きしゃ→汽車」を 1 回
        let events = vec![
            commit("きしゃ", "記者"),
            commit("きしゃ", "記者"),
            commit("きしゃ", "記者"),
            commit("きしゃ", "汽車"),
        ];
        let store = extract_corrections(&events, 3);
        assert_eq!(store.len(), 1);
        assert_eq!(store.preferences[0].reading, "きしゃ");
        assert_eq!(store.preferences[0].prefer, "記者", "★ 最多コミットを採用");
        assert_eq!(store.preferences[0].seen, 3);
    }

    #[test]
    fn below_threshold_makes_no_preference() {
        let events = vec![commit("あ", "亜"), commit("あ", "亜")];
        // min_seen=3 に届かない
        assert!(extract_corrections(&events, 3).is_empty());
    }

    #[test]
    fn picks_majority_surface() {
        let events = vec![
            commit("かえる", "帰る"),
            commit("かえる", "帰る"),
            commit("かえる", "蛙"),
        ];
        let store = extract_corrections(&events, 2);
        assert_eq!(store.preferences[0].prefer, "帰る", "★ 多数決で「帰る」");
    }

    #[test]
    fn bias_matches_reading_and_surface() {
        let events = vec![commit("きしゃ", "記者"), commit("きしゃ", "記者")];
        let store = extract_corrections(&events, 2);
        assert!(store.bias("きしゃ", "記者") > 0, "★ 一致する選好に bias");
        assert_eq!(
            store.bias("きしゃ", "汽車"),
            0,
            "★ 別 surface には bias なし"
        );
        assert_eq!(
            store.bias("でんしゃ", "電車"),
            0,
            "★ 別 reading には bias なし"
        );
    }

    /// ★ churn-free: 同じ入力から常に同じ出力(順序も含めバイト一致)。
    #[test]
    fn extraction_is_deterministic() {
        let events = vec![
            commit("に", "二"),
            commit("さん", "三"),
            commit("に", "二"),
            commit("いち", "一"),
            commit("さん", "三"),
            commit("いち", "一"),
        ];
        let a = extract_corrections(&events, 2);
        let b = extract_corrections(&events, 2);
        assert_eq!(a, b, "★ 同じ入力から同じ store");
        // reading 昇順で安定 (いち, さん, に)
        let readings: Vec<&str> = a.preferences.iter().map(|p| p.reading.as_str()).collect();
        assert_eq!(readings, vec!["いち", "さん", "に"], "★ 安定した順序");
        // toml 直列化もバイト一致
        assert_eq!(
            toml::to_string_pretty(&a).unwrap(),
            toml::to_string_pretty(&b).unwrap(),
        );
    }

    #[test]
    fn toml_roundtrip_and_human_readable() {
        let events = vec![commit("きしゃ", "記者"), commit("きしゃ", "記者")];
        let store = extract_corrections(&events, 2);

        let mut path = std::env::temp_dir();
        path.push(format!("nuko-corr-test-{}.toml", std::process::id()));
        store.save(&path).unwrap();

        // 人間可読: [[preference]] と reading/prefer が見える
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[[preference]]"));
        assert!(content.contains("reading = \"きしゃ\""));
        assert!(content.contains("prefer = \"記者\""));

        let loaded = CorrectionStore::load(&path).unwrap();
        assert_eq!(loaded, store, "★ save→load で往復一致");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_returns_empty() {
        let store = CorrectionStore::load("/nonexistent/nuko/corrections.toml").unwrap();
        assert!(store.is_empty());
    }
}
