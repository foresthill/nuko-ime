//! ローマ字→かな変換

use crate::error::Result;
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// ローマ字→かな変換テーブル
static ROMAJI_TABLE: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // 基本母音
    m.insert("a", "あ");
    m.insert("i", "い");
    m.insert("u", "う");
    m.insert("e", "え");
    m.insert("o", "お");

    // か行
    m.insert("ka", "か");
    m.insert("ki", "き");
    m.insert("ku", "く");
    m.insert("ke", "け");
    m.insert("ko", "こ");

    // さ行
    m.insert("sa", "さ");
    m.insert("si", "し");
    m.insert("shi", "し");
    m.insert("su", "す");
    m.insert("se", "せ");
    m.insert("so", "そ");

    // た行
    m.insert("ta", "た");
    m.insert("ti", "ち");
    m.insert("chi", "ち");
    m.insert("tu", "つ");
    m.insert("tsu", "つ");
    m.insert("te", "て");
    m.insert("to", "と");

    // な行
    m.insert("na", "な");
    m.insert("ni", "に");
    m.insert("nu", "ぬ");
    m.insert("ne", "ね");
    m.insert("no", "の");

    // は行
    m.insert("ha", "は");
    m.insert("hi", "ひ");
    m.insert("hu", "ふ");
    m.insert("fu", "ふ");
    m.insert("he", "へ");
    m.insert("ho", "ほ");

    // ま行
    m.insert("ma", "ま");
    m.insert("mi", "み");
    m.insert("mu", "む");
    m.insert("me", "め");
    m.insert("mo", "も");

    // や行
    m.insert("ya", "や");
    m.insert("yi", "い");
    m.insert("yu", "ゆ");
    m.insert("ye", "いぇ");
    m.insert("yo", "よ");

    // ら行
    m.insert("ra", "ら");
    m.insert("ri", "り");
    m.insert("ru", "る");
    m.insert("re", "れ");
    m.insert("ro", "ろ");

    // わ行
    m.insert("wa", "わ");
    m.insert("wi", "うぃ");
    m.insert("we", "うぇ");
    m.insert("wo", "を");

    // ん
    m.insert("n", "ん");
    m.insert("nn", "ん");
    m.insert("n'", "ん");

    // 濁音 - が行
    m.insert("ga", "が");
    m.insert("gi", "ぎ");
    m.insert("gu", "ぐ");
    m.insert("ge", "げ");
    m.insert("go", "ご");

    // 濁音 - ざ行
    m.insert("za", "ざ");
    m.insert("zi", "じ");
    m.insert("ji", "じ");
    m.insert("zu", "ず");
    m.insert("ze", "ぜ");
    m.insert("zo", "ぞ");

    // 濁音 - だ行
    m.insert("da", "だ");
    m.insert("di", "ぢ");
    m.insert("du", "づ");
    m.insert("de", "で");
    m.insert("do", "ど");

    // 濁音 - ば行
    m.insert("ba", "ば");
    m.insert("bi", "び");
    m.insert("bu", "ぶ");
    m.insert("be", "べ");
    m.insert("bo", "ぼ");

    // 半濁音 - ぱ行
    m.insert("pa", "ぱ");
    m.insert("pi", "ぴ");
    m.insert("pu", "ぷ");
    m.insert("pe", "ぺ");
    m.insert("po", "ぽ");

    // 拗音 - きゃ行
    m.insert("kya", "きゃ");
    m.insert("kyi", "きぃ");
    m.insert("kyu", "きゅ");
    m.insert("kye", "きぇ");
    m.insert("kyo", "きょ");

    // 拗音 - しゃ行
    m.insert("sya", "しゃ");
    m.insert("sha", "しゃ");
    m.insert("syi", "しぃ");
    m.insert("syu", "しゅ");
    m.insert("shu", "しゅ");
    m.insert("sye", "しぇ");
    m.insert("she", "しぇ");
    m.insert("syo", "しょ");
    m.insert("sho", "しょ");

    // 拗音 - ちゃ行
    m.insert("tya", "ちゃ");
    m.insert("cha", "ちゃ");
    m.insert("tyi", "ちぃ");
    m.insert("tyu", "ちゅ");
    m.insert("chu", "ちゅ");
    m.insert("tye", "ちぇ");
    m.insert("che", "ちぇ");
    m.insert("tyo", "ちょ");
    m.insert("cho", "ちょ");

    // 拗音 - にゃ行
    m.insert("nya", "にゃ");
    m.insert("nyi", "にぃ");
    m.insert("nyu", "にゅ");
    m.insert("nye", "にぇ");
    m.insert("nyo", "にょ");

    // 拗音 - ひゃ行
    m.insert("hya", "ひゃ");
    m.insert("hyi", "ひぃ");
    m.insert("hyu", "ひゅ");
    m.insert("hye", "ひぇ");
    m.insert("hyo", "ひょ");

    // 拗音 - みゃ行
    m.insert("mya", "みゃ");
    m.insert("myi", "みぃ");
    m.insert("myu", "みゅ");
    m.insert("mye", "みぇ");
    m.insert("myo", "みょ");

    // 拗音 - りゃ行
    m.insert("rya", "りゃ");
    m.insert("ryi", "りぃ");
    m.insert("ryu", "りゅ");
    m.insert("rye", "りぇ");
    m.insert("ryo", "りょ");

    // 拗音 - ぎゃ行
    m.insert("gya", "ぎゃ");
    m.insert("gyi", "ぎぃ");
    m.insert("gyu", "ぎゅ");
    m.insert("gye", "ぎぇ");
    m.insert("gyo", "ぎょ");

    // 拗音 - じゃ行
    m.insert("ja", "じゃ");
    m.insert("jya", "じゃ");
    m.insert("zya", "じゃ");
    m.insert("ji", "じ");
    m.insert("jyi", "じぃ");
    m.insert("ju", "じゅ");
    m.insert("jyu", "じゅ");
    m.insert("zyu", "じゅ");
    m.insert("je", "じぇ");
    m.insert("jye", "じぇ");
    m.insert("jo", "じょ");
    m.insert("jyo", "じょ");
    m.insert("zyo", "じょ");

    // 拗音 - びゃ行
    m.insert("bya", "びゃ");
    m.insert("byi", "びぃ");
    m.insert("byu", "びゅ");
    m.insert("bye", "びぇ");
    m.insert("byo", "びょ");

    // 拗音 - ぴゃ行
    m.insert("pya", "ぴゃ");
    m.insert("pyi", "ぴぃ");
    m.insert("pyu", "ぴゅ");
    m.insert("pye", "ぴぇ");
    m.insert("pyo", "ぴょ");

    // 小文字
    m.insert("xa", "ぁ");
    m.insert("xi", "ぃ");
    m.insert("xu", "ぅ");
    m.insert("xe", "ぇ");
    m.insert("xo", "ぉ");
    m.insert("la", "ぁ");
    m.insert("li", "ぃ");
    m.insert("lu", "ぅ");
    m.insert("le", "ぇ");
    m.insert("lo", "ぉ");
    m.insert("xya", "ゃ");
    m.insert("lya", "ゃ");
    m.insert("xyu", "ゅ");
    m.insert("lyu", "ゅ");
    m.insert("xyo", "ょ");
    m.insert("lyo", "ょ");
    m.insert("xtu", "っ");
    m.insert("xtsu", "っ");
    m.insert("ltu", "っ");
    m.insert("ltsu", "っ");
    m.insert("xwa", "ゎ");
    m.insert("lwa", "ゎ");

    // 特殊
    m.insert("-", "ー");
    m.insert(".", "。");
    m.insert(",", "、");
    m.insert("!", "！");
    m.insert("?", "？");

    // ファ行
    m.insert("fa", "ふぁ");
    m.insert("fi", "ふぃ");
    m.insert("fe", "ふぇ");
    m.insert("fo", "ふぉ");

    // ティ・ディ
    m.insert("thi", "てぃ");
    m.insert("dhi", "でぃ");
    m.insert("thu", "てゅ");
    m.insert("dhu", "でゅ");

    // ヴァ行
    m.insert("va", "ゔぁ");
    m.insert("vi", "ゔぃ");
    m.insert("vu", "ゔ");
    m.insert("ve", "ゔぇ");
    m.insert("vo", "ゔぉ");

    m
});

/// ローマ字からかなへの変換器
#[derive(Debug, Clone, Default)]
pub struct RomajiConverter {
    /// 未確定のローマ字バッファ
    buffer: String,
}

impl RomajiConverter {
    /// 新しいコンバーターを作成
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 入力を追加してかな変換を試みる
    ///
    /// # 引数
    /// * `input` - 追加するローマ字文字
    ///
    /// # 戻り値
    /// 変換されたかな文字列（変換できない場合は空文字列）
    pub fn input(&mut self, c: char) -> String {
        let c_lower = c.to_ascii_lowercase();
        self.buffer.push(c_lower);

        // 促音の処理（同じ子音が続く場合）
        // 例: "tt" → "っ" を出力し、バッファには "t" を残す
        if self.buffer.len() >= 2 {
            let chars: Vec<char> = self.buffer.chars().collect();
            let len = chars.len();
            if len >= 2
                && chars[len - 2] == chars[len - 1]
                && !is_vowel(chars[len - 1])
                && chars[len - 1] != 'n'
            {
                // バッファには子音1文字だけを残す（っを入れない）
                self.buffer = chars[len - 1].to_string();
                return "っ".to_string();
            }
        }

        // 最長一致でテーブルを検索
        if let Some(kana) = self.try_convert() {
            return kana;
        }

        String::new()
    }

    /// バッファの内容を変換を試みる
    fn try_convert(&mut self) -> Option<String> {
        // 「n」の特別処理
        if self.buffer == "n" {
            return None; // まだ確定しない
        }

        // 「n」の後に子音が来た場合（母音とy以外）
        // 例: "nt" → "ん" + "t", "nn" → "ん" + "n"（nannaをかんなに変換するため）
        if self.buffer.len() >= 2 && self.buffer.starts_with('n') {
            let second = self.buffer.chars().nth(1).unwrap();
            if second != 'y' && !is_vowel(second) {
                let rest: String = self.buffer.chars().skip(1).collect();
                self.buffer = rest;
                return Some("ん".to_string());
            }
        }

        // テーブルから検索（最長一致）
        // 文字単位でスライスしてUTF-8境界問題を回避
        let chars: Vec<char> = self.buffer.chars().collect();
        for len in (1..=chars.len()).rev() {
            let prefix: String = chars[..len].iter().collect();
            if let Some(&kana) = ROMAJI_TABLE.get(prefix.as_str()) {
                self.buffer = chars[len..].iter().collect();
                return Some(kana.to_string());
            }
        }

        // バッファが長すぎる場合は先頭を破棄
        if chars.len() > 4 {
            self.buffer = chars[1..].iter().collect();
            return Some(chars[0].to_string());
        }

        None
    }

    /// バッファを強制的に確定する
    ///
    /// 入力終了時などに使用
    pub fn flush(&mut self) -> String {
        if self.buffer.is_empty() {
            return String::new();
        }

        // 「n」単体は「ん」に変換
        if self.buffer == "n" {
            self.buffer.clear();
            return "ん".to_string();
        }

        // テーブルから変換を試みる
        if let Some(&kana) = ROMAJI_TABLE.get(self.buffer.as_str()) {
            self.buffer.clear();
            return kana.to_string();
        }

        // 変換できない場合はそのまま返す
        let result = self.buffer.clone();
        self.buffer.clear();
        result
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// 現在のバッファ内容を取得
    #[must_use]
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// 文字列全体を一括変換
    pub fn convert(&mut self, input: &str) -> Result<String> {
        let mut result = String::new();

        for c in input.chars() {
            result.push_str(&self.input(c));
        }
        result.push_str(&self.flush());

        Ok(result)
    }
}

/// 母音かどうかを判定
fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_conversion() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("aiueo").unwrap(), "あいうえお");
    }

    #[test]
    fn test_ka_gyou() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kakikukeko").unwrap(), "かきくけこ");
    }

    #[test]
    fn test_sokuon() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kitte").unwrap(), "きって");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("nippon").unwrap(), "にっぽん");
    }

    #[test]
    fn test_n_handling() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kanna").unwrap(), "かんな");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kantan").unwrap(), "かんたん");
    }

    #[test]
    fn test_youon() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kyouto").unwrap(), "きょうと");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("shashin").unwrap(), "しゃしん");
    }

    #[test]
    fn test_nihongo() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("nihongo").unwrap(), "にほんご");
    }
}
