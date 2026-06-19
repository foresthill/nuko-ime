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

/// 数字 1-9 キーが押された時の commit 決定 (テスト基盤 #2)。
///
/// `digit` が `'1'..='9'` で、かつ `state.candidates` の `line_idx (= digit - '1')`
/// 番目の候補が存在するときのみ `Some(CommitDecision)` を返す。
///
/// segmented モード時は **focused 文節に line_idx を反映してから全文連結**
/// (= 既存の数字ハンドラのデータ消失バグを防ぐ。PR #51 で fix した挙動を
/// 純粋関数として固定化)。
///
/// 純粋関数: `state` は immut borrow、segmented mode 時は内部で clone して
/// mutate するため呼び出し側の状態は変わらない。
///
/// # 戻り値
///
/// - `Some(decision)` — 数字選択可能、commit してよい
/// - `None` — 数字でない / 範囲外 / 候補無し
///   呼び出し側は別経路 (= 「他文字打鍵で auto-commit」) にフォールバックする
#[must_use]
pub fn decide_digit_select_and_commit(state: &InputState, digit: char) -> Option<CommitDecision> {
    if !('1'..='9').contains(&digit) {
        return None;
    }
    let line_idx = (digit as usize) - ('1' as usize);

    let candidates = state.candidates.as_ref()?;
    if line_idx >= candidates.iter().count() {
        return None;
    }

    // segmented モード: focused 文節に line_idx を反映 → 全文連結
    if let Some(segmented) = state.segmented.as_ref() {
        let mut new_segmented = segmented.clone();
        if let Some(seg) = new_segmented.focused_segment_mut() {
            seg.select(line_idx);
        }
        return Some(CommitDecision {
            commit_text: new_segmented.current_surface(),
            learn_targets: new_segmented
                .segments
                .iter()
                .filter_map(|s| s.current().cloned())
                .collect(),
        });
    }

    // flat モード: line_idx の候補だけ
    let picked = candidates.iter().nth(line_idx)?.clone();
    Some(CommitDecision {
        commit_text: picked.surface.clone(),
        learn_targets: vec![picked],
    })
}

/// `didCommandBySelector:` のセレクタ分岐を表す決定 (テスト基盤 #4)。
///
/// controller の `_did_command_impl` は AppKit のセレクタ名 (`insertNewline:` 等)
/// を見て「確定 / 取消 / 削除 / 文節移動 / 候補移動 / パススルー」を分岐していた。
/// この **「セレクタ名 + composing 状態 → アクション」** のマッピングだけを
/// 純粋関数として切り出し、ObjC ランタイム無しで検証可能にする。
///
/// 副作用 (do_commit / do_cancel / 候補移動 / marked text 更新) は引き続き
/// 呼び出し側の責務。本 enum は「何をすべきか」だけを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    /// 未変換 (= 非 composing) → IME は処理しない (passthrough, `Bool::NO`)
    PassThrough,
    /// Enter (`insertNewline:`) → 確定
    Commit,
    /// Escape (`cancelOperation:`) → 取消
    Cancel,
    /// Backspace (`deleteBackward:`) → 1 文字削除
    Backspace,
    /// Left (`moveLeft:`) → 文節フォーカスを前へ (segmented モードのみ実効)
    FocusShiftLeft,
    /// Right (`moveRight:`) → 文節フォーカスを後ろへ (segmented モードのみ実効)
    FocusShiftRight,
    /// Down (`moveDown:`) → 次候補
    SelectNext,
    /// Up (`moveUp:`) → 前候補
    SelectPrev,
    /// 未知のセレクタ → 確定してからパススルー (`Bool::NO`)
    CommitAndPassThrough,
}

/// セレクタ名 (`&CStr`) と composing 状態から実行すべきアクションを返す。
///
/// ## 守りたい不変条件
///
/// 1. **非 composing 時はセレクタに関わらず必ず `PassThrough`** — 未変換状態で
///    IME が Enter/矢印を横取りするとアプリの挙動を壊す (= 過去のバグ源)。
/// 2. **既知セレクタは composing 時のみ専用アクションになる**。
/// 3. **未知セレクタは `CommitAndPassThrough`** — 未確定テキストを取りこぼさず
///    確定してから OS に処理を返す。
///
/// 純粋関数: 入力は `&CStr` と `bool` のみ。ObjC ランタイム不要。
#[must_use]
pub fn decide_command(selector_name: &std::ffi::CStr, is_composing: bool) -> CommandAction {
    // 不変条件 1: 未変換状態は常にパススルー
    if !is_composing {
        return CommandAction::PassThrough;
    }

    match selector_name.to_bytes() {
        b"insertNewline:" => CommandAction::Commit,
        b"cancelOperation:" => CommandAction::Cancel,
        b"deleteBackward:" => CommandAction::Backspace,
        b"moveLeft:" => CommandAction::FocusShiftLeft,
        b"moveRight:" => CommandAction::FocusShiftRight,
        b"moveDown:" => CommandAction::SelectNext,
        b"moveUp:" => CommandAction::SelectPrev,
        _ => CommandAction::CommitAndPassThrough,
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

    // -- decide_digit_select_and_commit のテスト群 ------------------------
    //
    // 守りたい不変条件:
    //
    // a. 数字でないキー入力では `None` を返す
    // b. 範囲外 line (= 候補数 < line_idx) では `None`
    // c. candidates 無しでは `None`
    // d. **segmented モード時は focused 文節に line_idx を反映 → 全文連結**
    //    (= データ消失防止の本丸、PR #51 で fix した挙動)
    // e. **flat モード時は line_idx の候補だけ commit**
    // f. **副作用なし** — 元の state は変わらない

    #[test]
    fn digit_returns_none_for_non_digit() {
        let state = flat_state(vec![cand("日本", "にほん", 100)]);
        assert!(decide_digit_select_and_commit(&state, 'a').is_none());
        assert!(decide_digit_select_and_commit(&state, '0').is_none()); // 0 は対象外
        assert!(decide_digit_select_and_commit(&state, ' ').is_none());
    }

    #[test]
    fn digit_returns_none_when_no_candidates() {
        let state = composing_state("にほん");
        assert!(decide_digit_select_and_commit(&state, '1').is_none());
    }

    #[test]
    fn digit_returns_none_when_out_of_range() {
        // 候補 2 件しか無いのに '3' を押すと None
        let state = flat_state(vec![
            cand("日本", "にほん", 100),
            cand("二本", "にほん", 80),
        ]);
        assert!(decide_digit_select_and_commit(&state, '3').is_none());
    }

    #[test]
    fn digit_flat_mode_selects_line() {
        let state = flat_state(vec![
            cand("日本", "にほん", 100),
            cand("二本", "にほん", 80),
            cand("にほん", "にほん", -100),
        ]);

        let d = decide_digit_select_and_commit(&state, '2').unwrap();
        assert_eq!(d.commit_text, "二本", "★ '2' で line 1 (= 二本) を確定");
        assert_eq!(d.learn_targets.len(), 1);
    }

    #[test]
    fn digit_segmented_mode_full_sentence_with_chosen_line() {
        // 「わたしのなまえ」: 文節 0 (= わたし) で焦点
        // candidates は focused 文節の候補リスト
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
        // controller は focused 文節の候補リストを state.candidates に load する
        let mut list = CandidateList::new();
        for c in &state.segmented.as_ref().unwrap().segments[0].candidates {
            list.push(c.clone());
        }
        state.candidates = Some(list);

        // '2' で line 1 (= 渡し) を選ぶ → 全文「渡しの名前」になることを保証
        let d = decide_digit_select_and_commit(&state, '2').unwrap();
        assert_eq!(
            d.commit_text, "渡しの名前",
            "★ segmented モード: focused 文節 0 の line 1 (= 渡し) + 他文節 (の / 名前) を連結"
        );
        assert_eq!(d.learn_targets.len(), 3, "★ 各文節の選択を個別に学習");
    }

    #[test]
    fn digit_segmented_mode_no_data_loss_pattern() {
        // 「こうそくでうってると」 PR #49/51 のデータ消失バグパターン
        let mut state = segmented_state(
            vec![
                Segment::new(
                    "こうそく",
                    vec![cand("高速", "こうそく", 100), cand("拘束", "こうそく", 80)],
                ),
                Segment::new("で", vec![cand("で", "で", 100)]),
                Segment::new("うって", vec![cand("打って", "うって", 100)]),
                Segment::new("る", vec![cand("る", "る", 100)]),
                Segment::new("と", vec![cand("と", "と", 100)]),
            ],
            0,
        );
        let mut list = CandidateList::new();
        for c in &state.segmented.as_ref().unwrap().segments[0].candidates {
            list.push(c.clone());
        }
        state.candidates = Some(list);

        // '1' で line 0 (= 高速) を確定 → 全文「高速で打ってると」
        let d = decide_digit_select_and_commit(&state, '1').unwrap();
        assert_eq!(
            d.commit_text, "高速で打ってると",
            "★ 焦点 0 で '1' を押しても残り文節 (で/打って/る/と) は消失しない"
        );
    }

    #[test]
    fn digit_does_not_mutate_state() {
        let state = flat_state(vec![
            cand("日本", "にほん", 100),
            cand("二本", "にほん", 80),
        ]);
        let original_selected = state.candidates.as_ref().unwrap().selected_index();

        let _ = decide_digit_select_and_commit(&state, '2');
        assert_eq!(
            state.candidates.as_ref().unwrap().selected_index(),
            original_selected,
            "★ 純粋関数: state は変化しない"
        );
    }

    #[test]
    fn digit_segmented_does_not_mutate_state() {
        let mut state = segmented_state(
            vec![Segment::new(
                "わたし",
                vec![cand("私", "わたし", 100), cand("渡し", "わたし", 80)],
            )],
            0,
        );
        let mut list = CandidateList::new();
        for c in &state.segmented.as_ref().unwrap().segments[0].candidates {
            list.push(c.clone());
        }
        state.candidates = Some(list);

        let original_focused = state.segmented.as_ref().unwrap().focused;
        let original_seg0_sel = state.segmented.as_ref().unwrap().segments[0].selected;

        let _ = decide_digit_select_and_commit(&state, '2');
        assert_eq!(state.segmented.as_ref().unwrap().focused, original_focused);
        assert_eq!(
            state.segmented.as_ref().unwrap().segments[0].selected,
            original_seg0_sel,
            "★ segmented モードでも state は変化しない (内部で clone)"
        );
    }

    // -- auto-commit (他文字打鍵時) のテスト群 ---------------------------
    //
    // 「候補表示中に新しい文字を打ったら現在の候補を確定して新しい入力を始める」パス。
    // 決定ロジックは `decide_commit` を使い回せるので、ここでは
    // **auto-commit 経路特有の不変条件** を追加検証する:
    //
    // - segmented モード時、auto-commit でも **全 segment が学習対象** になる
    //   (旧コード PR #51 は commit_text だけ全文で、学習は focused 文節のみ
    //   = 潜在バグがあった)
    // - flat モード時は 1 件の selected を学習

    #[test]
    fn auto_commit_segmented_learns_all_segments_not_just_focused() {
        // 「よろしくお願いいたします」で focused=2 (= いたします) で auto-commit
        // 学習対象は **全 3 segment** であるべき (= 文節 0/1 も学習されてしかるべき)
        let state = segmented_state(
            vec![
                Segment::new("よろしく", vec![cand("宜しく", "よろしく", 100)]),
                Segment::new("おねがい", vec![cand("お願い", "おねがい", 100)]),
                Segment::new("いたします", vec![cand("致します", "いたします", 100)]),
            ],
            2, // focus = 最後の segment
        );

        let d = decide_commit(&state);
        assert_eq!(d.commit_text, "宜しくお願い致します");
        assert_eq!(
            d.learn_targets.len(),
            3,
            "★ auto-commit でも全 segment が learn_targets に入る (旧コードは 1 件しか記録してなかった)"
        );
        // 順序保証
        assert_eq!(d.learn_targets[0].surface, "宜しく");
        assert_eq!(d.learn_targets[1].surface, "お願い");
        assert_eq!(d.learn_targets[2].surface, "致します");
    }

    #[test]
    fn auto_commit_flat_mode_learns_only_selected() {
        // flat モードでは learn_targets = 1 件 (selected のみ)
        let state = flat_state(vec![
            cand("日本", "にほん", 100),
            cand("二本", "にほん", 80),
        ]);
        let d = decide_commit(&state);
        assert_eq!(d.learn_targets.len(), 1);
        assert_eq!(d.learn_targets[0].surface, "日本");
    }

    #[test]
    fn auto_commit_after_focus_shift_still_commits_full_sentence() {
        // ←→ で焦点を動かした後でも auto-commit で全文が commit される
        // = 焦点を変えただけで前半 segment が消えるバグの防止
        let mut state = segmented_state(
            vec![
                Segment::new("わたし", vec![cand("私", "わたし", 100)]),
                Segment::new("の", vec![cand("の", "の", 100)]),
                Segment::new("なまえ", vec![cand("名前", "なまえ", 100)]),
            ],
            0,
        );
        // 焦点を 2 に動かす
        state.segmented.as_mut().unwrap().focus(2);

        let d = decide_commit(&state);
        assert_eq!(
            d.commit_text, "私の名前",
            "★ 焦点が末尾でも全文連結 (= 焦点位置に依存しない)"
        );
    }

    // -- テスト基盤 #4: decide_command の不変条件テスト群 --
    //
    // 守りたい不変条件:
    //   1. 非 composing 時はどんなセレクタでも必ず PassThrough
    //   2. 既知セレクタ (Enter/Esc/BS/矢印) は composing 時に専用アクション
    //   3. 未知セレクタは CommitAndPassThrough (= 未確定を取りこぼさない)

    /// 不変条件 1: 非 composing なら全セレクタが PassThrough
    #[test]
    fn non_composing_always_passes_through() {
        let selectors = [
            c"insertNewline:",
            c"cancelOperation:",
            c"deleteBackward:",
            c"moveLeft:",
            c"moveRight:",
            c"moveDown:",
            c"moveUp:",
            c"someUnknownSelector:",
        ];
        for sel in selectors {
            assert_eq!(
                decide_command(sel, /*is_composing=*/ false),
                CommandAction::PassThrough,
                "★ 非 composing 時は {sel:?} でも PassThrough でなければならない"
            );
        }
    }

    /// 不変条件 2: composing 時の既知セレクタが正しいアクションに対応
    #[test]
    fn composing_known_selectors_map_to_actions() {
        let cases = [
            (c"insertNewline:", CommandAction::Commit),
            (c"cancelOperation:", CommandAction::Cancel),
            (c"deleteBackward:", CommandAction::Backspace),
            (c"moveLeft:", CommandAction::FocusShiftLeft),
            (c"moveRight:", CommandAction::FocusShiftRight),
            (c"moveDown:", CommandAction::SelectNext),
            (c"moveUp:", CommandAction::SelectPrev),
        ];
        for (sel, expected) in cases {
            assert_eq!(
                decide_command(sel, /*is_composing=*/ true),
                expected,
                "★ composing 時 {sel:?} は {expected:?} に対応すべき"
            );
        }
    }

    /// 不変条件 3: composing 時の未知セレクタは CommitAndPassThrough
    #[test]
    fn composing_unknown_selector_commits_and_passes_through() {
        assert_eq!(
            decide_command(c"someRandomSelector:", /*is_composing=*/ true),
            CommandAction::CommitAndPassThrough,
            "★ 未知セレクタは確定してから OS にパススルー (未確定を取りこぼさない)"
        );
        // 空セレクタや無関係なものも同様
        assert_eq!(
            decide_command(c"", /*is_composing=*/ true),
            CommandAction::CommitAndPassThrough,
        );
        assert_eq!(
            decide_command(c"noop:", /*is_composing=*/ true),
            CommandAction::CommitAndPassThrough,
        );
    }

    /// 回帰: Enter は composing 時のみ Commit、非 composing では PassThrough。
    /// (= 未変換状態で IME が Enter を横取りして改行を潰すバグの防止)
    #[test]
    fn enter_is_committed_only_while_composing() {
        assert_eq!(
            decide_command(c"insertNewline:", true),
            CommandAction::Commit
        );
        assert_eq!(
            decide_command(c"insertNewline:", false),
            CommandAction::PassThrough,
            "★ 未変換状態の Enter は IME が触らずアプリに渡す"
        );
    }
}
