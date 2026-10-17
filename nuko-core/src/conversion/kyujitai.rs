//! 旧字体モード: 新字体の表層を歴史的な **旧字体** へ変換する (opt-in / 既定 OFF)。
//!
//! # 位置づけ (思想的中立の設計)
//! nuko は **既定では新字体のみ**。旧字体は「旧字体モード」を ON にしたユーザーにだけ
//! 候補として出す (中立な製品のまま、使いたい人が選ぶ)。和多志 のような現代の当て字
//! (スピリチュアル ateji) は本モジュールに **含めない** — それはユーザーの個人
//! 単語登録 (correction 学習) で各自が入れるもので、製品が同梱して思想を帯びないため。
//! 正統な旧字体 (國/體/學/禮…) は 1946 年の字体整理以前の正式字で、文化的・中立。
//!
//! # 正確性 (曖昧な対応は入れない)
//! 新字体→旧字体は **1 対 1 とは限らない**。例: 弁 → 辨 / 瓣 / 辯 / 辦 (語により別字)。
//! 単純な文字置換で壊れるこうした字は **本テーブルに含めない** (文脈=辞書が要るため)。
//! ここに載せるのは **曖昧性のない 1:1 対応のみ**。より網羅的な対応は新旧字体対照表
//! から生成して拡張する (seed → full)。
//!
//! 参考: 立命館 新旧字対照表 / asahi-net old_chara。

/// 新字体 → 旧字体 の **曖昧性のない 1:1 対応** (seed)。
///
/// 1 新字体が複数の旧字体に対応し得る字 (弁/芸/予/余/糸/欠/台 等) は **含めない**。
const NEW_TO_OLD: &[(char, char)] = &[
    ('国', '國'),
    ('学', '學'),
    ('体', '體'),
    ('礼', '禮'),
    ('会', '會'),
    ('医', '醫'),
    ('円', '圓'),
    ('旧', '舊'),
    ('当', '當'),
    ('写', '寫'),
    ('実', '實'),
    ('戦', '戰'),
    ('読', '讀'),
    ('発', '發'),
    ('売', '賣'),
    ('変', '變'),
    ('対', '對'),
    ('独', '獨'),
    ('広', '廣'),
    ('気', '氣'),
    ('歯', '齒'),
    ('来', '來'),
    ('数', '數'),
    ('児', '兒'),
    ('党', '黨'),
    ('真', '眞'),
];

/// 文字列中の新字体を旧字体へ置換する。対応の無い文字はそのまま返す。
#[must_use]
pub fn to_kyujitai(s: &str) -> String {
    s.chars()
        .map(|c| {
            NEW_TO_OLD
                .iter()
                .find(|(new, _)| *new == c)
                .map_or(c, |(_, old)| *old)
        })
        .collect()
}

/// 旧字体へ変換した結果が元と **異なる** なら `Some(旧字体)` を返す。
///
/// 変換後が元と同じ (旧字体化できる字が無い) なら `None`。
/// 旧字体モードで「追加候補として出すべきか」の判定に使う (= None なら足さない)。
#[must_use]
pub fn kyujitai_variant(s: &str) -> Option<String> {
    let out = to_kyujitai(s);
    (out != s).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_char_confirmed_pairs() {
        // ソース (chiba-kc / asahi-net 対照表) で確認済みの 4 対。
        assert_eq!(to_kyujitai("国"), "國");
        assert_eq!(to_kyujitai("学"), "學");
        assert_eq!(to_kyujitai("体"), "體");
        assert_eq!(to_kyujitai("礼"), "禮");
    }

    #[test]
    fn word_level_substitution() {
        // 語中: 対応する字だけ置換し、他はそのまま。
        assert_eq!(to_kyujitai("国語"), "國語"); // 語 は対象外→不変
        assert_eq!(to_kyujitai("学校"), "學校"); // 校 は対象外→不変
        assert_eq!(to_kyujitai("体育"), "體育");
        assert_eq!(to_kyujitai("礼儀"), "禮儀");
    }

    #[test]
    fn no_change_when_no_mapped_kanji() {
        // 対応字が無ければ不変。かな・対象外漢字。
        assert_eq!(to_kyujitai("日本語"), "日本語");
        assert_eq!(to_kyujitai("あいうえお"), "あいうえお");
    }

    #[test]
    fn variant_returns_some_only_on_change() {
        assert_eq!(kyujitai_variant("国語"), Some("國語".to_string()));
        assert_eq!(kyujitai_variant("体"), Some("體".to_string()));
        assert_eq!(kyujitai_variant("日本"), None); // 旧字体化できる字なし
        assert_eq!(kyujitai_variant("あいうえお"), None);
    }

    /// ★ 曖昧な字 (1新字→多旧字) は **含めない** ことの固定。
    /// 弁 は 辨/瓣/辯/辦 のどれかが語により正しいので、単純置換してはいけない。
    /// テーブルに無い = 不変、が正しい挙動 (誤置換で「弁当」を壊さない)。
    #[test]
    fn ambiguous_kanji_are_excluded() {
        assert_eq!(to_kyujitai("弁"), "弁", "弁 は曖昧なので置換しない");
        assert_eq!(to_kyujitai("弁当"), "弁當", "当→當 のみ。弁 は不変");
    }
}
