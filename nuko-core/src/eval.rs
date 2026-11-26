//! 変換精度ベンチの純粋ロジック（モデル不要・CI でテスト可能）。
//!
//! 「数千件のロングテール」を whack-a-mole でなく**計測**で相手にするための土台
//! （Gemini レビュー C 軸）。reading→expected の Golden Set に対し Top-1/Top-3/Top-5/MRR と
//! **Lexical Purity@5**（上位 5 候補に実在語が占める割合＝候補キュレーションの効果が測れる独自指標）
//! を出す。実際の変換呼び出し（libakaza 要）は CLI 側（`nuko eval`）が行い、ここは**集計の純粋関数**
//! だけを持つ。数値主張はテストで pin（[[nuko-ime-comment-code-drift]] の実践）。

// 件数 (usize) を割合計算で f64 にするだけ。Golden Set は高々数千件で 2^52 を超えず精度損失は無い。
#![allow(clippy::cast_precision_loss)]

/// 表層が「実在語」か（= 漢字・英字など、かな以外の文字を 1 つ以上含む）。
///
/// 全てがひらがな/カタカナ/半角カナ/長音だけの表層（にんい/ニンイ/ﾆﾝｲ 等の機械生成の疑似語、
/// および純カタカナ外来語 リアクト）は false。Lexical Purity の「実在語」判定に使う。
/// 注: 外来語カタカナ（リアクト）も false になるが、Purity は「漢字変換がどれだけ上位を占めるか」の
/// 近似指標なので、この定義で足りる（厳密な語彙判定ではない）。
#[must_use]
pub fn is_real_word(surface: &str) -> bool {
    surface.chars().any(|c| {
        !matches!(c,
            '\u{3040}'..='\u{309F}'    // ひらがな
            | '\u{30A0}'..='\u{30FF}'  // カタカナ（長音符含む）
            | '\u{FF61}'..='\u{FF9F}'  // 半角カナ
        )
    })
}

/// 候補表層の列の中で `expected` が何位か（1-based）。無ければ `None`。
#[must_use]
pub fn rank_of(surfaces: &[String], expected: &str) -> Option<usize> {
    surfaces.iter().position(|s| s == expected).map(|i| i + 1)
}

/// 上位 `n` 候補のうち実在語が占める割合（0.0〜1.0）。候補が無ければ 0.0。
#[must_use]
pub fn lexical_purity_at(surfaces: &[String], n: usize) -> f64 {
    let top = &surfaces[..surfaces.len().min(n)];
    if top.is_empty() {
        return 0.0;
    }
    let real = top.iter().filter(|s| is_real_word(s)).count();
    real as f64 / top.len() as f64
}

/// 1 件分の評価結果。
#[derive(Debug, Clone)]
pub struct CaseResult {
    /// 入力の読み（ひらがな）。
    pub reading: String,
    /// 期待する Top-1 表層。
    pub expected: String,
    /// expected の順位（1-based）。候補に無ければ `None`。
    pub rank: Option<usize>,
    /// 上位 5 候補の Lexical Purity。
    pub purity_at5: f64,
    /// 実際の上位候補（レポートの miss 表示用、先頭数件）。
    pub top: Vec<String>,
}

impl CaseResult {
    /// reading / expected と、変換で得た候補表層列から 1 件の結果を作る。
    #[must_use]
    pub fn evaluate(reading: &str, expected: &str, surfaces: &[String]) -> Self {
        Self {
            reading: reading.to_string(),
            expected: expected.to_string(),
            rank: rank_of(surfaces, expected),
            purity_at5: lexical_purity_at(surfaces, 5),
            top: surfaces.iter().take(5).cloned().collect(),
        }
    }

    /// expected が `n` 位以内か。
    #[must_use]
    pub fn in_top(&self, n: usize) -> bool {
        matches!(self.rank, Some(r) if r <= n)
    }

    /// Reciprocal Rank（1 位=1.0, 2 位=0.5, …, 圏外=0.0）。
    #[must_use]
    pub fn reciprocal_rank(&self) -> f64 {
        match self.rank {
            Some(r) => 1.0 / r as f64,
            None => 0.0,
        }
    }
}

/// Golden Set 全体の集計。
#[derive(Debug, Default, Clone)]
pub struct EvalSummary {
    /// 評価した総件数。
    pub total: usize,
    /// expected が 1 位だった件数。
    pub top1: usize,
    /// expected が 3 位以内だった件数。
    pub top3: usize,
    /// expected が 5 位以内だった件数。
    pub top5: usize,
    mrr_sum: f64,
    purity_sum: f64,
}

impl EvalSummary {
    /// 空の集計を作る。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 1 件の結果を取り込む。
    pub fn add(&mut self, case: &CaseResult) {
        self.total += 1;
        if case.in_top(1) {
            self.top1 += 1;
        }
        if case.in_top(3) {
            self.top3 += 1;
        }
        if case.in_top(5) {
            self.top5 += 1;
        }
        self.mrr_sum += case.reciprocal_rank();
        self.purity_sum += case.purity_at5;
    }

    fn ratio(n: usize, total: usize) -> f64 {
        if total == 0 {
            0.0
        } else {
            n as f64 / total as f64
        }
    }

    /// Top-1 正解率（0.0〜1.0）。
    #[must_use]
    pub fn top1_rate(&self) -> f64 {
        Self::ratio(self.top1, self.total)
    }
    /// Top-3 正解率（0.0〜1.0）。
    #[must_use]
    pub fn top3_rate(&self) -> f64 {
        Self::ratio(self.top3, self.total)
    }
    /// Top-5 正解率（0.0〜1.0）。
    #[must_use]
    pub fn top5_rate(&self) -> f64 {
        Self::ratio(self.top5, self.total)
    }
    /// Mean Reciprocal Rank。
    #[must_use]
    pub fn mrr(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.mrr_sum / self.total as f64
        }
    }
    /// 平均 Lexical Purity@5。
    #[must_use]
    pub fn mean_purity_at5(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.purity_sum / self.total as f64
        }
    }
}

// ============================================================================
// 文節境界 (segmentation) の評価
// ============================================================================
//
// 「表層 Top-1」とは別軸で、**どこで切るか**の精度を測る。表層は惜しいが区切り/語選択で
// 損しているケース (例: きょうはいいてんき → 今日/は/いい/転記) を独立に数値化する。
// 指標は単語/文節分割の定番である **境界 P/R/F1** (micro 平均) と **文完全一致率**。
//
// 境界は「文節と文節のあいだの位置」を**文字数**の累積で表す (バイト数でなく文字数=人が読める
// 単位で、全角幅にも依存しない)。例: ["きょう","は","いい","てんき"] → 文字長 [3,1,2,3] →
// 内部境界 {3,4,6} (0 と末尾 10 は含めない)。この集合を予測と正解で比べる。

/// 文節読み列から**内部境界**の位置集合 (文字数の累積, 昇順) を作る。
///
/// 末尾 (= 全文長) と先頭 (0) は含めない。1 文節だけなら空ベクタ (内部境界なし)。
#[must_use]
pub fn segmentation_boundaries(segment_readings: &[&str]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut acc = 0usize;
    // 最後の文節のあとは末尾境界なので含めない。
    for s in segment_readings
        .iter()
        .take(segment_readings.len().saturating_sub(1))
    {
        acc += s.chars().count();
        out.push(acc);
    }
    out
}

/// 予測境界と正解境界から (TP, FP, FN) を数える。
///
/// TP=両方にある境界, FP=予測だけ (余計に切った), FN=正解だけ (切り損ねた)。
#[must_use]
pub fn boundary_counts(pred: &[usize], gold: &[usize]) -> (usize, usize, usize) {
    use std::collections::BTreeSet;
    let p: BTreeSet<usize> = pred.iter().copied().collect();
    let g: BTreeSet<usize> = gold.iter().copied().collect();
    let tp = p.intersection(&g).count();
    let fp = p.difference(&g).count();
    let fn_ = g.difference(&p).count();
    (tp, fp, fn_)
}

/// (TP, FP, FN) から Precision / Recall / F1 を出す。
///
/// 分母が 0 の扱い: 予測境界が無い (TP+FP=0) なら P=1.0、正解境界が無い (TP+FN=0) なら R=1.0
/// (「切る所が無いのを正しく切らなかった」= 完璧)。P+R=0 のときだけ F1=0。
#[must_use]
pub fn prf(tp: usize, fp: usize, fn_: usize) -> (f64, f64, f64) {
    let p = if tp + fp == 0 {
        1.0
    } else {
        tp as f64 / (tp + fp) as f64
    };
    let r = if tp + fn_ == 0 {
        1.0
    } else {
        tp as f64 / (tp + fn_) as f64
    };
    let f1 = if p + r == 0.0 {
        0.0
    } else {
        2.0 * p * r / (p + r)
    };
    (p, r, f1)
}

/// 正解仕様文字列 (`きょう|は|いい|てんき`) を文節読み列に割る。`|` 区切り・各要素 trim・空要素は除く。
#[must_use]
pub fn split_gold_segments(spec: &str) -> Vec<String> {
    spec.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// 文節境界の 1 件分の評価結果。
#[derive(Debug, Clone)]
pub struct SegCaseResult {
    /// 入力の読み (ひらがな)。
    pub reading: String,
    /// 正解の文節読み列。
    pub gold_segs: Vec<String>,
    /// 予測の文節読み列 (レポートの miss 表示用)。
    pub pred_segs: Vec<String>,
    /// この件の TP / FP / FN (micro 集計用)。
    pub tp: usize,
    /// FP (余計に切った境界数)。
    pub fp: usize,
    /// FN (切り損ねた境界数)。
    pub fn_: usize,
    /// 境界集合が完全一致したか (= 文として区切りが完璧)。
    pub exact: bool,
}

impl SegCaseResult {
    /// 正解と予測の文節読み列から 1 件を評価する。
    #[must_use]
    pub fn evaluate(reading: &str, gold_segs: &[String], pred_segs: &[String]) -> Self {
        let gold_refs: Vec<&str> = gold_segs.iter().map(String::as_str).collect();
        let pred_refs: Vec<&str> = pred_segs.iter().map(String::as_str).collect();
        let gold_b = segmentation_boundaries(&gold_refs);
        let pred_b = segmentation_boundaries(&pred_refs);
        let (tp, fp, fn_) = boundary_counts(&pred_b, &gold_b);
        Self {
            reading: reading.to_string(),
            gold_segs: gold_segs.to_vec(),
            pred_segs: pred_segs.to_vec(),
            tp,
            fp,
            fn_,
            exact: fp == 0 && fn_ == 0,
        }
    }
}

/// 文節境界の Golden Set 全体の集計 (micro 平均 P/R/F1 + 完全一致率)。
#[derive(Debug, Default, Clone)]
pub struct SegEvalSummary {
    /// 評価した総件数。
    pub total: usize,
    /// 境界が完全一致した件数。
    pub exact: usize,
    tp_sum: usize,
    fp_sum: usize,
    fn_sum: usize,
}

impl SegEvalSummary {
    /// 空の集計を作る。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 1 件の結果を取り込む。
    pub fn add(&mut self, case: &SegCaseResult) {
        self.total += 1;
        if case.exact {
            self.exact += 1;
        }
        self.tp_sum += case.tp;
        self.fp_sum += case.fp;
        self.fn_sum += case.fn_;
    }

    /// micro 平均の (Precision, Recall, F1)。全件の TP/FP/FN を合算して出す。
    #[must_use]
    pub fn micro_prf(&self) -> (f64, f64, f64) {
        prf(self.tp_sum, self.fp_sum, self.fn_sum)
    }

    /// 文完全一致率 (0.0〜1.0)。
    #[must_use]
    pub fn exact_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.exact as f64 / self.total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces(ss: &[&str]) -> Vec<String> {
        ss.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn is_real_word_detects_kanji_vs_pseudo() {
        assert!(is_real_word("任意"));
        assert!(is_real_word("Re・Act"));
        assert!(!is_real_word("にんい"));
        assert!(!is_real_word("ニンイ"));
        assert!(!is_real_word("ﾆﾝｲ"));
        assert!(
            !is_real_word("リアクト"),
            "純カタカナ外来語は purity 上は非実在語扱い"
        );
        assert!(!is_real_word(""));
    }

    #[test]
    fn rank_of_is_one_based() {
        let s = surfaces(&["任意", "任に", "忍に"]);
        assert_eq!(rank_of(&s, "任意"), Some(1));
        assert_eq!(rank_of(&s, "忍に"), Some(3));
        assert_eq!(rank_of(&s, "存在しない"), None);
    }

    #[test]
    fn lexical_purity_counts_real_words_in_top_n() {
        // 任意 のみ実在語、残りは疑似語 → 1/5。
        let s = surfaces(&["任意", "にんに", "ニンニ", "ﾆﾝﾆ", "ニンイ"]);
        assert!((lexical_purity_at(&s, 5) - 0.2).abs() < 1e-9);
        // キュレーション後: 実在語が上位 → 3/3。
        let s2 = surfaces(&["任意", "任に", "忍に"]);
        assert!((lexical_purity_at(&s2, 5) - 1.0).abs() < 1e-9);
        // 候補が n 未満でも割合は実際の件数基準。
        assert!((lexical_purity_at(&surfaces(&["任意"]), 5) - 1.0).abs() < 1e-9);
        assert!((lexical_purity_at(&[], 5) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn reciprocal_rank_and_summary_rates() {
        let c1 = CaseResult::evaluate("にんい", "任意", &surfaces(&["任意", "任い"]));
        let c2 = CaseResult::evaluate("ようい", "用意", &surfaces(&["容易", "用意", "要意"])); // 2位
        let c3 = CaseResult::evaluate("むり", "無理", &surfaces(&["ムリ", "むり"])); // 圏外

        assert_eq!(c1.rank, Some(1));
        assert_eq!(c2.rank, Some(2));
        assert_eq!(c3.rank, None);
        assert!((c2.reciprocal_rank() - 0.5).abs() < 1e-9);
        assert!((c3.reciprocal_rank() - 0.0).abs() < 1e-9);

        let mut sum = EvalSummary::new();
        for c in [&c1, &c2, &c3] {
            sum.add(c);
        }
        assert_eq!(sum.total, 3);
        assert_eq!(sum.top1, 1); // c1 のみ
        assert_eq!(sum.top3, 2); // c1, c2
                                 // MRR = (1 + 0.5 + 0) / 3
        assert!((sum.mrr() - (1.5 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn boundaries_are_char_cumulative_interior_only() {
        // ["きょう","は","いい","てんき"] 文字長 [3,1,2,3] → 内部境界 {3,4,6}。
        assert_eq!(
            segmentation_boundaries(&["きょう", "は", "いい", "てんき"]),
            vec![3, 4, 6]
        );
        // 1 文節だけなら内部境界なし。
        assert_eq!(
            segmentation_boundaries(&["きょうはいいてんき"]),
            Vec::<usize>::new()
        );
        // 空入力も空。
        assert_eq!(segmentation_boundaries(&[]), Vec::<usize>::new());
    }

    #[test]
    fn boundary_counts_tp_fp_fn() {
        // 正解 {3,4,6}、予測 {3,6,9}: TP={3,6}=2, FP={9}=1, FN={4}=1。
        let (tp, fp, fn_) = boundary_counts(&[3, 6, 9], &[3, 4, 6]);
        assert_eq!((tp, fp, fn_), (2, 1, 1));
    }

    #[test]
    fn prf_edge_cases() {
        // 完全一致 (TP=3,FP=0,FN=0) → P=R=F1=1。
        let (p, r, f) = prf(3, 0, 0);
        assert!((p - 1.0).abs() < 1e-9 && (r - 1.0).abs() < 1e-9 && (f - 1.0).abs() < 1e-9);
        // 境界ゼロ同士 (切る所が無いのを正しく切らなかった) → P=R=F1=1。
        let (p, r, f) = prf(0, 0, 0);
        assert!((p - 1.0).abs() < 1e-9 && (r - 1.0).abs() < 1e-9 && (f - 1.0).abs() < 1e-9);
        // TP=2,FP=1,FN=1 → P=R=2/3, F1=2/3。
        let (p, r, f) = prf(2, 1, 1);
        assert!((p - 2.0 / 3.0).abs() < 1e-9);
        assert!((r - 2.0 / 3.0).abs() < 1e-9);
        assert!((f - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn split_gold_segments_splits_trims_filters() {
        assert_eq!(
            split_gold_segments("きょう|は|いい|てんき"),
            surfaces(&["きょう", "は", "いい", "てんき"])
        );
        // 余分な空白・空要素は落とす。
        assert_eq!(
            split_gold_segments(" きょう |は|| てんき "),
            surfaces(&["きょう", "は", "てんき"])
        );
    }

    #[test]
    fn seg_case_and_summary() {
        let gold = surfaces(&["きょう", "は", "いい", "てんき"]); // 境界 {3,4,6}
                                                                  // 完全一致のケース。
        let exact_case = SegCaseResult::evaluate("きょうはいいてんき", &gold, &gold);
        assert!(exact_case.exact);
        assert_eq!((exact_case.tp, exact_case.fp, exact_case.fn_), (3, 0, 0));

        // 「今日/はいい/転記」相当 (きょう|はいい|てんき) 境界 {3,6}: TP={3,6}=2, FP=0, FN={4}=1。
        let pred = surfaces(&["きょう", "はいい", "てんき"]);
        let miss_case = SegCaseResult::evaluate("きょうはいいてんき", &gold, &pred);
        assert!(!miss_case.exact);
        assert_eq!((miss_case.tp, miss_case.fp, miss_case.fn_), (2, 0, 1));

        let mut sum = SegEvalSummary::new();
        sum.add(&exact_case);
        sum.add(&miss_case);
        assert_eq!(sum.total, 2);
        assert_eq!(sum.exact, 1);
        assert!((sum.exact_rate() - 0.5).abs() < 1e-9);
        // micro: TP=5, FP=0, FN=1 → P=1.0, R=5/6, F1=2*1*(5/6)/(1+5/6)=10/11。
        let (p, r, f) = sum.micro_prf();
        assert!((p - 1.0).abs() < 1e-9);
        assert!((r - 5.0 / 6.0).abs() < 1e-9);
        assert!((f - 10.0 / 11.0).abs() < 1e-9);
    }
}
