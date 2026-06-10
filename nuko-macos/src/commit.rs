//! テスト可能な commit 決定ロジック (テスト基盤の第 1 弾)
//!
//! ## 経緯
//!
//! ユーザー指摘 (2026-06-10):
//! - 「日本語が打てなくなった」「OSS だったら大事」
//! - 「テスト基盤を作ってほしい」「打てるか? からテスト」
//!
//! controller.rs は `IMKInputController` (NSResponder 系) を継承していて、
//! 直接的な unit test には NSEvent / NSObject の objc2 ランタイムが必要 = ハード。
//!
//! 本モジュールでは「commit 時に何のテキストを確定するか」「どの候補を学習対象
//! とするか」を **純粋関数** として切り出して、ObjC 不要で検証可能にする。
//!
//! ## 設計方針
//!
//! - 入力: `&InputState` (= データの snapshot)
//! - 出力: `CommitDecision` (= commit_text + learn_targets)
//! - 副作用: 一切なし (= state.reset / engine.commit / insert_text は呼び出し側で)
//!
//! controller.rs の `do_commit` / 数字 1-9 ハンドラ / 「他文字打鍵で auto-commit」
//! パスがすべてこの関数を使うように移行することで、データ消失バグ
//! (= focused 文節だけ commit して残り消える系) を unit test で catch できる。

use nuko_core::conversion::Candidate;

use crate::state::InputState;

/// commit 時に「何を確定するか」と「何を学習するか」を返すデータ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDecision {
    /// クライアントに `insertText:` で送る最終文字列
    pub commit_text: String,
    /// `engine.commit` で学習記録する Candidate 群 (順序保持)
    pub learn_targets: Vec<Candidate>,
}

impl CommitDecision {
    /// 何も commit するものが無い (= reset で済む) ケース
    #[must_use]
    pub fn empty() -> Self {
        Self {
            commit_text: String::new(),
            learn_targets: Vec::new(),
        }
    }
}

/// 現在の `InputState` から commit 決定を返す。
///
/// 優先順:
///
/// 1. **segmented モード**: `state.segmented` が `Some` なら、各文節の選択候補を
///    連結した全文を返す。学習は各文節の選択候補を個別に。
///    (= データ消失防止の本丸)
/// 2. **flat 候補モード**: `state.candidates` が `Some` で `selected()` がある
///    なら、その surface を commit。学習は selected を 1 件。
/// 3. **未変換のかなだけ**: `state.composition` (+ romaji buffer flush) を
///    そのまま commit。学習対象なし。
/// 4. **空**: 全て None / 空文字列なら `empty()` を返す。
///
/// `romaji` の buffer は **state を mut で受けないため flush しない**。
/// 呼び出し側で必要なら事前に flush してから本関数に渡すこと。
#[must_use]
pub fn decide_commit(state: &InputState) -> CommitDecision {
    // 1. segmented モード
    if let Some(segmented) = state.segmented.as_ref() {
        let commit_text = segmented.current_surface();
        let learn_targets: Vec<Candidate> = segmented
            .segments
            .iter()
            .filter_map(|s| s.current().cloned())
            .collect();
        return CommitDecision {
            commit_text,
            learn_targets,
        };
    }

    // 2. flat 候補モード
    if let Some(candidates) = state.candidates.as_ref() {
        if let Some(selected) = candidates.selected() {
            return CommitDecision {
                commit_text: selected.surface.clone(),
                learn_targets: vec![selected.clone()],
            };
        }
    }

    // 3. 未変換のかなだけ (= composition そのまま)
    //    romaji buffer の flush は呼び出し側責務 (本関数は state を mut しない)
    let text = state.composition.clone();
    if text.is_empty() {
        return CommitDecision::empty();
    }
    CommitDecision {
        commit_text: text,
        learn_targets: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    //! `decide_commit` の不変条件テスト群。
    //!
    //! ## 守りたい不変条件 (= 過去のバグから抽出)
    //!
    //! 1. **segmented モードでは全文節が必ず commit される** (PR #49→#51 で
    //!    発生したデータ消失バグの再発防止)
    //! 2. **focused 文節の候補切替が反映される** (= 学習意図が確定に乗る)
    //! 3. **flat 候補モードでは selected の surface が commit される**
    //! 4. **未変換のかなは composition がそのまま commit される**
    //! 5. **何も無いときは empty が返る**

    use super::*;
    use nuko_core::conversion::{
        Candidate, CandidateList, CandidateSource, ConversionContext, Segment, SegmentedConversion,
    };

    fn cand(surface: &str, reading: &str, score: i32) -> Candidate {
        Candidate::new(surface, reading)
            .with_score(score)
            .with_source(CandidateSource::System)
    }

    /// テスト用に空の `InputState` を作る
    fn empty_state() -> InputState {
        InputState::new()
    }

    /// テスト用に composition (未変換かな) だけの state を作る
    fn composing_state(composition: &str) -> InputState {
        let mut s = InputState::new();
        s.composition = composition.to_string();
        s.is_composing = true;
        s
    }

    /// テスト用に flat 候補がある state を作る
    fn flat_state(candidates: Vec<Candidate>) -> InputState {
        let mut s = InputState::new();
        let mut list = CandidateList::new();
        for c in candidates {
            list.push(c);
        }
        s.candidates = Some(list);
        s.composition = "にほん".to_string();
        s.is_composing = true;
        s
    }

    /// テスト用に segmented モードの state を作る
    fn segmented_state(segments: Vec<Segment>, focused: usize) -> InputState {
        let mut s = InputState::new();
        let mut sc = SegmentedConversion::new(segments);
        sc.focus(focused);
        s.segmented = Some(sc);
        s.composition = "わたしのなまえ".to_string();
        s.is_composing = true;
        s
    }

    // -- 不変条件 1: segmented モードでは全文節が必ず commit される --

    #[test]
    fn segmented_commits_full_sentence_with_initial_selections() {
        let state = segmented_state(
            vec![
                Segment::new("わたし", vec![cand("私", "わたし", 100)]),
                Segment::new("の", vec![cand("の", "の", 100)]),
                Segment::new("なまえ", vec![cand("名前", "なまえ", 100)]),
            ],
            0,
        );

        let d = decide_commit(&state);
        assert_eq!(
            d.commit_text, "私の名前",
            "★ 全 segment の選択候補が連結されて commit_text になる"
        );
        assert_eq!(d.learn_targets.len(), 3, "★ 各 segment の選択を個別に学習");
    }

    #[test]
    fn segmented_commits_full_sentence_even_when_focused_changed() {
        // 「よろしくお願いいたします」パターン: 焦点を最後の segment に動かして
        // その候補を切替えた場合、前半 segment が消失しないことを保証
        let mut state = segmented_state(
            vec![
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
            ],
            2, // 焦点 = 最後の segment
        );

        // 焦点 segment の候補を 2 番目に変える (= 「いたします」)
        if let Some(seg) = state
            .segmented
            .as_mut()
            .and_then(|s| s.focused_segment_mut())
        {
            seg.select(1);
        }

        let d = decide_commit(&state);
        assert_eq!(
            d.commit_text, "宜しくお願いいたします",
            "★ 焦点を動かして候補を切替えても前半 (宜しく / お願い) は維持"
        );
        assert_eq!(d.learn_targets.len(), 3);
    }

    #[test]
    fn segmented_long_input_no_data_loss() {
        // 「こうそくでうってると」パターン: 5 文節入力で 1 つ目だけ選択して commit
        // PR #49 のデータ消失バグ再現入力に対応
        let state = segmented_state(
            vec![
                Segment::new("こうそく", vec![cand("高速", "こうそく", 100)]),
                Segment::new("で", vec![cand("で", "で", 100)]),
                Segment::new("うって", vec![cand("打って", "うって", 100)]),
                Segment::new("る", vec![cand("る", "る", 100)]),
                Segment::new("と", vec![cand("と", "と", 100)]),
            ],
            0, // 焦点 = 最初の segment
        );

        let d = decide_commit(&state);
        assert_eq!(
            d.commit_text, "高速で打ってると",
            "★ 焦点が segment 0 (= 高速) でも全 segment 連結で commit"
        );
    }

    // -- 不変条件 2: focused 文節の候補切替が反映される --

    #[test]
    fn segmented_focused_selection_change_reflected_in_commit_text() {
        let mut state = segmented_state(
            vec![
                Segment::new(
                    "わたし",
                    vec![cand("私", "わたし", 100), cand("渡し", "わたし", 80)],
                ),
                Segment::new("の", vec![cand("の", "の", 100)]),
                Segment::new("なまえ", vec![cand("名前", "なまえ", 100)]),
            ],
            0,
        );

        // 初期状態
        assert_eq!(decide_commit(&state).commit_text, "私の名前");

        // 文節 0 の候補 1 (= 「渡し」) に変更
        if let Some(seg) = state
            .segmented
            .as_mut()
            .and_then(|s| s.focused_segment_mut())
        {
            seg.select(1);
        }
        assert_eq!(decide_commit(&state).commit_text, "渡しの名前");
    }

    // -- 不変条件 3: flat 候補モードでは selected の surface が commit される --

    #[test]
    fn flat_candidates_commit_selected_surface() {
        let state = flat_state(vec![
            cand("日本", "にほん", 100_000),
            cand("二本", "にほん", 99_000),
        ]);

        let d = decide_commit(&state);
        assert_eq!(d.commit_text, "日本", "★ selected = index 0 の surface");
        assert_eq!(d.learn_targets.len(), 1);
        assert_eq!(d.learn_targets[0].surface, "日本");
    }

    #[test]
    fn flat_candidates_select_next_changes_commit_text() {
        let mut state = flat_state(vec![
            cand("日本", "にほん", 100_000),
            cand("二本", "にほん", 99_000),
        ]);
        // selected を index 1 に
        if let Some(c) = state.candidates.as_mut() {
            c.select_next();
        }

        let d = decide_commit(&state);
        assert_eq!(
            d.commit_text, "二本",
            "★ select_next で commit_text が変わる"
        );
    }

    // -- 不変条件 4: 未変換のかなは composition がそのまま commit される --

    #[test]
    fn composing_only_kana_commits_as_is() {
        let state = composing_state("にほん");
        let d = decide_commit(&state);
        assert_eq!(d.commit_text, "にほん", "★ 未変換のかながそのまま");
        assert!(d.learn_targets.is_empty(), "学習対象なし");
    }

    // -- 不変条件 5: 何も無いときは empty --

    #[test]
    fn empty_state_returns_empty() {
        let state = empty_state();
        let d = decide_commit(&state);
        assert_eq!(d, CommitDecision::empty());
        assert!(d.commit_text.is_empty());
        assert!(d.learn_targets.is_empty());
    }

    // -- 優先順位の検証: segmented > candidates > composition --

    #[test]
    fn segmented_takes_priority_over_candidates() {
        // segmented と candidates の両方が Some の場合、segmented を優先 (= 全文 commit)
        let mut state = segmented_state(
            vec![Segment::new("わたし", vec![cand("私", "わたし", 100)])],
            0,
        );
        let mut list = CandidateList::new();
        list.push(cand("にほん固有", "にほん", 100));
        state.candidates = Some(list);

        let d = decide_commit(&state);
        assert_eq!(
            d.commit_text, "私",
            "★ segmented が優先 (= candidates ではなく)"
        );
    }

    #[test]
    fn candidates_take_priority_over_composition() {
        let state = flat_state(vec![cand("日本", "にほん", 100)]);
        // composition = "にほん" だが候補があるので候補優先
        let d = decide_commit(&state);
        assert_eq!(d.commit_text, "日本");
    }

    // -- context 不変条件 (= 副作用なし) --

    #[test]
    fn decide_commit_does_not_mutate_state() {
        let original = segmented_state(
            vec![
                Segment::new("わたし", vec![cand("私", "わたし", 100)]),
                Segment::new("の", vec![cand("の", "の", 100)]),
            ],
            0,
        );
        let original_focused = original.segmented.as_ref().unwrap().focused;

        // 関数呼び出し前後で state は変わらない
        let _ = decide_commit(&original);
        assert_eq!(
            original.segmented.as_ref().unwrap().focused,
            original_focused
        );
        assert_eq!(original.composition, "わたしのなまえ");
    }

    /// `ConversionContext` は decide_commit には不要 (= 学習側で使う)
    /// のサニティチェック
    #[test]
    fn context_field_is_not_referenced() {
        let mut state = composing_state("にほん");
        state.context = ConversionContext::default();
        let _ = decide_commit(&state); // panic しないこと
    }
}
