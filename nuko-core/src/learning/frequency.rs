//! 頻度学習

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// 学習の「強さ」= 候補を選んだときスコアをどれだけ上げるか。
///
/// 可視化で「1 回選んでも +10 しか上がらず、Layer2 で底上げされた rival
/// (例 `用` ~300k) に永遠に勝てない」と判明した (2026-11-14、実機 + `--explain`)。
/// そこで「上げ幅」をユーザー設定にする。設計判断は docs/LEARNING_ARCHITECTURE.md §1.6:
/// - **減衰する recency ボーナスは不採用**(「さっき上に出たのに戻ってる!?」= 非単調で混乱)。
/// - 採用 = **追い越し(恒久・単調)**: 選んだら今のトップより上へ恒久的に引き上げる
///   ([`FrequencyEntry::boost_to_at_least`])。スコアは選択時だけ上がる = 下がらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LearningStrength {
    /// 従来どおり (1 選択 = 使用回数 +1 = スコア +10)。追い越しなし。
    Weak,
    /// 段階的に大きく上げる (1 選択ごとに固定ステップ加点)。数回で逆転。
    Medium,
    /// 1 選択で必ずトップへ (今の最上位候補を恒久的に追い越す)。既定。
    #[default]
    Strong,
}

/// 「中」1 選択あたりの固定ブースト加点。数回で高スコア rival も抜ける大きさ。
pub const MEDIUM_STEP: i32 = 20_000;
/// 追い越し時に相手へ乗せる余裕 (同点負けを防ぐ)。
pub const OVERTAKE_MARGIN: i32 = 100;

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
    /// 追い越しブースト (選択時に積む恒久加点。単調増加のみ)。既定 0。
    ///
    /// 「選んだら今のトップより上へ」を Layer0 内で実現するための加点。下げない
    /// (単調) ので「順位が勝手に戻る」ことがない。`#[serde(default)]` で旧
    /// `learning.json` (このフィールド無し) も読める。
    #[serde(default)]
    pub boost: i32,
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
            boost: 0,
        }
    }

    /// 使用回数を増やす
    pub fn increment(&mut self) {
        self.count = self.count.saturating_add(1);
        self.last_used = current_timestamp();
    }

    /// 固定ステップだけブーストを積む (「中」用。単調増加)。
    pub fn add_boost(&mut self, step: i32) {
        if step > 0 {
            self.boost = self.boost.saturating_add(step);
        }
    }

    /// このエントリの [`score`](Self::score) が `target + margin` 以上になるよう
    /// ブーストを引き上げる (「強」= 追い越し用)。
    ///
    /// **単調**: 既に上回っていれば何もしない。下げることはない (= 順位が勝手に
    /// 戻らない)。`target` は追い越したい相手候補の**合成スコア** (libakaza / Layer2
    /// 込み) を渡す。Layer0 は `max(基礎, Layer0)` で合成されるため、Layer0 側を
    /// 相手の合成値より上げれば Layer2 で底上げされた rival にも勝てる。
    pub fn boost_to_at_least(&mut self, target: i32, margin: i32) {
        let needed = target.saturating_add(margin).saturating_sub(self.score());
        if needed > 0 {
            self.boost = self.boost.saturating_add(needed);
        }
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

        // 追い越しブースト (選択時に積む恒久加点)。単調増加なので「選んだのに
        // 順位が戻る」ことがない。
        LEARNING_PRIORITY_BOOST
            .saturating_add(frequency_part)
            .saturating_sub(decay)
            .saturating_add(self.boost)
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

    #[test]
    fn boost_to_at_least_overtakes_higher_rival() {
        // 様 (純 Layer0) が Layer2 で底上げされた 用 (合成 300_691) を 1 回で追い越す。
        let mut sama = FrequencyEntry::new("様", "よう"); // count=1 → score ~200_010
        let rival_combined = 300_691;
        assert!(sama.score() < rival_combined, "最初は rival に負けている");
        sama.boost_to_at_least(rival_combined, OVERTAKE_MARGIN);
        assert!(
            sama.score() > rival_combined,
            "追い越し後は rival を上回る (score={} > {})",
            sama.score(),
            rival_combined
        );
    }

    #[test]
    fn boost_to_at_least_is_monotonic_noop_when_already_above() {
        // 既に上回っていれば boost は積まない (下げない・無駄に増やさない)。
        let mut e = FrequencyEntry::new("猫", "ねこ");
        let before = e.boost;
        e.boost_to_at_least(100, OVERTAKE_MARGIN); // 100+margin < 200_000+ なので不要
        assert_eq!(e.boost, before, "既に上ならブースト不変 (単調・無駄なし)");
    }

    #[test]
    fn boost_never_decreases_on_reapply() {
        // 同じ target を再適用しても下がらない (単調 = 順位が戻らない)。
        let mut e = FrequencyEntry::new("様", "よう");
        e.boost_to_at_least(300_000, OVERTAKE_MARGIN);
        let b1 = e.boost;
        e.boost_to_at_least(300_000, OVERTAKE_MARGIN);
        assert_eq!(e.boost, b1, "再適用で変化なし (単調)");
        assert!(b1 > 0);
    }

    #[test]
    fn add_boost_accumulates_for_medium() {
        let mut e = FrequencyEntry::new("様", "よう");
        let base = e.score();
        e.add_boost(MEDIUM_STEP);
        e.add_boost(MEDIUM_STEP);
        assert_eq!(e.score(), base + 2 * MEDIUM_STEP, "中は固定ステップで加算");
    }

    #[test]
    fn learning_strength_default_is_strong() {
        assert_eq!(LearningStrength::default(), LearningStrength::Strong);
    }
}
