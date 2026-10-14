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
    m.insert('ア', "ｱ");
    m.insert('イ', "ｲ");
    m.insert('ウ', "ｳ");
    m.insert('エ', "ｴ");
    m.insert('オ', "ｵ");
    m.insert('カ', "ｶ");
    m.insert('キ', "ｷ");
    m.insert('ク', "ｸ");
    m.insert('ケ', "ｹ");
    m.insert('コ', "ｺ");
    m.insert('サ', "ｻ");
    m.insert('シ', "ｼ");
    m.insert('ス', "ｽ");
    m.insert('セ', "ｾ");
    m.insert('ソ', "ｿ");
    m.insert('タ', "ﾀ");
    m.insert('チ', "ﾁ");
    m.insert('ツ', "ﾂ");
    m.insert('テ', "ﾃ");
    m.insert('ト', "ﾄ");
    m.insert('ナ', "ﾅ");
    m.insert('ニ', "ﾆ");
    m.insert('ヌ', "ﾇ");
    m.insert('ネ', "ﾈ");
    m.insert('ノ', "ﾉ");
    m.insert('ハ', "ﾊ");
    m.insert('ヒ', "ﾋ");
    m.insert('フ', "ﾌ");
    m.insert('ヘ', "ﾍ");
    m.insert('ホ', "ﾎ");
    m.insert('マ', "ﾏ");
    m.insert('ミ', "ﾐ");
    m.insert('ム', "ﾑ");
    m.insert('メ', "ﾒ");
    m.insert('モ', "ﾓ");
    m.insert('ヤ', "ﾔ");
    m.insert('ユ', "ﾕ");
    m.insert('ヨ', "ﾖ");
    m.insert('ラ', "ﾗ");
    m.insert('リ', "ﾘ");
    m.insert('ル', "ﾙ");
    m.insert('レ', "ﾚ");
    m.insert('ロ', "ﾛ");
    m.insert('ワ', "ﾜ");
    m.insert('ヲ', "ｦ");
    m.insert('ン', "ﾝ");
    m.insert('ガ', "ｶﾞ");
    m.insert('ギ', "ｷﾞ");
    m.insert('グ', "ｸﾞ");
    m.insert('ゲ', "ｹﾞ");
    m.insert('ゴ', "ｺﾞ");
    m.insert('ザ', "ｻﾞ");
    m.insert('ジ', "ｼﾞ");
    m.insert('ズ', "ｽﾞ");
    m.insert('ゼ', "ｾﾞ");
    m.insert('ゾ', "ｿﾞ");
    m.insert('ダ', "ﾀﾞ");
    m.insert('ヂ', "ﾁﾞ");
    m.insert('ヅ', "ﾂﾞ");
    m.insert('デ', "ﾃﾞ");
    m.insert('ド', "ﾄﾞ");
    m.insert('バ', "ﾊﾞ");
    m.insert('ビ', "ﾋﾞ");
    m.insert('ブ', "ﾌﾞ");
    m.insert('ベ', "ﾍﾞ");
    m.insert('ボ', "ﾎﾞ");
    m.insert('パ', "ﾊﾟ");
    m.insert('ピ', "ﾋﾟ");
    m.insert('プ', "ﾌﾟ");
    m.insert('ペ', "ﾍﾟ");
    m.insert('ポ', "ﾎﾟ");
    m.insert('ヴ', "ｳﾞ");
    m.insert('ァ', "ｧ");
    m.insert('ィ', "ｨ");
    m.insert('ゥ', "ｩ");
    m.insert('ェ', "ｪ");
    m.insert('ォ', "ｫ");
    m.insert('ャ', "ｬ");
    m.insert('ュ', "ｭ");
    m.insert('ョ', "ｮ");
    m.insert('ッ', "ｯ");
    m.insert('ー', "ｰ");
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

/// F9/F10 の英数変換で連打したときの大小文字サイクル段階。
///
/// MS-IME / Google 等の標準: 1 回目=小文字, 2 回目=大文字, 3 回目=先頭大文字。
/// 以降はこの 3 段を循環する。出典: Wikipedia "Language input keys"。
fn apply_alnum_case(raw: &str, stage: u8) -> String {
    match stage % 3 {
        0 => raw.to_lowercase(),
        1 => raw.to_uppercase(),
        // 2: 先頭の英字のみ大文字、残りは小文字 (Howaito)。
        _ => {
            let lower = raw.to_lowercase();
            let mut out = String::with_capacity(lower.len());
            let mut done = false;
            for c in lower.chars() {
                if !done && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    done = true;
                } else {
                    out.push(c);
                }
            }
            out
        }
    }
}

/// F9 (全角英数) / F10 (半角英数): 打った生ローマ字 `raw` を英数に変換する。
///
/// `fullwidth=true` で全角ラテン (F9)、false で半角 (F10)。`stage` は連打段階
/// (0=小文字 / 1=大文字 / 2=先頭大文字、以降 3 で循環)。
///
/// 例: `to_alnum_fkey("howaito", false, 0)` → `"howaito"`,
///     `(.., true, 1)` → `"ＨＯＷＡＩＴＯ"`, `(.., false, 2)` → `"Howaito"`。
#[must_use]
pub fn to_alnum_fkey(raw: &str, fullwidth: bool, stage: u8) -> String {
    let cased = apply_alnum_case(raw, stage);
    if fullwidth {
        to_fullwidth_ascii(&cased)
    } else {
        to_halfwidth_ascii(&cased)
    }
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
        // 半角カタカナ (U+FF61..=FF9F)。濁音は基字＋半角濁点ﾞ(U+FF9E) に分解される。
        assert_eq!(to_halfwidth_katakana("にほんご"), "ﾆﾎﾝｺﾞ");
        assert_eq!(to_halfwidth_katakana("ぬこいめ"), "ﾇｺｲﾒ");
        assert_eq!(to_halfwidth_katakana("ガギグゲゴ"), "ｶﾞｷﾞｸﾞｹﾞｺﾞ");
        assert_eq!(to_halfwidth_katakana("パピプペポ"), "ﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ");
        assert_eq!(to_halfwidth_katakana("ヴァイオリン"), "ｳﾞｧｲｵﾘﾝ");
        assert_eq!(to_halfwidth_katakana("キャット"), "ｷｬｯﾄ");
        // 全ての出力文字が半角カタカナ領域に収まることを保証（全角混入の回帰防止）。
        let out = to_halfwidth_katakana("あいうえおがぱゔぁっー");
        assert!(
            out.chars().all(|c| ('\u{FF61}'..='\u{FF9F}').contains(&c)),
            "全角が混入: {out:?}"
        );
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
    fn test_to_alnum_fkey_cycle() {
        // F10 (半角英数): 小文字 → 大文字 → 先頭大文字 を循環 (出典 Wikipedia/MS-IME)。
        assert_eq!(to_alnum_fkey("howaito", false, 0), "howaito");
        assert_eq!(to_alnum_fkey("howaito", false, 1), "HOWAITO");
        assert_eq!(to_alnum_fkey("howaito", false, 2), "Howaito");
        // stage は 3 で循環 (3→0 相当)。
        assert_eq!(to_alnum_fkey("howaito", false, 3), "howaito");
        // F9 (全角英数): 同じ段階で全角ラテンに。
        assert_eq!(to_alnum_fkey("abc", true, 0), "ａｂｃ");
        assert_eq!(to_alnum_fkey("abc", true, 1), "ＡＢＣ");
        assert_eq!(to_alnum_fkey("abc", true, 2), "Ａｂｃ");
        // 数字混じり: 先頭大文字は「最初の英字」に効く。
        assert_eq!(to_alnum_fkey("123abc", false, 2), "123Abc");
        // 既に大文字で打っても stage の規則が優先 (小文字化してから適用)。
        assert_eq!(to_alnum_fkey("Kawasaki", false, 0), "kawasaki");
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
