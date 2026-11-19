//! 打ち直し訂正ストア (`retype.json`) — 誤変換の「打ち直し」を**常時ON**で捉える。
//!
//! 「同じ読みを短時間に別の表層で確定し直した」= 最も痛い瞬間であり、最大の学習機会
//! (森岡さん 2026-11-15)。打ち直しの検知自体は controller/state で以前から在ったが、
//! 記録先が観察ログ (Layer 1, opt-in・既定 OFF [`super::observation`]) だったため、
//! **既定ユーザーでは打ち直しシグナルを丸ごと捨てていた**。
//!
//! このストアは **フレーズ予測 ([`super::phrase`]) と同じく常時ON・ローカル・非共有**
//! で、打ち直し訂正だけを溜める (= 低 volume・高価値シグナル)。外部送信は一切しない。
//!
//! ## 変換への反映は「新しい層を足さない」
//!
//! [`RetypeStore::as_correction_events`] で [`ObservationEvent::Correction`] 列へ変換し、
//! 既存の [`extract_corrections`](super::extract_corrections) に食わせて
//! [`CorrectionStore`](super::CorrectionStore) を作る。訂正選好は `convert` /
//! `convert_segmented` の**両経路で既に消費**されており (bias / preferred /
//! apply_segment_corrections)、かつ `CORRECTION_BOOST`(300_000) > Layer0(200_000) なので、
//! **1 回の打ち直しで「正」が「誤」に決定的に勝つ**。観察ログ opt-in の有無に依らず
//! 効くのが本改善の要点。

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::observation::ObservationEvent;
use crate::error::{NukoError, Result};

/// 打ち直し訂正 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetypeEntry {
    /// 読み (かな)。訂正選好の検索キー。
    pub reading: String,
    /// 打ち直し後の表層 (= 正)。
    pub prefer: String,
    /// 打ち直し前の表層 (= 誤)。provenance / ビューア / supersede 判定に使う。
    pub avoid: String,
    /// 同じ (reading, prefer) を打ち直した回数。
    pub count: u32,
    /// 最終更新時刻 (UNIX 秒)。recency 用 (v1 は保存のみ)。
    #[serde(default)]
    pub last_used: u64,
}

/// 打ち直し訂正ストア (`retype.json`、人間可読・個人・非共有)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetypeStore {
    /// 打ち直し訂正の一覧。
    entries: Vec<RetypeEntry>,
}

impl RetypeStore {
    /// 空のストアを作る。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `retype.json` を読む。ファイルが無ければ空。
    ///
    /// 形式は `phrases.json` と同じ「エントリ配列」(トップレベル JSON 配列)。
    ///
    /// # エラー
    /// ファイルはあるが JSON パースに失敗した場合。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<RetypeEntry> = serde_json::from_str(&content)
            .map_err(|e| NukoError::Learning(format!("retype.json のパースに失敗: {e}")))?;
        Ok(Self { entries })
    }

    /// `retype.json` に保存する (読み昇順で churn-free)。
    ///
    /// # エラー
    /// 直列化・書き込みに失敗した場合。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| a.reading.cmp(&b.reading).then(a.prefer.cmp(&b.prefer)));
        let content = serde_json::to_string_pretty(&sorted)
            .map_err(|e| NukoError::Learning(format!("retype.json の直列化に失敗: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 打ち直し訂正を 1 件記録する。記録・更新したら `true`。
    ///
    /// ゲート (いずれかに該当すれば記録しない → `false`):
    /// - 読み・誤・正のどれかが空
    /// - 正 == 読み (かな素通りへの訂正は選好にしない。恒等は
    ///   [`extract_corrections`](super::extract_corrections) も弾く)
    /// - 正 == 誤 (実際の打ち直しでない)
    ///
    /// **supersede**: 同じ読みで「以前 prefer にしていた表層」を今回 avoid (= 誤) として
    /// 打ち直したときは、その古いエントリを落とす。A→B の後に B→C と打ち直すと
    /// 「読み→B」は消え「読み→C」だけが残る (最新の意図が勝つ)。
    pub fn record(&mut self, reading: &str, avoid: &str, prefer: &str) -> bool {
        if reading.is_empty() || avoid.is_empty() || prefer.is_empty() {
            return false;
        }
        if prefer == reading || prefer == avoid {
            return false;
        }
        // supersede: この読みで「今回捨てた表層 (avoid)」を正としていた古い学習を落とす。
        self.entries
            .retain(|e| !(e.reading == reading && e.prefer == avoid));

        let now = current_timestamp();
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.reading == reading && e.prefer == prefer)
        {
            e.count = e.count.saturating_add(1);
            e.avoid = avoid.to_string();
            e.last_used = now;
        } else {
            self.entries.push(RetypeEntry {
                reading: reading.to_string(),
                prefer: prefer.to_string(),
                avoid: avoid.to_string(),
                count: 1,
                last_used: now,
            });
        }
        true
    }

    /// 指定の読み (と任意で正の表層) に一致するエントリを忘れる。除去件数を返す。
    ///
    /// `surface = None` ならその読みを全て、`Some(s)` なら `prefer == s` のものだけ除去。
    /// 設定窓 / `nuko learn forget` と共用。観察ログ側の [`filter_out_reading`] と対。
    ///
    /// [`filter_out_reading`]: super::filter_out_reading
    pub fn forget(&mut self, reading: &str, surface: Option<&str>) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            if e.reading != reading {
                return true;
            }
            match surface {
                None => false,
                Some(s) => e.prefer != s,
            }
        });
        before - self.entries.len()
    }

    /// 全エントリを [`ObservationEvent::Correction`] 列へ変換する (extract_corrections 用)。
    ///
    /// 1 エントリ = 1 Correction。`extract_corrections` は Correction 1 件で
    /// weight 5 (≥ 既定 min_seen 2) を与えるので、**1 回の打ち直しで選好化**される。
    /// 読み昇順で決定的 (churn-free)。
    #[must_use]
    pub fn as_correction_events(&self) -> Vec<ObservationEvent> {
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| a.reading.cmp(&b.reading).then(a.prefer.cmp(&b.prefer)));
        sorted
            .into_iter()
            .map(|e| ObservationEvent::correction(e.reading, e.avoid, e.prefer))
            .collect()
    }

    /// 全エントリの `(読み, 誤=avoid)` ペアを返す (降格対象の列挙用)。
    ///
    /// 打ち直しで「誤」とされた表層を、頻度学習側 (Layer0) でも降格するために使う。
    /// 訂正 (Layer2) で「正」を上げるだけでは、追い越しブーストで膨張した「誤」が
    /// 訂正tier を超えていると勝てないため (2026-11 悪循環ループの根治)。
    #[must_use]
    pub fn avoided_pairs(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .map(|e| (e.reading.clone(), e.avoid.clone()))
            .collect()
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
    use crate::learning::extract_corrections;

    #[test]
    fn record_rejects_empty_and_identity() {
        let mut s = RetypeStore::new();
        assert!(!s.record("", "誤", "正"));
        assert!(!s.record("よみ", "", "正"));
        assert!(!s.record("よみ", "誤", ""));
        // 正 == 読み (かな素通りへの訂正) は記録しない。
        assert!(!s.record("ねこ", "猫", "ねこ"));
        // 正 == 誤 (打ち直しでない) は記録しない。
        assert!(!s.record("ねこ", "猫", "猫"));
        assert!(s.is_empty());
    }

    #[test]
    fn record_and_count_increments() {
        let mut s = RetypeStore::new();
        assert!(s.record("いしみね", "石峯", "石峰"));
        assert!(s.record("いしみね", "石峯", "石峰"));
        assert_eq!(s.len(), 1, "同じ (読み,正) は 1 件に集約");
        assert_eq!(s.as_correction_events().len(), 1);
    }

    /// ★ supersede: A→B の後に B→C と打ち直すと「読み→B」は消え「読み→C」だけ残る。
    #[test]
    fn reflip_supersedes_old_entry() {
        let mut s = RetypeStore::new();
        s.record("こまや", "駒屋", "駒谷"); // 駒屋 → 駒谷
        s.record("こまや", "駒谷", "駒茶"); // さらに 駒谷(=前回の正) → 駒茶
        assert_eq!(s.len(), 1, "古い 駒谷 は supersede で消える");
        let evs = s.as_correction_events();
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            ObservationEvent::Correction { corrected, .. } => {
                assert_eq!(corrected, "駒茶", "最新の正が残る");
            }
            ev @ ObservationEvent::Commit { .. } => panic!("Correction でない: {ev:?}"),
        }
    }

    #[test]
    fn forget_by_reading_and_by_surface() {
        let mut s = RetypeStore::new();
        s.record("いしみね", "石峯", "石峰");
        s.record("こまや", "駒屋", "駒谷");
        // 読み+表層指定: 該当 1 件だけ消える。
        assert_eq!(s.forget("いしみね", Some("石峰")), 1);
        assert_eq!(s.len(), 1);
        // 不一致表層は消えない。
        assert_eq!(s.forget("こまや", Some("別")), 0);
        // 読みのみ指定: その読みを全消し。
        assert_eq!(s.forget("こまや", None), 1);
        assert!(s.is_empty());
    }

    /// ★★ 全部つながる: 打ち直し → Correction 列 → extract_corrections で、
    /// 正が Layer0(200_000) を超える boost で選好化される (= 決定的に誤に勝つ)。
    #[test]
    fn retype_feeds_decisive_correction_preference() {
        let mut s = RetypeStore::new();
        s.record("いしみね", "石峯", "石峰");
        let store = extract_corrections(&s.as_correction_events(), 2);
        let (surface, bias) = store.preferred("いしみね").expect("選好が作られる");
        assert_eq!(surface, "石峰", "★ 打ち直し後(石峰)が選好");
        assert!(
            bias > 200_000,
            "★ Layer0(200_000) を超える boost = 決定的に勝つ: bias={bias}"
        );
    }

    #[test]
    fn save_load_roundtrip() {
        let mut s = RetypeStore::new();
        s.record("いしみね", "石峯", "石峰");
        s.record("こまや", "駒屋", "駒谷");
        let path =
            std::env::temp_dir().join(format!("nuko-retype-test-{}.json", std::process::id()));
        s.save(&path).unwrap();
        let loaded = RetypeStore::load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded, {
            // save は読み昇順に並べ替えるので、比較用に同じ並びへ。
            let mut e = s.clone();
            e.entries
                .sort_by(|a, b| a.reading.cmp(&b.reading).then(a.prefer.cmp(&b.prefer)));
            e
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let path = std::env::temp_dir().join("nuko-retype-does-not-exist-xyz.json");
        let _ = std::fs::remove_file(&path);
        assert!(RetypeStore::load(&path).unwrap().is_empty());
    }
}
