//! 頻度学習

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// 学習の「強さ」= 候補を選んだときスコアをどれだけ上げるか (= 学習モード)。
///
/// 可視化で「1 回選んでも +10 しか上がらず、Layer2 で底上げされた rival
/// (例 `用` ~300k) に永遠に勝てない」と判明した (2026-11-14、実機 + `--explain`)。
/// そこで「上げ幅」をユーザー設定にする。設計判断は docs/LEARNING_ARCHITECTURE.md §1.6。
///
/// ## モードの系譜 (なぜ 4 つあるか)
///
/// - 当初の `Strong` (追い越し・単調増加) は「選んだら今のトップより上へ恒久加点」
///   する設計思想 (森岡さん) だった。だが **単調増加 × 相手の“ブースト済み”スコアを
///   追う** ため、`用`/`様` を交互確定すると両方が青天井に競り上がり実機で 500k〜800k
///   まで膨張、打ち直し訂正(300k)でも勝てない悪循環を生んだ (2026-11)。
/// - 根治として **`Crown` (王冠・既定)** を追加。確定した表層へ**上限付き**のブーストを与え、
///   同じ読みの**他表層の王冠は剥奪**する (= 王冠を持てるのは「最後に確定した 1 つ」だけ)。
///   非単調 + 剥奪なので交互確定でも競り上がらず、訂正tier(300k)**未満**に収まる
///   (打ち直し訂正は王冠より強い、という階層を保つ)。
/// - `Strong` は「累積(集合知的に上がる従来挙動)」を好むユーザー向けに**選択肢として残す**
///   (森岡さん「単純上書きでなく集合知として上がるパターンも選べると良い」2026-11-21)。
///
/// 減衰する recency ボーナスは依然**不採用** (「さっき上に出たのに戻ってる!?」= 非単調で混乱)。
/// 王冠の非単調は**絶対値**が下がり得るだけで**順位は常に 1 位**なので、この混乱は起きない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LearningStrength {
    /// 頻度のみ (1 選択 = 使用回数 +1 = スコア +10)。確定でブーストしない。
    Weak,
    /// 段階的に大きく上げる (1 選択ごとに固定ステップ加点)。数回で逆転。
    Medium,
    /// 累積・追い越し (今の最上位を恒久的に追い越す・単調増加)。集合知的に上がるが青天井。
    Strong,
    /// 王冠 (既定): 確定した 1 つだけへ**上限付き**ブースト。他表層の王冠は剥奪。
    /// 暴走せず、打ち直し訂正(300k)より下に収まる。
    #[default]
    Crown,
}

/// 「中」1 選択あたりの固定ブースト加点。数回で高スコア rival も抜ける大きさ。
pub const MEDIUM_STEP: i32 = 20_000;
/// 追い越し時に相手へ乗せる余裕 (同点負けを防ぐ)。王冠でも同じ余裕を使う。
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

    /// 追い越しブーストを取り除いて**降格**する (score を頻度ベース 200_000+count*10 へ戻す)。
    ///
    /// 追い越しブーストは単調増加なので、間違って確定し続けた表層が訂正tier(300_000)を
    /// 超えて膨張し (実機で 500_000〜800_000)、打ち直し訂正でも勝てなくなる悪循環を生む
    /// (2026-11 森岡さん報告)。打ち直し / 明示的な降格操作 = 「この表層は誤り」という
    /// 明確なシグナルのときだけ、**単調増加の例外**として boost を 0 に戻す。
    ///
    /// `count` は保持する (頻度=使用実績は失わない)。boost を消すだけなので、訂正
    /// (Layer2, +300_000) を乗せた正の表層が確実に上に出る。既に 0 なら no-op。
    ///
    /// 戻り値: 降格した (boost が 0 でなかった) なら `true`。
    pub fn demote(&mut self) -> bool {
        if self.boost != 0 {
            self.boost = 0;
            true
        } else {
            false
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

    /// **王冠**を与える: score が `target + margin` になるよう boost を設定する
    /// (「王冠」= `LearningStrength::Crown`。確定した 1 つだけが持つ上限付きブースト)。
    ///
    /// `boost_to_at_least` との違いは 2 点、どちらも**暴走を断つ**ためにある:
    /// 1. **非単調** (set であって add でない)。相手が低ければ boost は下がり得る。これにより
    ///    `用`/`様` を交互確定しても、毎回 strip 済みの相手 base を目標に**再設定**するだけで
    ///    青天井に競り上がらない。順位は常に相手の上 = 1 位なので「戻った」感は出ない。
    /// 2. **`ceiling` でクランプ**。score が `ceiling` 以上にならない。呼び出し側は訂正tier
    ///    (`correction::CORRECTION_BOOST`) を渡し、**打ち直し訂正は王冠より強い**階層を保つ。
    ///
    /// `target` には「同じ読みの他表層の最大 score (王冠剥奪後の base)」を渡す。相手が居ない
    /// (target が自分の base 以下) なら boost は 0 = 頻度ベースのまま (それでも libakaza には勝つ)。
    pub fn set_crown(&mut self, target: i32, margin: i32, ceiling: i32) {
        let desired = target.saturating_add(margin).min(ceiling.saturating_sub(1));
        // boost を除いた素点 (= 200_000 + count*10 − 減衰)。score() = base + boost。
        let base = self.score().saturating_sub(self.boost);
        self.boost = desired.saturating_sub(base).max(0);
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

    /// ★ 公式の pin（実行可能ドキュメント・コメント乖離の防止）。
    ///
    /// `score() = 200_000 + count*10 − 減衰 + boost`。**乗数や基準値を変えると
    /// ここが赤くなる**ので、コメント/ドキュメントに書いた数値が実装と食い違ったまま
    /// 放置されない (2026-11、engine.rs のコメントが `count*50` と誤記されていた再発防止。
    /// 詳細は docs/CODE_COMMENT_DISCIPLINE.md)。
    #[test]
    fn score_formula_is_pinned_count_times_10() {
        // 生成直後: count=1, boost=0, last_used=now なので減衰 0。
        let mut e = FrequencyEntry::new("ねこ", "ねこ");
        assert_eq!(e.score(), 200_010, "count=1 → 200_000 + 1*10");
        for _ in 0..4 {
            e.increment(); // increment は last_used も now に更新するので減衰は 0 のまま
        }
        assert_eq!(
            e.score(),
            200_050,
            "count=5 → 200_000 + 5*10 (= 200_050)。×50 なら 200_250 になるが、それは誤り"
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
    fn learning_strength_default_is_crown() {
        // 既定は王冠 (上限付き・暴走なし)。旧既定 Strong は「累積」選択肢として残す。
        assert_eq!(LearningStrength::default(), LearningStrength::Crown);
    }

    #[test]
    fn set_crown_overtakes_rival_and_stays_below_ceiling() {
        // 様 (base 200_010) を rival(用 base 200_200) の上へ。天井 300_000 未満。
        let mut sama = FrequencyEntry::new("様", "よう");
        let ceiling = crate::learning::correction::CORRECTION_BOOST; // 300_000
        sama.set_crown(200_200, OVERTAKE_MARGIN, ceiling);
        assert_eq!(sama.score(), 200_300, "rival base + margin へ乗る");
        assert!(
            sama.score() < ceiling,
            "訂正tier未満 (打ち直し訂正は王冠より強い)"
        );
    }

    #[test]
    fn set_crown_is_clamped_by_ceiling() {
        // 相手が訂正tierを超える高スコアでも、王冠は天井-1 までしか上がらない。
        let mut e = FrequencyEntry::new("用", "よう");
        let ceiling = crate::learning::correction::CORRECTION_BOOST;
        e.set_crown(999_999, OVERTAKE_MARGIN, ceiling);
        assert_eq!(
            e.score(),
            ceiling - 1,
            "天井でクランプ (訂正tierは超えない)"
        );
    }

    #[test]
    fn set_crown_is_nonmonotonic_no_ratchet() {
        // 非単調: 大きい rival で一度上げ、その後小さい rival で再設定すると下がる
        // (= 交互確定で青天井に競り上がらない)。順位は常に rival の上。
        let mut e = FrequencyEntry::new("様", "よう");
        let ceiling = crate::learning::correction::CORRECTION_BOOST;
        e.set_crown(250_000, OVERTAKE_MARGIN, ceiling);
        assert_eq!(e.score(), 250_100);
        e.set_crown(200_200, OVERTAKE_MARGIN, ceiling); // 相手が下がった
        assert_eq!(e.score(), 200_300, "非単調で下がる (ratchet しない)");
    }

    #[test]
    fn set_crown_no_rival_keeps_frequency_base() {
        // rival が自分の base 以下なら boost 0 = 頻度ベース (libakaza には base で勝つ)。
        let mut e = FrequencyEntry::new("猫", "ねこ"); // base 200_010
        let ceiling = crate::learning::correction::CORRECTION_BOOST;
        e.set_crown(100, OVERTAKE_MARGIN, ceiling);
        assert_eq!(e.boost, 0, "相手が居なければブーストしない");
        assert_eq!(e.score(), 200_010);
    }

    #[test]
    fn demote_removes_runaway_boost_back_to_frequency_base() {
        // 暴走した表層 (追い越しで 800_000 まで膨張) を降格 → 頻度ベースへ戻る。
        let mut e = FrequencyEntry::new("用", "よう");
        e.boost_to_at_least(800_000, OVERTAKE_MARGIN); // 実機の暴走を模擬
        assert!(e.score() > 300_000, "降格前は訂正tierを超えて膨張");

        assert!(e.demote(), "boost があれば降格した = true");
        // count=1 なので base + 10。暴走が消え、訂正 (300_000) に確実に負ける。
        assert_eq!(e.boost, 0, "boost は 0 に戻る");
        assert!(
            e.score() < 300_000,
            "降格後は訂正tier (300_000) 未満 = 打ち直し訂正が勝てる (score={})",
            e.score()
        );
    }

    #[test]
    fn demote_keeps_count_and_is_noop_when_not_boosted() {
        let mut e = FrequencyEntry::new("用", "よう");
        e.increment(); // count=2
        assert!(!e.demote(), "boost が無ければ no-op = false");
        assert_eq!(e.count, 2, "count (使用実績) は降格で失わない");
    }
}
