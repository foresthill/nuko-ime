//! Mozc 由来のローマ字 → ひらがな変換テーブル
//!
//! Google Mozc プロジェクト (BSD-3) の `src/data/preedit/romanji-hiragana.tsv` を
//! コンパイル時に埋め込み、起動時に1度だけパースして HashMap として提供する。
//!
//! テーブル形式:
//! - 2列: `romaji<TAB>hiragana`
//! - 3列 (促音用): `romaji<TAB>hiragana<TAB>next_state`
//!   `next_state` は変換後にバッファに残す文字列 (例: "kk" → "っ" + 残す "k")

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// ローマ字テーブルのエントリ
#[derive(Debug, Clone, Copy)]
pub struct RomajiEntry {
    /// 出力するかな文字列
    pub kana: &'static str,
    /// 変換後にバッファに残す文字列 (None なら空にする)
    ///
    /// 現状はカスタム促音処理が同等の動作をするため未使用。
    /// 将来カスタムロジックを廃して Mozc のテーブル駆動に統一する際に活用する。
    #[allow(dead_code)]
    pub next_state: Option<&'static str>,
}

/// Mozc TSV を埋め込み (取得 SHA: 60af02ff797275f2ba1b7fddccdec916798d112e)
const MOZC_TSV: &str = include_str!("../../data/vendor/mozc/romanji-hiragana.tsv");

/// パース済みのローマ字 → エントリ マップ
pub static MOZC_TABLE: Lazy<HashMap<&'static str, RomajiEntry>> = Lazy::new(|| {
    let mut table = HashMap::with_capacity(MOZC_TSV.lines().count());
    for line in MOZC_TSV.lines() {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.len() {
            2 => {
                table.insert(
                    cols[0],
                    RomajiEntry {
                        kana: cols[1],
                        next_state: None,
                    },
                );
            }
            3 => {
                table.insert(
                    cols[0],
                    RomajiEntry {
                        kana: cols[1],
                        next_state: Some(cols[2]),
                    },
                );
            }
            _ => {
                // 想定外のフォーマット行は無視 (空行・コメント等)
            }
        }
    }
    table
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_loads() {
        // 主要エントリが存在することを確認
        assert!(MOZC_TABLE.contains_key("a"));
        assert!(MOZC_TABLE.contains_key("ka"));
        assert!(MOZC_TABLE.contains_key("nn"));
    }

    #[test]
    fn table_size_reasonable() {
        // Mozc TSV は 323 行、エントリ数は 300+ 程度を期待
        let len = MOZC_TABLE.len();
        assert!(
            len > 280 && len < 400,
            "Mozc テーブルのエントリ数が想定範囲外: {len}"
        );
    }

    #[test]
    fn basic_vowels() {
        assert_eq!(MOZC_TABLE.get("a").unwrap().kana, "あ");
        assert_eq!(MOZC_TABLE.get("i").unwrap().kana, "い");
        assert_eq!(MOZC_TABLE.get("u").unwrap().kana, "う");
        assert_eq!(MOZC_TABLE.get("e").unwrap().kana, "え");
        assert_eq!(MOZC_TABLE.get("o").unwrap().kana, "お");
    }

    #[test]
    fn sokuon_has_next_state() {
        // 促音エントリは next_state を持つ
        let kk = MOZC_TABLE.get("kk").expect("kk エントリ");
        assert_eq!(kk.kana, "っ");
        assert_eq!(kk.next_state, Some("k"));

        let tt = MOZC_TABLE.get("tt").expect("tt エントリ");
        assert_eq!(tt.kana, "っ");
        assert_eq!(tt.next_state, Some("t"));
    }

    #[test]
    fn n_variants_present() {
        assert_eq!(MOZC_TABLE.get("n").unwrap().kana, "ん");
        assert_eq!(MOZC_TABLE.get("nn").unwrap().kana, "ん");
        assert_eq!(MOZC_TABLE.get("n'").unwrap().kana, "ん");
    }

    #[test]
    fn small_kana_present() {
        // ぁぃぅぇぉ等の小書きかな (xa la 等)
        assert!(MOZC_TABLE.contains_key("xa") || MOZC_TABLE.contains_key("la"));
    }

    #[test]
    fn v_row_present() {
        // ゔ行 (外来音)
        assert!(MOZC_TABLE.contains_key("va") || MOZC_TABLE.contains_key("vu"));
    }
}
