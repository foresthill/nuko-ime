//! 文節別変換結果の型定義
//!
//! libakaza は変換結果を「文節 (segment) × 候補 (candidate)」の二段構造で返す
//! (`Vec<Vec<Candidate>>`)。このモジュールはそれを ぬこIME 側の型として保持し、
//! 候補ウィンドウ表示 (Phase 1.3 Step 2) や文節境界の編集 (Step 3) の基盤を提供する。
//!
//! 設計判断は `docs/spikes/libakaza-api-survey.md` §4.4 案 B に対応する。
//!
//! ## 静的辞書フォールバックでの扱い
//!
//! libakaza バックエンド不在時は `ConversionEngine::convert_segmented()` が `None`
//! を返す契約にし、本モジュールでは静的辞書だけのケースを扱わない。
//! プラットフォーム層は `None` の場合にだけ既存の `CandidateList` ベースのフローへ
//! フォールバックすればよい。

use super::Candidate;

/// 1 文節分の変換情報
///
/// libakaza の 1 segment に対応。候補は libakaza の cost 順 (= 先頭が最良)
/// で格納される前提。
#[derive(Debug, Clone)]
pub struct Segment {
    /// 文節の読み (ひらがな)
    pub reading: String,
    /// 候補リスト (先頭が最良)
    pub candidates: Vec<Candidate>,
    /// 現在選択中の候補 index
    pub selected: usize,
}

impl Segment {
    /// 新しい文節を作成。`selected` は 0 で初期化される。
    #[must_use]
    pub fn new(reading: impl Into<String>, candidates: Vec<Candidate>) -> Self {
        Self {
            reading: reading.into(),
            candidates,
            selected: 0,
        }
    }

    /// 候補数
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// 候補が空か
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// 現在選択中の候補
    #[must_use]
    pub fn current(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    /// 現在選択中の候補の surface
    #[must_use]
    pub fn surface(&self) -> Option<&str> {
        self.current().map(|c| c.surface.as_str())
    }

    /// `index` 番目の候補を選択 (範囲外なら何もしない)
    pub fn select(&mut self, index: usize) {
        if index < self.candidates.len() {
            self.selected = index;
        }
    }

    /// 次の候補に進む (末尾なら先頭に戻る)
    pub fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = (self.selected + 1) % self.candidates.len();
        }
    }

    /// 前の候補に戻る (先頭なら末尾に戻る)
    pub fn select_prev(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = if self.selected == 0 {
                self.candidates.len() - 1
            } else {
                self.selected - 1
            };
        }
    }
}

/// 文節分割された変換結果
///
/// 1 回の `convert` 呼び出しに対応。`focused` は文節境界編集 (Phase 1.3 Step 3)
/// で「いまどの文節を操作中か」を保持するためのカーソル。
#[derive(Debug, Clone, Default)]
pub struct SegmentedConversion {
    /// 文節リスト (左から右への順)
    pub segments: Vec<Segment>,
    /// 現在フォーカス中の文節 index
    pub focused: usize,
}

impl SegmentedConversion {
    /// 文節リストから新規作成。`focused` は 0 で初期化される。
    #[must_use]
    pub fn new(segments: Vec<Segment>) -> Self {
        Self {
            segments,
            focused: 0,
        }
    }

    /// 文節数
    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// 文節が空か
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// 全文節の選択中候補を連結した surface
    #[must_use]
    pub fn current_surface(&self) -> String {
        self.segments
            .iter()
            .filter_map(Segment::surface)
            .collect::<Vec<_>>()
            .concat()
    }

    /// 全文節の読みを連結
    #[must_use]
    pub fn reading(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.reading.as_str())
            .collect::<Vec<_>>()
            .concat()
    }

    /// フォーカス中の文節
    #[must_use]
    pub fn focused_segment(&self) -> Option<&Segment> {
        self.segments.get(self.focused)
    }

    /// フォーカス中の文節 (可変)
    pub fn focused_segment_mut(&mut self) -> Option<&mut Segment> {
        self.segments.get_mut(self.focused)
    }

    /// `index` 番目の文節にフォーカスを移す (範囲外なら何もしない)
    pub fn focus(&mut self, index: usize) {
        if index < self.segments.len() {
            self.focused = index;
        }
    }

    /// 次の文節にフォーカスを移す (末尾なら先頭に戻る)
    pub fn focus_next(&mut self) {
        if !self.segments.is_empty() {
            self.focused = (self.focused + 1) % self.segments.len();
        }
    }

    /// 前の文節にフォーカスを移す (先頭なら末尾に戻る)
    pub fn focus_prev(&mut self) {
        if !self.segments.is_empty() {
            self.focused = if self.focused == 0 {
                self.segments.len() - 1
            } else {
                self.focused - 1
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::CandidateSource;

    fn cand(surface: &str, reading: &str, score: i32) -> Candidate {
        Candidate::new(surface, reading)
            .with_score(score)
            .with_source(CandidateSource::System)
    }

    fn sample_segment() -> Segment {
        Segment::new(
            "にほん",
            vec![
                cand("日本", "にほん", 100),
                cand("二本", "にほん", 80),
                cand("にほん", "にほん", -100),
            ],
        )
    }

    #[test]
    fn segment_basic_accessors() {
        let s = sample_segment();
        assert_eq!(s.len(), 3);
        assert!(!s.is_empty());
        assert_eq!(s.surface(), Some("日本"));
        assert_eq!(s.current().map(|c| c.score), Some(100));
    }

    #[test]
    fn segment_select_in_range() {
        let mut s = sample_segment();
        s.select(2);
        assert_eq!(s.surface(), Some("にほん"));
    }

    #[test]
    fn segment_select_out_of_range_is_noop() {
        let mut s = sample_segment();
        s.select(99);
        assert_eq!(s.selected, 0, "範囲外は selected を変更しない");
    }

    #[test]
    fn segment_select_next_wraps() {
        let mut s = sample_segment();
        s.select_next();
        assert_eq!(s.selected, 1);
        s.select_next();
        s.select_next();
        assert_eq!(s.selected, 0, "末尾の次は先頭に戻る");
    }

    #[test]
    fn segment_select_prev_wraps() {
        let mut s = sample_segment();
        s.select_prev();
        assert_eq!(s.selected, 2, "先頭の前は末尾");
    }

    #[test]
    fn segment_select_on_empty_is_noop() {
        let mut s = Segment::new("にほん", Vec::new());
        assert!(s.is_empty());
        assert_eq!(s.current(), None);
        s.select_next();
        s.select_prev();
        assert_eq!(s.selected, 0);
    }

    fn sample_segmented() -> SegmentedConversion {
        SegmentedConversion::new(vec![
            Segment::new("わたし", vec![cand("私", "わたし", 100)]),
            Segment::new("の", vec![cand("の", "の", 100)]),
            Segment::new(
                "なまえ",
                vec![cand("名前", "なまえ", 100), cand("ナマエ", "なまえ", 50)],
            ),
        ])
    }

    #[test]
    fn segmented_current_surface_concatenates() {
        let sc = sample_segmented();
        assert_eq!(sc.current_surface(), "私の名前");
    }

    #[test]
    fn segmented_reading_concatenates() {
        let sc = sample_segmented();
        assert_eq!(sc.reading(), "わたしのなまえ");
    }

    #[test]
    fn segmented_focus_navigation() {
        let mut sc = sample_segmented();
        assert_eq!(sc.focused, 0);
        sc.focus_next();
        assert_eq!(sc.focused, 1);
        sc.focus_prev();
        sc.focus_prev();
        assert_eq!(sc.focused, 2, "先頭の前は末尾");
    }

    #[test]
    fn segmented_focus_out_of_range_is_noop() {
        let mut sc = sample_segmented();
        sc.focus(99);
        assert_eq!(sc.focused, 0);
    }

    #[test]
    fn segmented_focused_segment_mut_allows_inner_select() {
        let mut sc = sample_segmented();
        sc.focus(2);
        if let Some(seg) = sc.focused_segment_mut() {
            seg.select(1);
        }
        assert_eq!(sc.current_surface(), "私のナマエ");
    }

    #[test]
    fn segmented_empty_behaves_consistently() {
        let mut sc = SegmentedConversion::default();
        assert!(sc.is_empty());
        assert_eq!(sc.current_surface(), "");
        assert_eq!(sc.reading(), "");
        sc.focus_next();
        sc.focus_prev();
        assert_eq!(sc.focused, 0);
    }

    // -- データ消失防止リグレッションテスト ---------------------------------
    //
    // 2026-06-10 実機検証で「複数文節入力中、focused 文節の候補だけ確定 →
    // 残り segment が消失」する致命的バグが PR #49 で発生し PR #50/51 で
    // 修正された。**`current_surface()` が全 segment を連結すること** を
    // 不変条件として今後の regression を防ぐためのテスト群。

    #[test]
    fn segmented_current_surface_includes_all_segments_for_long_input() {
        // 「こうそくでうってると」: 5 文節相当 (= ユーザー報告のバグ再現入力に対応)
        let sc = SegmentedConversion::new(vec![
            Segment::new("こうそく", vec![cand("高速", "こうそく", 100)]),
            Segment::new("で", vec![cand("で", "で", 100)]),
            Segment::new("うって", vec![cand("打って", "うって", 100)]),
            Segment::new("る", vec![cand("る", "る", 100)]),
            Segment::new("と", vec![cand("と", "と", 100)]),
        ]);
        // ★ 全 segment が連結されることを保証 (= 焦点 0 でも「高速」だけにならない)
        assert_eq!(sc.current_surface(), "高速で打ってると");
        assert_eq!(
            sc.reading(),
            "こうそくでうってると",
            "reading も全 segment 連結"
        );
    }

    #[test]
    fn segmented_current_surface_changes_with_focused_selection() {
        // 焦点文節の候補を切替えても **他文節は維持** される
        let mut sc = SegmentedConversion::new(vec![
            Segment::new(
                "わたし",
                vec![cand("私", "わたし", 100), cand("渡し", "わたし", 80)],
            ),
            Segment::new("の", vec![cand("の", "の", 100)]),
            Segment::new(
                "なまえ",
                vec![cand("名前", "なまえ", 100), cand("ナマエ", "なまえ", 50)],
            ),
        ]);
        assert_eq!(sc.current_surface(), "私の名前");

        // 文節 0 で「渡し」選択 → 文節 1, 2 は維持
        sc.focus(0);
        if let Some(seg) = sc.focused_segment_mut() {
            seg.select(1);
        }
        assert_eq!(sc.current_surface(), "渡しの名前");

        // 文節 2 で「ナマエ」選択 → 文節 0, 1 は維持
        sc.focus(2);
        if let Some(seg) = sc.focused_segment_mut() {
            seg.select(1);
        }
        assert_eq!(sc.current_surface(), "渡しのナマエ");
    }

    #[test]
    fn segmented_select_in_one_segment_doesnt_lose_others() {
        // PR #49 のバグ再現: focused 文節の候補だけ commit する経路で残り消失。
        // この単体テストでは current_surface() 自体は常に全 segment 連結を返す
        // ことを保証する (= 呼び出し側の commit ロジックも信頼可)。
        let mut sc = SegmentedConversion::new(vec![
            Segment::new(
                "よろしく",
                vec![
                    cand("宜しく", "よろしく", 100),
                    cand("よろしく", "よろしく", 80),
                ],
            ),
            Segment::new(
                "おねがい",
                vec![
                    cand("お願い", "おねがい", 100),
                    cand("おねがい", "おねがい", 80),
                ],
            ),
            Segment::new(
                "いたします",
                vec![
                    cand("致します", "いたします", 100),
                    cand("いたします", "いたします", 80),
                ],
            ),
        ]);

        // 初期: 全 segment 先頭で連結
        assert_eq!(sc.current_surface(), "宜しくお願い致します");

        // 文節 2 の候補を切替えても文節 0, 1 は変わらず連結結果に含まれる
        sc.focus(2);
        if let Some(seg) = sc.focused_segment_mut() {
            seg.select(1);
        }
        assert_eq!(
            sc.current_surface(),
            "宜しくお願いいたします",
            "★ 文節 2 だけ変えても文節 0, 1 (= 宜しく / お願い) は維持される"
        );
    }

    #[test]
    fn segmented_focus_navigation_full_cycle() {
        // ← → 相当の動作で全 segment を回って戻る (wrap 動作)
        let mut sc = SegmentedConversion::new(vec![
            Segment::new("A", vec![cand("Α", "A", 100)]),
            Segment::new("B", vec![cand("Β", "B", 100)]),
            Segment::new("C", vec![cand("Γ", "C", 100)]),
        ]);

        // 右 (→) 3 回で先頭に戻る
        assert_eq!(sc.focused, 0);
        sc.focus_next();
        assert_eq!(sc.focused, 1);
        sc.focus_next();
        assert_eq!(sc.focused, 2);
        sc.focus_next();
        assert_eq!(sc.focused, 0, "末尾の次は先頭 (wrap)");

        // 左 (←) で末尾へ
        sc.focus_prev();
        assert_eq!(sc.focused, 2, "先頭の前は末尾 (wrap)");

        // 任意 focus 後も current_surface は全文節を返す
        sc.focus(1);
        assert_eq!(sc.current_surface(), "ΑΒΓ");
    }
}
