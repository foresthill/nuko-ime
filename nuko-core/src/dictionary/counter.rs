//! 助数詞（カウンター）音便形を **規則から決定論生成** する。
//!
//! 日本語の助数詞の音便は規則的（促音便・濁音・半濁音）で、個別に辞書登録するのは
//! 破綻する（本/杯/匹… × 数 × 音便 で数千通り）。そこで
//! 「数 (1-10, 何) × 助数詞 × 音便規則」から `(表層, 読み)` を網羅生成する。
//! これは「個別パッチ」ではなく **ルールエンジン**。
//! 出力は静的辞書にマージされ、`has_dict_word` 経由で変換候補の 1 位に出せる
//! （再訓練不要）。
//!
//! ## 音便規則
//! - **促音便**: 1,6,8,10 の後、か/さ/た/は行の助数詞で数側が「っ」になる
//!   (いち→いっ, ろく→ろっ, はち→はっ, じゅう→じゅっ/じっ)。
//! - **半濁音**: 促音便が起きた は行助数詞は半濁音化 (ほん→ぽん)。例 いっぽん/じゅっぽん。
//! - **濁音**: 3,何 の後の は行助数詞は濁音化 (ほん→ぼん)。例 さんぼん/なんぼん。促音便なし。
//! - **無変化**: 2,4,5,7,9 と、か/さ行助数詞の濁/半濁は原則なし（数側の促音のみ）。
//!
//! 不規則なもの（分=ぷん・階=がい・人=ひとり・日 など）は本モジュールから除外し、
//! 必要なら静的辞書で個別に扱う。ここは「規則で正しく出せる範囲」に限定する。

use std::collections::{BTreeMap, BTreeSet};

/// 数: (漢数字, 基本読み, 促音読み, 促音を起こす行, は行変化)。
/// 促音読み `Some` のとき、`促音を起こす行` に該当する助数詞で数側が促音化する。
/// は行変化 `'p'`=半濁音(ぱ)、`'b'`=濁音(ば)、`None`=変化なし。
type Num = (
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
    Option<char>,
);

/// 数の定義（1-10, 何、および 10 の別形 じっ）。
const NUMBERS: &[Num] = &[
    ("一", "いち", Some("いっ"), "HKST", Some('p')),
    ("二", "に", None, "", None),
    ("三", "さん", None, "", Some('b')),
    ("四", "よん", None, "", None),
    ("五", "ご", None, "", None),
    ("六", "ろく", Some("ろっ"), "HK", Some('p')),
    ("七", "なな", None, "", None),
    ("八", "はち", Some("はっ"), "HKST", Some('p')),
    ("九", "きゅう", None, "", None),
    ("十", "じゅう", Some("じゅっ"), "HKST", Some('p')),
    ("何", "なん", None, "", Some('b')),
    ("十", "じゅう", Some("じっ"), "HKST", Some('p')), // 口語別形 じっぽん/じっかい
];

/// 助数詞: (漢字, 基本読み, 行)。行 = H(は行)/K(か行)/S(さ行)/N(無変化)。
/// 不規則 (分/階/人/日…) は含めない。
const COUNTERS: &[(&str, &str, char)] = &[
    ("本", "ほん", 'H'),
    ("杯", "はい", 'H'),
    ("匹", "ひき", 'H'),
    ("票", "ひょう", 'H'),
    ("泊", "はく", 'H'),
    ("編", "へん", 'H'),
    ("回", "かい", 'K'),
    ("個", "こ", 'K'),
    ("課", "か", 'K'),
    ("校", "こう", 'K'),
    ("軒", "けん", 'K'),
    ("冊", "さつ", 'S'),
    ("歳", "さい", 'S'),
    ("足", "そく", 'S'),
    ("隻", "せき", 'S'),
    ("枚", "まい", 'N'),
    ("台", "だい", 'N'),
    ("番", "ばん", 'N'),
    ("円", "えん", 'N'),
    ("年", "ねん", 'N'),
];

/// は行の先頭かなを半濁音化 (は→ぱ)。
fn handaku(c: char) -> char {
    match c {
        'は' => 'ぱ',
        'ひ' => 'ぴ',
        'ふ' => 'ぷ',
        'へ' => 'ぺ',
        'ほ' => 'ぽ',
        other => other,
    }
}

/// は行の先頭かなを濁音化 (は→ば)。
fn daku(c: char) -> char {
    match c {
        'は' => 'ば',
        'ひ' => 'び',
        'ふ' => 'ぶ',
        'へ' => 'べ',
        'ほ' => 'ぼ',
        other => other,
    }
}

/// 助数詞側の読みを音便規則で変換する。
fn counter_yomi(cyomi: &str, row: char, use_sokuon: bool, h_mode: Option<char>) -> String {
    if row != 'H' {
        return cyomi.to_string(); // か/さ/無変化行は先頭変化なし
    }
    let mut chars = cyomi.chars();
    let Some(first) = chars.next() else {
        return cyomi.to_string();
    };
    let rest: String = chars.collect();
    if use_sokuon && h_mode == Some('p') {
        format!("{}{}", handaku(first), rest) // 半濁音
    } else if !use_sokuon && h_mode == Some('b') {
        format!("{}{}", daku(first), rest) // 濁音
    } else {
        cyomi.to_string()
    }
}

/// `(表層, 読み)` を規則から生成する（重複排除・読み昇順）。
fn generate() -> Vec<(String, String)> {
    let mut set: BTreeSet<(String, String)> = BTreeSet::new();
    for &(kanji_num, base, sokuon, rows, h_mode) in NUMBERS {
        for &(ckanji, cyomi, row) in COUNTERS {
            let use_sokuon = rows.contains(row) && sokuon.is_some();
            let num_yomi = if use_sokuon { sokuon.unwrap() } else { base };
            let cy = counter_yomi(cyomi, row, use_sokuon, h_mode);
            let yomi = format!("{num_yomi}{cy}");
            let surface = format!("{kanji_num}{ckanji}");
            if surface != yomi {
                set.insert((surface, yomi));
            }
        }
    }
    set.into_iter().collect()
}

/// 静的辞書マージ用のエントリ列: 読み → [(表層, 品詞)]。
///
/// 同じ読みに複数表層が付くことがある（例: じゅっぽん/じっぽん は別読みだが
/// 十本 を指す）ので、読みごとにまとめて返す。
#[must_use]
pub fn counter_entries() -> Vec<(String, Vec<(String, &'static str)>)> {
    let mut map: BTreeMap<String, Vec<(String, &'static str)>> = BTreeMap::new();
    for (surface, yomi) in generate() {
        map.entry(yomi).or_default().push((surface, "名詞"));
    }
    map.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 読み→表層の検索ヘルパ。
    fn lookup(yomi: &str) -> Vec<String> {
        counter_entries()
            .into_iter()
            .find(|(y, _)| y == yomi)
            .map(|(_, surfaces)| surfaces.into_iter().map(|(s, _)| s).collect())
            .unwrap_or_default()
    }

    /// ★ 音便規則が正しい（促音便・半濁音・濁音・無変化）。
    #[test]
    fn sound_changes_are_correct() {
        // は行 + 促音便 → 半濁音
        assert!(
            lookup("いっぽん").contains(&"一本".to_string()),
            "1本=いっぽん"
        );
        assert!(
            lookup("ろっぽん").contains(&"六本".to_string()),
            "6本=ろっぽん"
        );
        assert!(
            lookup("はっぽん").contains(&"八本".to_string()),
            "8本=はっぽん"
        );
        assert!(
            lookup("じゅっぽん").contains(&"十本".to_string()),
            "10本=じゅっぽん"
        );
        assert!(
            lookup("じっぽん").contains(&"十本".to_string()),
            "10本=じっぽん(別形)"
        );
        // は行 + 3/何 → 濁音
        assert!(
            lookup("さんぼん").contains(&"三本".to_string()),
            "3本=さんぼん"
        );
        assert!(
            lookup("なんぼん").contains(&"何本".to_string()),
            "何本=なんぼん"
        );
        // は行 + 無変化
        assert!(lookup("にほん").contains(&"二本".to_string()), "2本=にほん");
        // 杯/匹 も同規則
        assert!(
            lookup("いっぱい").contains(&"一杯".to_string()),
            "1杯=いっぱい"
        );
        assert!(
            lookup("さんばい").contains(&"三杯".to_string()),
            "3杯=さんばい"
        );
        assert!(
            lookup("さんびき").contains(&"三匹".to_string()),
            "3匹=さんびき"
        );
        // か行 = 促音のみ（先頭変化なし）
        assert!(
            lookup("いっかい").contains(&"一回".to_string()),
            "1回=いっかい"
        );
        assert!(
            lookup("じゅっかい").contains(&"十回".to_string()),
            "10回=じゅっかい"
        );
        assert!(
            lookup("さんかい").contains(&"三回".to_string()),
            "3回=さんかい"
        );
        // さ行 = 促音のみ
        assert!(
            lookup("はっさつ").contains(&"八冊".to_string()),
            "8冊=はっさつ"
        );
        // 無変化行
        assert!(
            lookup("いちまい").contains(&"一枚".to_string()),
            "1枚=いちまい"
        );
        assert!(
            lookup("じゅうまい").contains(&"十枚".to_string()),
            "10枚=じゅうまい"
        );
    }

    /// ★ 決定論・churn-free: 同じ出力が常に返る。
    #[test]
    fn generation_is_deterministic() {
        assert_eq!(generate(), generate());
        // 一定数以上を生成する（本/杯/… × 数 × 音便）。
        assert!(generate().len() > 150, "十分な網羅: {}", generate().len());
    }

    /// ★ 恒等 (表層==読み) は含めない。
    #[test]
    fn no_identity_entries() {
        assert!(generate().iter().all(|(s, y)| s != y));
    }
}
