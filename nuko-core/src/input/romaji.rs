//! ローマ字→かな変換
//!
//! 変換テーブルは Google Mozc プロジェクト (BSD-3) の
//! `romanji-hiragana.tsv` を基盤として使用する (`super::mozc_table` 参照)。
//! Mozc の生のテーブルだけでは "kanna" → "かんな" のような
//! ケースが扱えないため、「n」周辺の特殊処理と促音の同子音重ね処理は
//! このファイルでカスタムロジックを保持する。

use crate::error::Result;
use crate::input::mozc_table::MOZC_TABLE;

/// ローマ字からかなへの変換器
#[derive(Debug, Clone, Default)]
pub struct RomajiConverter {
    /// 未確定のローマ字バッファ
    buffer: String,
    /// 直前の出力が "nn" ルール由来の "ん" だったか。
    /// flush 時に buffer="n" が残っていても、これが true なら
    /// 「nn 完結直後の余分な n」と判断して "ん" を再出力しない。
    last_was_nn_emit: bool,
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
                self.last_was_nn_emit = false;
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

        // 「n'」は特別処理（テーブルで直接マッチ）
        if self.buffer == "n'" {
            if let Some(entry) = MOZC_TABLE.get("n'") {
                self.buffer.clear();
                self.last_was_nn_emit = false;
                return Some(entry.kana.to_string());
            }
        }

        // テーブルから検索（最長一致）
        // 文字単位でスライスしてUTF-8境界問題を回避
        let chars: Vec<char> = self.buffer.chars().collect();

        // 「n」の後に子音が来た場合（母音とy以外）
        // 例: "nt" → "ん" + "t", "nn" → "ん" + "n"（kannaをかんなに変換するため）
        if chars.len() >= 2 && chars[0] == 'n' {
            let second = chars[1];
            if second != 'y' && !is_vowel(second) {
                self.buffer = chars[1..].iter().collect();
                // nn 由来の ん 出力 → buffer に残った "n" を flush で重複させない
                self.last_was_nn_emit = second == 'n';
                return Some("ん".to_string());
            }
        }

        // テーブルから検索（最長一致）
        // Mozc 3 列エントリ (例: "tch" → "っ" + next_state="ch") は
        // next_state を buffer に残して後続文字と結合させる。
        for len in (1..=chars.len()).rev() {
            let prefix: String = chars[..len].iter().collect();
            if let Some(entry) = MOZC_TABLE.get(prefix.as_str()) {
                let remaining: String = chars[len..].iter().collect();
                self.buffer = match entry.next_state {
                    Some(ns) => format!("{ns}{remaining}"),
                    None => remaining,
                };
                self.last_was_nn_emit = false;
                return Some(entry.kana.to_string());
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
            self.last_was_nn_emit = false;
            return String::new();
        }

        // nn 完結直後の余分な「n」は重複出力しない
        // 例: "henn" → 'n','n' で「ん」を出した後 buffer="n" が残るが、
        //     これを flush でさらに「ん」化すると "へんん" になってしまう
        if self.buffer == "n" && self.last_was_nn_emit {
            self.buffer.clear();
            self.last_was_nn_emit = false;
            return String::new();
        }

        // 「n」単体は「ん」に変換
        if self.buffer == "n" {
            self.buffer.clear();
            self.last_was_nn_emit = false;
            return "ん".to_string();
        }

        // テーブルから変換を試みる
        if let Some(entry) = MOZC_TABLE.get(self.buffer.as_str()) {
            self.buffer.clear();
            self.last_was_nn_emit = false;
            return entry.kana.to_string();
        }

        // 変換できない場合はそのまま返す
        let result = self.buffer.clone();
        self.buffer.clear();
        self.last_was_nn_emit = false;
        result
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.last_was_nn_emit = false;
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

    #[test]
    fn test_dakuon() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("gakkou").unwrap(), "がっこう");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("zannen").unwrap(), "ざんねん");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("denwa").unwrap(), "でんわ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("tabemono").unwrap(), "たべもの");
    }

    #[test]
    fn test_handakuon() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("panpan").unwrap(), "ぱんぱん");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("pinpon").unwrap(), "ぴんぽん");
    }

    #[test]
    fn test_small_kana() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("xtu").unwrap(), "っ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("ltu").unwrap(), "っ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("xa").unwrap(), "ぁ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("xya").unwrap(), "ゃ");
    }

    #[test]
    fn test_special_chars() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("-").unwrap(), "ー");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert(".").unwrap(), "。");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert(",").unwrap(), "、");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("ra-men").unwrap(), "らーめん");
    }

    #[test]
    fn test_n_apostrophe() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kan'i").unwrap(), "かんい");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("shin'you").unwrap(), "しんよう");
    }

    #[test]
    fn test_uppercase_input() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("NIHON").unwrap(), "にほん");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("ToKyO").unwrap(), "ときょ");
    }

    #[test]
    fn test_empty_input() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("").unwrap(), "");
    }

    #[test]
    fn test_complex_words() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("konnichiha").unwrap(), "こんにちは");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("arigatou").unwrap(), "ありがとう");

        let mut conv = RomajiConverter::new();
        assert_eq!(
            conv.convert("ohayougozaimasu").unwrap(),
            "おはようございます"
        );

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("sumimasen").unwrap(), "すみません");
    }

    #[test]
    fn test_fa_row() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("fairu").unwrap(), "ふぁいる");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("fea").unwrap(), "ふぇあ");
    }

    #[test]
    fn test_ji_zu_variations() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("ji").unwrap(), "じ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("zi").unwrap(), "じ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("zu").unwrap(), "ず");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("du").unwrap(), "づ");
    }

    #[test]
    fn test_chi_tsu_variations() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("chi").unwrap(), "ち");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("ti").unwrap(), "ち");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("tsu").unwrap(), "つ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("tu").unwrap(), "つ");
    }

    #[test]
    fn test_shi_variations() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("shi").unwrap(), "し");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("si").unwrap(), "し");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("shu").unwrap(), "しゅ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("sya").unwrap(), "しゃ");
    }

    // ===== Mozc テーブル導入で広がったカバレッジ =====

    #[test]
    fn test_v_row_foreign_sounds() {
        // ゔ行 (外来音) は Mozc TSV に含まれる
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("va").unwrap(), "ゔぁ");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("vu").unwrap(), "ゔ");
    }

    #[test]
    fn test_mozc_symbols() {
        // Mozc TSV の z プレフィックス記号
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("z/").unwrap(), "・");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("z.").unwrap(), "…");

        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("z[").unwrap(), "『");
    }

    #[test]
    fn test_n_edge_cases() {
        // henn → へん (末尾の n が ん として描画されるべきだが、内部は buffer="n")
        // RomajiConverter 単体では buffer に "n" が残るため flush で "ん" になる
        let mut conv = RomajiConverter::new();
        let mut result = String::new();
        for c in "henn".chars() {
            result.push_str(&conv.input(c));
        }
        result.push_str(&conv.flush());
        assert_eq!(result, "へん", "henn → へん (flush で末尾 n を ん に)");
    }

    #[test]
    fn test_kanna_keeps_n_in_buffer() {
        // kanna → かんな
        // 1つ目の n は buffer、2つ目で ん 出力 + buffer に n、a で な
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("kanna").unwrap(), "かんな");
    }

    #[test]
    fn test_hon_ya_apostrophe() {
        // 本屋 = ほんや
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("hon'ya").unwrap(), "ほんや");
    }

    #[test]
    fn test_long_sokuon_chains() {
        // matcha = まっちゃ
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("matcha").unwrap(), "まっちゃ");

        // jisshuu = じっしゅう
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.convert("jisshuu").unwrap(), "じっしゅう");
    }

    #[test]
    fn test_consecutive_words() {
        // 文章レベルの連続入力
        let mut conv = RomajiConverter::new();
        assert_eq!(
            conv.convert("watashihagakuseidesu").unwrap(),
            "わたしはがくせいです"
        );
    }
}
