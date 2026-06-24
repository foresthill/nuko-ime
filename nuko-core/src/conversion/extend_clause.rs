//! 文節境界の伸縮 — `force_ranges` (文節境界の強制指定) を計算する純粋関数。
//!
//! Shift+→ / Shift+← で文節の区切りを手動調整するための上流ロジック。
//! libakaza の `convert(yomi, Some(force_ranges))` に渡す **バイトオフセットの
//! 範囲列** を、各文節の読みとフォーカス位置だけから計算する。
//!
//! ## 出典
//!
//! アルゴリズムは akaza (MIT, Copyright (c) 2023 Tokuhiro Matsuno) の
//! `libakaza/src/extend_clause.rs` を、ぬこIME の文節読み (`&[&str]`) に
//! 合わせて移植したもの。akaza は本体候補 (`clause[0].yomi`) のみを参照する
//! ので、読み文字列の配列だけで等価に計算できる。
//! 本プロジェクトのライセンス (Apache-2.0 OR MIT) と MIT は互換。
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

/// focused 文節の左境界を **左** に動かす (= focused を 1 文字縮める / 左隣を縮める)。
///
/// - `focused == 0`: 一番左の文節の末尾 1 文字を切り出して新しい文節にする。
/// - `focused > 0`: 左隣の末尾 1 文字を focused に移す (focused が左に伸びる)。
///
/// 縮められない (1 文字しかない等) 場合は現状維持。
#[must_use]
pub fn extend_left(readings: &[&str], focused: usize) -> Vec<Range<usize>> {
    if readings.is_empty() {
        return Vec::new();
    }

    // 文節が 1 個だけ: 末尾 1 文字を別文節に切り出す。
    if readings.len() == 1 {
        let yomi = readings[0];
        return match yomi.chars().last() {
            // 2 文字以上 (= 切り出してもどちらも空にならない) のときだけ分割。
            Some(last) if yomi.chars().count() > 1 => {
                let head = yomi.len() - last.len_utf8();
                vec![0..head, head..yomi.len()]
            }
            _ => keep_current(readings),
        };
    }

    if focused == 0 {
        // 一番左がフォーカス: 左文節を 1 文字短くする ([ab][c] → [a][bc])。
        if readings[0].chars().count() == 1 {
            return keep_current(readings);
        }
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut offset = 0;
        for (i, &yomi) in readings.iter().enumerate() {
            if i == focused {
                let Some(last) = yomi.chars().last() else {
                    return keep_current(readings);
                };
                ranges.push(offset..offset + yomi.len() - last.len_utf8());
            } else if i == focused + 1 {
                let Some(prev_last) = readings[i - 1].chars().last() else {
                    return keep_current(readings);
                };
                let prev_last_len = prev_last.len_utf8();
                let start = offset - prev_last_len;
                let end = start + (yomi.len() + prev_last_len);
                if start < end {
                    ranges.push(start..end);
                }
            } else {
                ranges.push(offset..offset + yomi.len());
            }
            offset += yomi.len();
        }
        ranges
    } else {
        // 2 番目以降がフォーカス: 左隣の末尾 1 文字を focused に移す。
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut offset = 0;
        for (i, &yomi) in readings.iter().enumerate() {
            let (start, end) = if i == focused {
                let Some(prev_last) = readings[i - 1].chars().last() else {
                    return keep_current(readings);
                };
                let prev_last_len = prev_last.len_utf8();
                let start = offset - prev_last_len;
                let end = start + yomi.len() + prev_last_len;
                (start, end)
            } else if i == focused - 1 {
                let Some(last) = yomi.chars().last() else {
                    return keep_current(readings);
                };
                let start = offset;
                let end = offset + (yomi.len() - last.len_utf8());
                (start, end)
            } else {
                (offset, offset + yomi.len())
            };
            if start < end {
                ranges.push(start..end);
            }
            offset += yomi.len();
        }
        ranges
    }
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
    fn left_focus1_moves_boundary_left() {
        // [わたし][の] focus=1 → [わた][しの]
        let r = extend_left(&["わたし", "の"], 1);
        assert_eq!(sliced(&["わたし", "の"], &r), vec!["わた", "しの"]);
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
