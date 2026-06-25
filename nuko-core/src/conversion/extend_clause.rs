//! 文節境界の伸縮 — `force_ranges` (文節境界の強制指定) を計算する純粋関数。
//!
//! Shift+→ / Shift+← で文節の区切りを手動調整するための上流ロジック。
//! libakaza の `convert(yomi, Some(force_ranges))` に渡す **バイトオフセットの
//! 範囲列** を、各文節の読みとフォーカス位置だけから計算する。
//!
//! ## 操作の方向 (重要な仕様)
//!
//! **`extend_left` / `extend_right` はどちらも「focused 文節の右端だけ」を動かす。
//! 左端は決して動かさない。**
//!
//! - **Shift+→ (`extend_right`)**: focused の右端を 1 文字 **右** へ = focused を伸ばす
//!   (右隣から 1 文字もらう)。
//! - **Shift+← (`extend_left`)**: focused の右端を 1 文字 **左** へ = focused を縮める
//!   (末尾 1 文字を右隣に渡す。末尾文節なら新文節を作る)。
//!
//! これは Google 日本語入力等の標準で、「いま自分がどこの区切りを調整しているか」が
//! 直感的になる。**左端を動かすとユーザーが混乱する**ため、この方針に統一している
//! (ユーザー要望 2026-06-25。CLAUDE.md「文節伸縮の仕様」にも明記)。
//!
//! ## 出典
//!
//! `extend_right` のアルゴリズムは akaza (MIT, Copyright (c) 2023 Tokuhiro Matsuno) の
//! `libakaza/src/extend_clause.rs` を、ぬこIME の文節読み (`&[&str]`) に合わせて移植した
//! もの (akaza は本体候補 `clause[0].yomi` のみ参照するので読み配列で等価)。
//! `extend_left` は akaza が focused>0 で **左端** を動かす実装だったため、「右端のみ
//! 動かす」方針に作り替えている。本プロジェクトのライセンス (Apache-2.0 OR MIT) と
//! MIT は互換。
//!
//! ## 不変条件
//!
//! - 返る `force_ranges` は元の全読みのバイト列を過不足なく覆う (合計長保存)。
//! - これ以上伸縮できない場合 (右端で右伸長 / 1 文字で縮小 等) は
//!   `keep_current` = 現状維持の範囲列を返す。

use std::ops::Range;

/// 現状の文節構成をそのまま `force_ranges` にして返す (= 伸縮しない)。
fn keep_current(readings: &[&str]) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(readings.len());
    let mut offset = 0;
    for yomi in readings {
        ranges.push(offset..offset + yomi.len());
        offset += yomi.len();
    }
    ranges
}

/// focused 文節を **右** に 1 文字伸ばす (= 右隣から 1 文字もらう)。
///
/// `readings` は各文節の読み、`focused` は左から 0 起点のフォーカス位置。
/// 右端の文節がフォーカスされている場合は現状維持。
#[must_use]
pub fn extend_right(readings: &[&str], focused: usize) -> Vec<Range<usize>> {
    if readings.is_empty() {
        return Vec::new();
    }
    // 一番右の文節は右に伸ばせない。
    if focused == readings.len() - 1 {
        return keep_current(readings);
    }

    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut offset = 0;
    for (i, &yomi) in readings.iter().enumerate() {
        if i == focused {
            // フォーカス文節は右隣の先頭 1 文字ぶん伸びる。
            let next = readings[i + 1];
            let Some(next_first) = next.chars().next() else {
                return keep_current(readings);
            };
            ranges.push(offset..offset + yomi.len() + next_first.len_utf8());
        } else if i == focused + 1 {
            // 右隣の文節は先頭 1 文字を奪われる。
            let Some(first) = yomi.chars().next() else {
                return keep_current(readings);
            };
            let first_len = first.len_utf8();
            let start = offset + first_len;
            let end = offset + yomi.len();
            // 1 文字しかなければ消失する (range を積まない)。
            if start < end {
                ranges.push(start..end);
            }
        } else {
            ranges.push(offset..offset + yomi.len());
        }
        offset += yomi.len();
    }
    ranges
}

/// focused 文節の **右端を 1 文字左** へ動かす (= focused を 1 文字縮める)。
///
/// 縮めた末尾 1 文字は **右隣の文節の先頭** に移る。focused が末尾文節のときは、
/// その 1 文字で **新しい文節を右に作る**。focused が 1 文字しかないときは縮められ
/// ないので現状維持。
///
/// **設計方針 (重要): `extend_left` / `extend_right` はどちらも「focused 文節の
/// 右端だけ」を動かす。左端は決して動かさない。** これは Google 日本語入力等の
/// 標準挙動で、「いま自分がどこの区切りを調整しているか」が直感的になる。
/// (akaza 由来の旧実装は focused>0 で左端を動かしていたが、ユーザーから「左が
/// 増えると混乱する」と指摘され 2026-06-25 にこの方針へ統一した。)
#[must_use]
pub fn extend_left(readings: &[&str], focused: usize) -> Vec<Range<usize>> {
    if readings.is_empty() {
        return Vec::new();
    }
    let f = focused.min(readings.len() - 1);

    // focused の末尾 1 文字。1 文字以下なら右端を左に動かせない (空になる) → 現状維持。
    let Some(last) = readings[f].chars().last() else {
        return keep_current(readings);
    };
    if readings[f].chars().count() <= 1 {
        return keep_current(readings);
    }
    let last_len = last.len_utf8();

    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut offset = 0;
    for (i, &yomi) in readings.iter().enumerate() {
        if i == f {
            // focused: 末尾 1 文字を削る
            ranges.push(offset..offset + yomi.len() - last_len);
            if f + 1 == readings.len() {
                // focused が末尾文節: 削った 1 文字で新しい文節を作る
                let start = offset + yomi.len() - last_len;
                ranges.push(start..offset + yomi.len());
            }
        } else if i == f + 1 {
            // 右隣: focused から移ってきた 1 文字を先頭に足す
            let start = offset - last_len;
            ranges.push(start..offset + yomi.len());
        } else {
            ranges.push(offset..offset + yomi.len());
        }
        offset += yomi.len();
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `readings` と range 列から、各 range が指す部分文字列を取り出す。
    fn sliced(readings: &[&str], ranges: &[Range<usize>]) -> Vec<String> {
        let yomi = readings.concat();
        ranges.iter().map(|r| yomi[r.clone()].to_string()).collect()
    }

    // -- extend_right --

    #[test]
    fn right_single_char_keeps() {
        assert_eq!(sliced(&["わ"], &extend_right(&["わ"], 0)), vec!["わ"]);
    }

    #[test]
    fn right_grows_focused_takes_from_next() {
        // [わた][しの] focus=0 → [わたし][の]
        let r = extend_right(&["わた", "しの"], 0);
        assert_eq!(sliced(&["わた", "しの"], &r), vec!["わたし", "の"]);
    }

    #[test]
    fn right_absorbs_single_char_neighbor() {
        // [わた][し] focus=0 → [わたし] (右隣が消失)
        let r = extend_right(&["わた", "し"], 0);
        assert_eq!(sliced(&["わた", "し"], &r), vec!["わたし"]);
    }

    #[test]
    fn right_on_last_clause_keeps() {
        let r = extend_right(&["わた", "しの"], 1);
        assert_eq!(sliced(&["わた", "しの"], &r), vec!["わた", "しの"]);
    }

    #[test]
    fn right_preserves_total_length() {
        let readings = ["わたし", "の", "なまえ"];
        let r = extend_right(&readings, 0);
        let total: usize = r.iter().map(|x| x.end - x.start).sum();
        assert_eq!(total, readings.concat().len(), "合計バイト長は保存される");
    }

    // -- extend_left --

    #[test]
    fn left_single_clause_splits_last_char() {
        // [わたし] → [わた][し]
        let r = extend_left(&["わたし"], 0);
        assert_eq!(sliced(&["わたし"], &r), vec!["わた", "し"]);
    }

    #[test]
    fn left_single_char_keeps() {
        assert_eq!(sliced(&["わ"], &extend_left(&["わ"], 0)), vec!["わ"]);
    }

    #[test]
    fn left_focus0_shrinks_left_clause() {
        // [わた][しの] focus=0 → [わ][たしの]
        let r = extend_left(&["わた", "しの"], 0);
        assert_eq!(sliced(&["わた", "しの"], &r), vec!["わ", "たしの"]);
    }

    #[test]
    fn left_focus_shrinks_right_edge_moves_char_to_next() {
        // 仕様: focused の右端を 1 文字左へ。末尾文字は右隣の先頭に移る。
        // [わたし][なまえ] focus=0 → [わた][しなまえ]
        let r = extend_left(&["わたし", "なまえ"], 0);
        assert_eq!(sliced(&["わたし", "なまえ"], &r), vec!["わた", "しなまえ"]);
    }

    #[test]
    fn left_focus1_shrinks_focused_not_left_neighbor() {
        // ★ 重要: focused>0 でも「左隣」ではなく focused 自身の右端を縮める。
        // [わたし][なまえ][です] focus=1 → [わたし][なま][えです]
        // (旧 akaza 実装は [わた][しなまえ][です] のように左端を動かしていた)
        let r = extend_left(&["わたし", "なまえ", "です"], 1);
        assert_eq!(
            sliced(&["わたし", "なまえ", "です"], &r),
            vec!["わたし", "なま", "えです"],
        );
    }

    #[test]
    fn left_focus_on_last_segment_creates_new_segment() {
        // 末尾文節を縮めると、削った 1 文字で新しい文節ができる。
        // [わたし][なまえ] focus=1 → [わたし][なま][え]
        let r = extend_left(&["わたし", "なまえ"], 1);
        assert_eq!(
            sliced(&["わたし", "なまえ"], &r),
            vec!["わたし", "なま", "え"]
        );
    }

    #[test]
    fn left_single_char_focused_keeps() {
        // focused が 1 文字なら縮められない (空になるため現状維持)。
        // [わたし][の][なまえ] focus=1 (=「の」) → 変化なし
        let r = extend_left(&["わたし", "の", "なまえ"], 1);
        assert_eq!(
            sliced(&["わたし", "の", "なまえ"], &r),
            vec!["わたし", "の", "なまえ"],
        );
    }

    #[test]
    fn left_then_right_roundtrips() {
        // 右端を 1 縮めて 1 伸ばせば元に戻る (focused 維持)。
        let base = ["わたし", "なまえ"];
        let shrunk = extend_left(&base, 0); // [わた][しなまえ]
        assert_eq!(sliced(&base, &shrunk), vec!["わた", "しなまえ"]);
        // shrunk の読みで focus=0 を右伸長 → 元の [わたし][なまえ]
        let readings: Vec<String> = sliced(&base, &shrunk);
        let refs: Vec<&str> = readings.iter().map(String::as_str).collect();
        let grown = extend_right(&refs, 0);
        assert_eq!(sliced(&refs, &grown), vec!["わたし", "なまえ"]);
    }

    #[test]
    fn left_preserves_total_length() {
        let readings = ["わたし", "の", "なまえ"];
        let r = extend_left(&readings, 2);
        let total: usize = r.iter().map(|x| x.end - x.start).sum();
        assert_eq!(total, readings.concat().len());
    }

    #[test]
    fn left_focus0_single_char_left_clause_keeps() {
        // [わ][たし] focus=0 → 左が 1 文字なので現状維持
        let r = extend_left(&["わ", "たし"], 0);
        assert_eq!(sliced(&["わ", "たし"], &r), vec!["わ", "たし"]);
    }
}
