//! かな処理ユーティリティ

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// かなの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanaType {
    /// ひらがな
    Hiragana,
    /// カタカナ
    Katakana,
    /// 半角カタカナ
    HalfwidthKatakana,
}

/// ひらがな→カタカナ変換テーブル
static HIRAGANA_TO_KATAKANA: Lazy<HashMap<char, char>> = Lazy::new(|| {
    let hiragana = "ぁあぃいぅうぇえぉおかがきぎくぐけげこごさざしじすずせぜそぞただちぢっつづてでとどなにぬねのはばぱひびぴふぶぷへべぺほぼぽまみむめもゃやゅゆょよらりるれろゎわゐゑをんゔゕゖ";
    let katakana = "ァアィイゥウェエォオカガキギクグケゲコゴサザシジスズセゼソゾタダチヂッツヅテデトドナニヌネノハバパヒビピフブプヘベペホボポマミムメモャヤュユョヨラリルレロヮワヰヱヲンヴヵヶ";

    hiragana.chars().zip(katakana.chars()).collect()
});

/// カタカナ→ひらがな変換テーブル
static KATAKANA_TO_HIRAGANA: Lazy<HashMap<char, char>> =
    Lazy::new(|| HIRAGANA_TO_KATAKANA.iter().map(|(&h, &k)| (k, h)).collect());

/// カタカナ→半角カタカナ変換テーブル
static KATAKANA_TO_HALFWIDTH: Lazy<HashMap<char, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert('ア', "ア");
    m.insert('イ', "イ");
    m.insert('ウ', "ウ");
    m.insert('エ', "エ");
    m.insert('オ', "オ");
    m.insert('カ', "カ");
    m.insert('キ', "キ");
    m.insert('ク', "ク");
    m.insert('ケ', "ケ");
    m.insert('コ', "コ");
    m.insert('サ', "サ");
    m.insert('シ', "シ");
    m.insert('ス', "ス");
    m.insert('セ', "セ");
    m.insert('ソ', "ソ");
    m.insert('タ', "タ");
    m.insert('チ', "チ");
    m.insert('ツ', "ツ");
    m.insert('テ', "テ");
    m.insert('ト', "ト");
    m.insert('ナ', "ナ");
    m.insert('ニ', "ニ");
    m.insert('ヌ', "ヌ");
    m.insert('ネ', "ネ");
    m.insert('ノ', "ノ");
    m.insert('ハ', "ハ");
    m.insert('ヒ', "ヒ");
    m.insert('フ', "フ");
    m.insert('ヘ', "ヘ");
    m.insert('ホ', "ホ");
    m.insert('マ', "マ");
    m.insert('ミ', "ミ");
    m.insert('ム', "ム");
    m.insert('メ', "メ");
    m.insert('モ', "モ");
    m.insert('ヤ', "ヤ");
    m.insert('ユ', "ユ");
    m.insert('ヨ', "ヨ");
    m.insert('ラ', "ラ");
    m.insert('リ', "リ");
    m.insert('ル', "ル");
    m.insert('レ', "レ");
    m.insert('ロ', "ロ");
    m.insert('ワ', "ワ");
    m.insert('ヲ', "ヲ");
    m.insert('ン', "ン");
    m.insert('ガ', "ガ");
    m.insert('ギ', "ギ");
    m.insert('グ', "グ");
    m.insert('ゲ', "ゲ");
    m.insert('ゴ', "ゴ");
    m.insert('ザ', "ザ");
    m.insert('ジ', "ジ");
    m.insert('ズ', "ズ");
    m.insert('ゼ', "ゼ");
    m.insert('ゾ', "ゾ");
    m.insert('ダ', "ダ");
    m.insert('ヂ', "ヂ");
    m.insert('ヅ', "ヅ");
    m.insert('デ', "デ");
    m.insert('ド', "ド");
    m.insert('バ', "バ");
    m.insert('ビ', "ビ");
    m.insert('ブ', "ブ");
    m.insert('ベ', "ベ");
    m.insert('ボ', "ボ");
    m.insert('パ', "パ");
    m.insert('ピ', "ピ");
    m.insert('プ', "プ");
    m.insert('ペ', "ペ");
    m.insert('ポ', "ポ");
    m.insert('ァ', "ァ");
    m.insert('ィ', "ィ");
    m.insert('ゥ', "ゥ");
    m.insert('ェ', "ェ");
    m.insert('ォ', "ォ");
    m.insert('ャ', "ャ");
    m.insert('ュ', "ュ");
    m.insert('ョ', "ョ");
    m.insert('ッ', "ッ");
    m.insert('ー', "ー");
    m
});

/// ひらがなに変換
///
/// # 引数
/// * `input` - 変換する文字列
///
/// # 戻り値
/// ひらがなに変換された文字列
#[must_use]
pub fn to_hiragana(input: &str) -> String {
    input
        .chars()
        .map(|c| *KATAKANA_TO_HIRAGANA.get(&c).unwrap_or(&c))
        .collect()
}

/// カタカナに変換
///
/// # 引数
/// * `input` - 変換する文字列
///
/// # 戻り値
/// カタカナに変換された文字列
#[must_use]
pub fn to_katakana(input: &str) -> String {
    input
        .chars()
        .map(|c| *HIRAGANA_TO_KATAKANA.get(&c).unwrap_or(&c))
        .collect()
}

/// 半角カタカナに変換
///
/// # 引数
/// * `input` - 変換する文字列
///
/// # 戻り値
/// 半角カタカナに変換された文字列
#[must_use]
pub fn to_halfwidth_katakana(input: &str) -> String {
    let katakana = to_katakana(input);
    katakana
        .chars()
        .map(|c| {
            KATAKANA_TO_HALFWIDTH
                .get(&c)
                .map_or_else(|| c.to_string(), |s| (*s).to_string())
        })
        .collect()
}

/// 半角 ASCII 印字可能文字を全角に変換（F9: 全角英数）。
///
/// 入力した**ローマ字（ascii）そのもの**を全角ラテンにする。
/// `0x20`(space)→`U+3000`、`0x21..=0x7E`→`U+FF01..=U+FF5E`。その他の文字はそのまま。
///
/// 例: `kawasaki` → `ｋａｗａｓａｋｉ`
#[must_use]
pub fn to_fullwidth_ascii(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            ' ' => '\u{3000}',
            '\u{21}'..='\u{7E}' => char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// 全角 ASCII を半角に変換（F10: 半角英数）。
///
/// `U+3000`→space、`U+FF01..=U+FF5E`→`0x21..=0x7E`。その他の文字はそのまま。
/// 既に半角の ascii はそのまま返るので、ローマ字 ascii をそのまま確定する用途にも使える。
///
/// 例: `ｋａｗａｓａｋｉ` → `kawasaki`
#[must_use]
pub fn to_halfwidth_ascii(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFF01 + 0x21).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_katakana() {
        assert_eq!(to_katakana("にほんご"), "ニホンゴ");
        assert_eq!(to_katakana("ぬこいめ"), "ヌコイメ");
    }

    #[test]
    fn test_to_hiragana() {
        assert_eq!(to_hiragana("ニホンゴ"), "にほんご");
        assert_eq!(to_hiragana("ヌコイメ"), "ぬこいめ");
    }

    #[test]
    fn test_to_halfwidth() {
        assert_eq!(to_halfwidth_katakana("にほんご"), "ニホンゴ");
    }

    #[test]
    fn test_to_fullwidth_ascii() {
        assert_eq!(to_fullwidth_ascii("kawasaki"), "ｋａｗａｓａｋｉ");
        assert_eq!(to_fullwidth_ascii("abc123"), "ａｂｃ１２３");
        assert_eq!(to_fullwidth_ascii("A!z"), "Ａ！ｚ");
        assert_eq!(
            to_fullwidth_ascii("a b"),
            "ａ　ｂ",
            "半角スペース→全角スペース"
        );
    }

    #[test]
    fn test_to_halfwidth_ascii() {
        assert_eq!(to_halfwidth_ascii("ｋａｗａｓａｋｉ"), "kawasaki");
        assert_eq!(to_halfwidth_ascii("Ａ！ｚ"), "A!z");
        assert_eq!(
            to_halfwidth_ascii("ａ　ｂ"),
            "a b",
            "全角スペース→半角スペース"
        );
        assert_eq!(
            to_halfwidth_ascii("kawasaki"),
            "kawasaki",
            "既に半角はそのまま"
        );
    }

    #[test]
    fn test_ascii_width_roundtrip() {
        let s = "Nuko-IME v0.1.2!";
        assert_eq!(
            to_halfwidth_ascii(&to_fullwidth_ascii(s)),
            s,
            "全角→半角で往復一致"
        );
    }
}
