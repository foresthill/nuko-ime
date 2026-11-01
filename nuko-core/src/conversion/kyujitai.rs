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

use super::{Candidate, CandidateList, CandidateSource};

/// 旧字体候補に付けるスコア (新字体より十分低く、末尾付近に出す)。
const KYUJITAI_SCORE: i32 = -80;

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
    // ── seed 拡張 (立命館「新・旧字対照表」から取得・検証した 1:1 対応) ──
    // https://www.ritsumei.ac.jp/acd/cg/lt/cl/koten/newold.htm
    // 全 274 対のうち、1 新字→複数旧字 (弁/辺) と、文脈依存で単純置換が壊す字
    // (芸/予/余/糸/欠/台/画/虫/缶) を除外した 234 対。既出 25 字との重複も除外済み。
    ('亜', '亞'),
    ('悪', '惡'),
    ('圧', '壓'),
    ('囲', '圍'),
    ('壱', '壹'),
    ('稲', '稻'),
    ('飲', '飮'),
    ('隠', '隱'),
    ('営', '營'),
    ('栄', '榮'),
    ('衛', '衞'),
    ('駅', '驛'),
    ('艶', '艷'),
    ('塩', '鹽'),
    ('奥', '奧'),
    ('応', '應'),
    ('欧', '歐'),
    ('殴', '毆'),
    ('穏', '穩'),
    ('仮', '假'),
    ('価', '價'),
    ('壊', '壞'),
    ('懐', '懷'),
    ('絵', '繪'),
    ('拡', '擴'),
    ('殻', '殼'),
    ('覚', '覺'),
    ('岳', '嶽'),
    ('楽', '樂'),
    ('勧', '勸'),
    ('巻', '卷'),
    ('歓', '歡'),
    ('観', '觀'),
    ('関', '關'),
    ('陥', '陷'),
    ('巌', '巖'),
    ('顔', '顏'),
    ('帰', '歸'),
    ('亀', '龜'),
    ('偽', '僞'),
    ('戯', '戲'),
    ('犠', '犧'),
    ('拠', '據'),
    ('挙', '擧'),
    ('峡', '峽'),
    ('挟', '挾'),
    ('狭', '狹'),
    ('尭', '堯'),
    ('暁', '曉'),
    ('区', '區'),
    ('駆', '驅'),
    ('勲', '勳'),
    ('径', '徑'),
    ('恵', '惠'),
    ('渓', '溪'),
    ('経', '經'),
    ('継', '繼'),
    ('茎', '莖'),
    ('蛍', '螢'),
    ('軽', '輕'),
    ('鶏', '鷄'),
    ('倹', '儉'),
    ('剣', '劍'),
    ('圏', '圈'),
    ('検', '檢'),
    ('権', '權'),
    ('献', '獻'),
    ('県', '縣'),
    ('険', '險'),
    ('顕', '顯'),
    ('験', '驗'),
    ('厳', '嚴'),
    ('効', '效'),
    ('恒', '恆'),
    ('鉱', '鑛'),
    ('号', '號'),
    ('済', '濟'),
    ('砕', '碎'),
    ('斎', '齋'),
    ('剤', '劑'),
    ('桜', '櫻'),
    ('冊', '册'),
    ('雑', '雜'),
    ('参', '參'),
    ('惨', '慘'),
    ('桟', '棧'),
    ('蚕', '蠶'),
    ('賛', '贊'),
    ('残', '殘'),
    ('辞', '辭'),
    ('湿', '濕'),
    ('舎', '舍'),
    ('釈', '釋'),
    ('寿', '壽'),
    ('収', '收'),
    ('従', '從'),
    ('渋', '澁'),
    ('獣', '獸'),
    ('縦', '縱'),
    ('粛', '肅'),
    ('処', '處'),
    ('叙', '敍'),
    ('奨', '奬'),
    ('将', '將'),
    ('焼', '燒'),
    ('称', '稱'),
    ('証', '證'),
    ('乗', '乘'),
    ('剰', '剩'),
    ('壌', '壤'),
    ('嬢', '孃'),
    ('条', '條'),
    ('浄', '淨'),
    ('畳', '疊'),
    ('穣', '穰'),
    ('譲', '讓'),
    ('醸', '釀'),
    ('嘱', '囑'),
    ('触', '觸'),
    ('寝', '寢'),
    ('慎', '愼'),
    ('晋', '晉'),
    ('尽', '盡'),
    ('図', '圖'),
    ('粋', '粹'),
    ('酔', '醉'),
    ('随', '隨'),
    ('髄', '髓'),
    ('枢', '樞'),
    ('声', '聲'),
    ('静', '靜'),
    ('斉', '齊'),
    ('摂', '攝'),
    ('窃', '竊'),
    ('専', '專'),
    ('浅', '淺'),
    ('潜', '潛'),
    ('繊', '纖'),
    ('践', '踐'),
    ('銭', '錢'),
    ('禅', '禪'),
    ('双', '雙'),
    ('壮', '壯'),
    ('捜', '搜'),
    ('挿', '插'),
    ('争', '爭'),
    ('総', '總'),
    ('聡', '聰'),
    ('荘', '莊'),
    ('装', '裝'),
    ('騒', '騷'),
    ('臓', '臟'),
    ('蔵', '藏'),
    ('属', '屬'),
    ('続', '續'),
    ('堕', '墮'),
    ('帯', '帶'),
    ('滞', '滯'),
    ('滝', '瀧'),
    ('択', '擇'),
    ('沢', '澤'),
    ('単', '單'),
    ('担', '擔'),
    ('胆', '膽'),
    ('団', '團'),
    ('弾', '彈'),
    ('断', '斷'),
    ('痴', '癡'),
    ('遅', '遲'),
    ('昼', '晝'),
    ('鋳', '鑄'),
    ('庁', '廳'),
    ('聴', '聽'),
    ('鎮', '鎭'),
    ('逓', '遞'),
    ('鉄', '鐵'),
    ('転', '轉'),
    ('点', '點'),
    ('伝', '傳'),
    ('盗', '盜'),
    ('灯', '燈'),
    ('闘', '鬪'),
    ('届', '屆'),
    ('縄', '繩'),
    ('弐', '貳'),
    ('悩', '惱'),
    ('脳', '腦'),
    ('廃', '廢'),
    ('拝', '拜'),
    ('麦', '麥'),
    ('髪', '髮'),
    ('抜', '拔'),
    ('蛮', '蠻'),
    ('秘', '祕'),
    ('浜', '濱'),
    ('払', '拂'),
    ('仏', '佛'),
    ('並', '竝'),
    ('舗', '舖'),
    ('穂', '穗'),
    ('宝', '寶'),
    ('豊', '豐'),
    ('没', '沒'),
    ('槙', '槇'),
    ('万', '萬'),
    ('満', '滿'),
    ('黙', '默'),
    ('弥', '彌'),
    ('薬', '藥'),
    ('訳', '譯'),
    ('薮', '藪'),
    ('与', '與'),
    ('誉', '譽'),
    ('揺', '搖'),
    ('様', '樣'),
    ('謡', '謠'),
    ('遥', '遙'),
    ('乱', '亂'),
    ('覧', '覽'),
    ('竜', '龍'),
    ('両', '兩'),
    ('猟', '獵'),
    ('塁', '壘'),
    ('励', '勵'),
    ('隷', '隸'),
    ('霊', '靈'),
    ('齢', '齡'),
    ('恋', '戀'),
    ('炉', '爐'),
    ('労', '勞'),
    ('楼', '樓'),
    ('禄', '祿'),
    ('湾', '灣'),
    ('瑶', '瑤'),
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

/// 候補リストの各候補について旧字体版を作り、**末尾に追加** する (旧字体モード用)。
///
/// - 旧字体化できない候補 (かな/対象外) はスキップ ([`kyujitai_variant`] が `None`)。
/// - 既に同じ表層があればスキップ (重複を出さない)。
/// - 追加する候補は低スコア (`KYUJITAI_SCORE`) なので、新字体の既定を押し下げない。
pub fn append_kyujitai_variants(candidates: &mut CandidateList) {
    // iterate 中に push しないよう、先に (旧字体表層, 読み) を収集する。
    let variants: Vec<(String, String)> = candidates
        .iter()
        .filter_map(|c| kyujitai_variant(&c.surface).map(|old| (old, c.reading.clone())))
        .collect();
    for (surface, reading) in variants {
        if candidates.iter().any(|c| c.surface == surface) {
            continue;
        }
        candidates.push(
            Candidate::new(&surface, &reading)
                .with_score(KYUJITAI_SCORE)
                .with_source(CandidateSource::System),
        );
    }
}

/// [`append_kyujitai_variants`] の `Vec<Candidate>` 版 (文節候補=segment 用)。
/// ロジックは同一: 旧字体版を末尾に低スコアで追加、重複/変化なしは除く。
pub fn append_kyujitai_variants_vec(candidates: &mut Vec<Candidate>) {
    let variants: Vec<(String, String)> = candidates
        .iter()
        .filter_map(|c| kyujitai_variant(&c.surface).map(|old| (old, c.reading.clone())))
        .collect();
    for (surface, reading) in variants {
        if candidates.iter().any(|c| c.surface == surface) {
            continue;
        }
        candidates.push(
            Candidate::new(&surface, &reading)
                .with_score(KYUJITAI_SCORE)
                .with_source(CandidateSource::System),
        );
    }
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

    #[test]
    fn append_variants_adds_old_forms_at_end() {
        let mut list = CandidateList::new();
        list.push(Candidate::new("国語", "こくご"));
        list.push(Candidate::new("こくご", "こくご")); // かな候補 (対象字なし)
        append_kyujitai_variants(&mut list);
        assert!(list.iter().any(|c| c.surface == "國語"), "旧字体版を追加");
        assert_eq!(
            list.iter().filter(|c| c.surface == "國語").count(),
            1,
            "重複追加しない"
        );
        // かな候補からは旧字体が生まれない。
        assert!(list
            .iter()
            .all(|c| c.surface != "こくご" || c.reading == "こくご"));
    }

    /// seed 拡張 (立命館) で増えた代表的な 1:1 対応が効く。
    #[test]
    fn expanded_pairs_convert() {
        assert_eq!(to_kyujitai("観"), "觀");
        assert_eq!(to_kyujitai("関"), "關");
        assert_eq!(to_kyujitai("権利"), "權利");
        assert_eq!(to_kyujitai("検査"), "檢査"); // 査 は対象外→不変
        assert_eq!(to_kyujitai("鉄道"), "鐵道");
        assert_eq!(to_kyujitai("図書"), "圖書");
        assert_eq!(to_kyujitai("恋愛"), "戀愛"); // 愛 は対象外→不変
    }

    /// ★ 文脈依存・多対応で除外した字は **変換しない** (誤置換で語を壊さない)。
    #[test]
    fn context_dependent_kanji_stay_unchanged() {
        assert_eq!(to_kyujitai("弁当"), "弁當", "弁は除外(辨/辯/瓣)、当→當のみ");
        assert_eq!(to_kyujitai("周辺"), "周辺", "辺は除外(邊/邉)");
        assert_eq!(to_kyujitai("芸術"), "芸術", "芸は除外(藝だが文脈依存)");
        assert_eq!(to_kyujitai("区画"), "區画", "画は除外(畫/劃)、区→區のみ");
        assert_eq!(to_kyujitai("昆虫"), "昆虫", "虫は除外(蟲)→不変");
    }

    /// ★ テーブル健全性: 新字キーの重複なし・恒等対応 (新==旧) なし。
    #[test]
    fn table_is_well_formed() {
        let mut keys: Vec<char> = NEW_TO_OLD.iter().map(|(n, _)| *n).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            n,
            "新字キーに重複がある (単純置換が非決定的になる)"
        );
        assert!(
            NEW_TO_OLD.iter().all(|(nw, old)| nw != old),
            "恒等対応 (新==旧) が混入している",
        );
    }

    #[test]
    fn append_variants_no_dup_when_already_present() {
        let mut list = CandidateList::new();
        list.push(Candidate::new("体", "からだ"));
        list.push(Candidate::new("體", "からだ")); // 既に旧字体がある
        append_kyujitai_variants(&mut list);
        assert_eq!(
            list.iter().filter(|c| c.surface == "體").count(),
            1,
            "既にある旧字体は重複追加しない"
        );
    }
}
