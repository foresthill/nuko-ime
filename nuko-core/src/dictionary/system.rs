//! システム辞書
//!
//! 静的辞書による基本的な辞書検索を提供します。
//! `lindera` featureを有効にすると、形態素解析機能も利用可能になります。

use crate::conversion::Candidate;
use crate::error::{NukoError, Result};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "lindera")]
use lindera::{
    dictionary::{load_dictionary_from_kind, DictionaryKind},
    mode::Mode,
    tokenizer::Tokenizer,
};

/// 読み→表層形のマッピングを構築するための静的辞書データ
/// linderaの辞書から抽出した基本的な変換候補
static READING_TO_SURFACE: Lazy<HashMap<String, Vec<(String, String)>>> = Lazy::new(|| {
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();

    // 基本的な単語マッピング（品詞情報付き）
    // 形式: (読み, [(表層形, 品詞), ...])
    let entries = [
        // 名詞 - 一般
        ("にほん", vec![("日本", "名詞"), ("二本", "名詞")]),
        ("にほんご", vec![("日本語", "名詞")]),
        ("にほんじん", vec![("日本人", "名詞")]),
        ("ぬこ", vec![("ぬこ", "名詞"), ("猫", "名詞")]),
        ("ねこ", vec![("猫", "名詞"), ("ネコ", "名詞")]),
        ("いぬ", vec![("犬", "名詞"), ("イヌ", "名詞")]),
        ("いめ", vec![("IME", "名詞")]),
        ("にゅうりょく", vec![("入力", "名詞")]),
        ("しゅつりょく", vec![("出力", "名詞")]),
        ("へんかん", vec![("変換", "名詞"), ("返還", "名詞")]),
        ("かんじ", vec![("漢字", "名詞"), ("幹事", "名詞"), ("感じ", "名詞")]),
        ("ひらがな", vec![("平仮名", "名詞"), ("ひらがな", "名詞")]),
        ("かたかな", vec![("片仮名", "名詞"), ("カタカナ", "名詞")]),
        ("もじ", vec![("文字", "名詞")]),
        ("ことば", vec![("言葉", "名詞")]),
        ("ぶんしょう", vec![("文章", "名詞")]),
        // 地名
        ("とうきょう", vec![("東京", "名詞")]),
        ("おおさか", vec![("大阪", "名詞")]),
        ("きょうと", vec![("京都", "名詞")]),
        ("なごや", vec![("名古屋", "名詞")]),
        ("ふくおか", vec![("福岡", "名詞")]),
        ("ほっかいどう", vec![("北海道", "名詞")]),
        ("よこはま", vec![("横浜", "名詞")]),
        ("こうべ", vec![("神戸", "名詞")]),
        ("さっぽろ", vec![("札幌", "名詞")]),
        ("せんだい", vec![("仙台", "名詞")]),
        ("ひろしま", vec![("広島", "名詞")]),
        ("おきなわ", vec![("沖縄", "名詞")]),
        // 挨拶
        ("こんにちは", vec![("こんにちは", "感動詞"), ("今日は", "感動詞")]),
        ("こんばんは", vec![("こんばんは", "感動詞"), ("今晩は", "感動詞")]),
        ("おはよう", vec![("おはよう", "感動詞"), ("お早う", "感動詞")]),
        ("おはようございます", vec![("おはようございます", "感動詞")]),
        ("ありがとう", vec![("ありがとう", "感動詞"), ("有難う", "感動詞")]),
        ("ありがとうございます", vec![("ありがとうございます", "感動詞")]),
        ("すみません", vec![("すみません", "感動詞"), ("済みません", "感動詞")]),
        ("ごめんなさい", vec![("ごめんなさい", "感動詞")]),
        ("さようなら", vec![("さようなら", "感動詞"), ("左様なら", "感動詞")]),
        ("おやすみなさい", vec![("おやすみなさい", "感動詞")]),
        ("いただきます", vec![("いただきます", "感動詞"), ("頂きます", "感動詞")]),
        ("ごちそうさま", vec![("ごちそうさま", "感動詞"), ("御馳走様", "感動詞")]),
        // 代名詞
        ("わたし", vec![("私", "代名詞"), ("わたし", "代名詞")]),
        ("わたくし", vec![("私", "代名詞")]),
        ("ぼく", vec![("僕", "代名詞")]),
        ("おれ", vec![("俺", "代名詞")]),
        ("あなた", vec![("あなた", "代名詞"), ("貴方", "代名詞"), ("貴女", "代名詞")]),
        ("きみ", vec![("君", "代名詞")]),
        ("かれ", vec![("彼", "代名詞")]),
        ("かのじょ", vec![("彼女", "代名詞")]),
        ("これ", vec![("これ", "代名詞"), ("此れ", "代名詞")]),
        ("それ", vec![("それ", "代名詞"), ("其れ", "代名詞")]),
        ("あれ", vec![("あれ", "代名詞"), ("彼れ", "代名詞")]),
        ("どれ", vec![("どれ", "代名詞")]),
        ("ここ", vec![("ここ", "代名詞"), ("此処", "代名詞")]),
        ("そこ", vec![("そこ", "代名詞"), ("其処", "代名詞")]),
        ("あそこ", vec![("あそこ", "代名詞"), ("彼処", "代名詞")]),
        ("どこ", vec![("どこ", "代名詞"), ("何処", "代名詞")]),
        // 時間
        ("いま", vec![("今", "名詞"), ("居間", "名詞")]),
        ("きょう", vec![("今日", "名詞"), ("京", "名詞")]),
        ("あした", vec![("明日", "名詞")]),
        ("あす", vec![("明日", "名詞")]),
        ("きのう", vec![("昨日", "名詞")]),
        ("おととい", vec![("一昨日", "名詞")]),
        ("あさって", vec![("明後日", "名詞")]),
        ("こんしゅう", vec![("今週", "名詞")]),
        ("らいしゅう", vec![("来週", "名詞")]),
        ("せんしゅう", vec![("先週", "名詞")]),
        ("こんげつ", vec![("今月", "名詞")]),
        ("らいげつ", vec![("来月", "名詞")]),
        ("せんげつ", vec![("先月", "名詞")]),
        ("ことし", vec![("今年", "名詞")]),
        ("らいねん", vec![("来年", "名詞")]),
        ("きょねん", vec![("去年", "名詞")]),
        ("おととし", vec![("一昨年", "名詞")]),
        // IT・開発用語
        ("ぷろぐらむ", vec![("プログラム", "名詞")]),
        ("ぷろぐらみんぐ", vec![("プログラミング", "名詞")]),
        ("こーど", vec![("コード", "名詞")]),
        ("そーすこーど", vec![("ソースコード", "名詞")]),
        ("かいはつ", vec![("開発", "名詞")]),
        ("せっけい", vec![("設計", "名詞")]),
        ("じっそう", vec![("実装", "名詞")]),
        ("てすと", vec![("テスト", "名詞")]),
        ("でばっぐ", vec![("デバッグ", "名詞")]),
        ("りりーす", vec![("リリース", "名詞")]),
        ("でぷろい", vec![("デプロイ", "名詞")]),
        ("びるど", vec![("ビルド", "名詞")]),
        ("こんぱいる", vec![("コンパイル", "名詞")]),
        ("らいぶらり", vec![("ライブラリ", "名詞")]),
        ("ふれーむわーく", vec![("フレームワーク", "名詞")]),
        ("えんじにあ", vec![("エンジニア", "名詞")]),
        ("ぷろぐらまー", vec![("プログラマー", "名詞")]),
        ("でーたべーす", vec![("データベース", "名詞")]),
        ("さーばー", vec![("サーバー", "名詞")]),
        ("くらいあんと", vec![("クライアント", "名詞")]),
        ("ねっとわーく", vec![("ネットワーク", "名詞")]),
        ("せきゅりてぃ", vec![("セキュリティ", "名詞")]),
        ("ぱふぉーまんす", vec![("パフォーマンス", "名詞")]),
        ("あるごりずむ", vec![("アルゴリズム", "名詞")]),
        ("いんたーふぇーす", vec![("インターフェース", "名詞")]),
        // 動詞（終止形）
        ("する", vec![("する", "動詞"), ("為る", "動詞")]),
        ("なる", vec![("なる", "動詞"), ("成る", "動詞")]),
        ("ある", vec![("ある", "動詞"), ("有る", "動詞")]),
        ("いる", vec![("いる", "動詞"), ("居る", "動詞")]),
        ("みる", vec![("見る", "動詞")]),
        ("きく", vec![("聞く", "動詞"), ("効く", "動詞")]),
        ("いく", vec![("行く", "動詞")]),
        ("くる", vec![("来る", "動詞")]),
        ("かく", vec![("書く", "動詞"), ("描く", "動詞")]),
        ("よむ", vec![("読む", "動詞")]),
        ("はなす", vec![("話す", "動詞"), ("放す", "動詞")]),
        ("たべる", vec![("食べる", "動詞")]),
        ("のむ", vec![("飲む", "動詞")]),
        ("ねる", vec![("寝る", "動詞")]),
        ("おきる", vec![("起きる", "動詞")]),
        ("あるく", vec![("歩く", "動詞")]),
        ("はしる", vec![("走る", "動詞")]),
        ("およぐ", vec![("泳ぐ", "動詞")]),
        ("つくる", vec![("作る", "動詞"), ("創る", "動詞")]),
        ("かんがえる", vec![("考える", "動詞")]),
        ("おもう", vec![("思う", "動詞")]),
        ("しる", vec![("知る", "動詞")]),
        ("わかる", vec![("分かる", "動詞"), ("解る", "動詞")]),
        ("できる", vec![("出来る", "動詞")]),
        // 形容詞
        ("おおきい", vec![("大きい", "形容詞")]),
        ("ちいさい", vec![("小さい", "形容詞")]),
        ("ながい", vec![("長い", "形容詞")]),
        ("みじかい", vec![("短い", "形容詞")]),
        ("たかい", vec![("高い", "形容詞")]),
        ("ひくい", vec![("低い", "形容詞")]),
        ("あつい", vec![("暑い", "形容詞"), ("熱い", "形容詞"), ("厚い", "形容詞")]),
        ("さむい", vec![("寒い", "形容詞")]),
        ("あたらしい", vec![("新しい", "形容詞")]),
        ("ふるい", vec![("古い", "形容詞")]),
        ("いい", vec![("良い", "形容詞"), ("いい", "形容詞")]),
        ("わるい", vec![("悪い", "形容詞")]),
        ("むずかしい", vec![("難しい", "形容詞")]),
        ("やさしい", vec![("易しい", "形容詞"), ("優しい", "形容詞")]),
        ("たのしい", vec![("楽しい", "形容詞")]),
        ("うれしい", vec![("嬉しい", "形容詞")]),
        ("かなしい", vec![("悲しい", "形容詞")]),
        // 数字
        ("いち", vec![("一", "名詞"), ("1", "名詞")]),
        ("に", vec![("二", "名詞"), ("2", "名詞")]),
        ("さん", vec![("三", "名詞"), ("3", "名詞")]),
        ("よん", vec![("四", "名詞"), ("4", "名詞")]),
        ("し", vec![("四", "名詞")]),
        ("ご", vec![("五", "名詞"), ("5", "名詞")]),
        ("ろく", vec![("六", "名詞"), ("6", "名詞")]),
        ("なな", vec![("七", "名詞"), ("7", "名詞")]),
        ("しち", vec![("七", "名詞")]),
        ("はち", vec![("八", "名詞"), ("8", "名詞")]),
        ("きゅう", vec![("九", "名詞"), ("9", "名詞")]),
        ("く", vec![("九", "名詞")]),
        ("じゅう", vec![("十", "名詞"), ("10", "名詞")]),
        ("ひゃく", vec![("百", "名詞"), ("100", "名詞")]),
        ("せん", vec![("千", "名詞"), ("1000", "名詞")]),
        ("まん", vec![("万", "名詞")]),
        ("おく", vec![("億", "名詞")]),
        // 助詞・接続詞
        ("そして", vec![("そして", "接続詞")]),
        ("しかし", vec![("しかし", "接続詞"), ("然し", "接続詞")]),
        ("だから", vec![("だから", "接続詞")]),
        ("でも", vec![("でも", "接続詞")]),
        ("また", vec![("また", "接続詞"), ("又", "接続詞")]),
        ("または", vec![("または", "接続詞"), ("又は", "接続詞")]),
        ("および", vec![("および", "接続詞"), ("及び", "接続詞")]),
        // その他よく使う単語
        ("ひと", vec![("人", "名詞")]),
        ("もの", vec![("物", "名詞"), ("者", "名詞")]),
        ("こと", vec![("事", "名詞"), ("こと", "名詞")]),
        ("とき", vec![("時", "名詞")]),
        ("ところ", vec![("所", "名詞"), ("ところ", "名詞")]),
        ("かた", vec![("方", "名詞"), ("型", "名詞")]),
        ("かたち", vec![("形", "名詞")]),
        ("いろ", vec![("色", "名詞")]),
        ("おと", vec![("音", "名詞")]),
        ("こえ", vec![("声", "名詞")]),
        ("て", vec![("手", "名詞")]),
        ("あし", vec![("足", "名詞"), ("脚", "名詞")]),
        ("め", vec![("目", "名詞"), ("眼", "名詞")]),
        ("みみ", vec![("耳", "名詞")]),
        ("くち", vec![("口", "名詞")]),
        ("はな", vec![("花", "名詞"), ("鼻", "名詞")]),
        ("あたま", vec![("頭", "名詞")]),
        ("からだ", vec![("体", "名詞"), ("身体", "名詞")]),
        ("こころ", vec![("心", "名詞")]),
        ("きもち", vec![("気持ち", "名詞")]),
        ("かんがえ", vec![("考え", "名詞")]),
        ("きおく", vec![("記憶", "名詞")]),
        ("けいけん", vec![("経験", "名詞")]),
        ("しごと", vec![("仕事", "名詞")]),
        ("がっこう", vec![("学校", "名詞")]),
        ("かいしゃ", vec![("会社", "名詞")]),
        ("いえ", vec![("家", "名詞")]),
        ("うち", vec![("家", "名詞"), ("内", "名詞")]),
        ("へや", vec![("部屋", "名詞")]),
        ("まち", vec![("町", "名詞"), ("街", "名詞")]),
        ("くに", vec![("国", "名詞")]),
        ("せかい", vec![("世界", "名詞")]),
    ];

    for (reading, surfaces) in entries {
        let surface_list: Vec<(String, String)> = surfaces
            .into_iter()
            .map(|(s, pos)| (s.to_string(), pos.to_string()))
            .collect();
        map.insert(reading.to_string(), surface_list);
    }

    map
});

/// システム辞書
///
/// 静的辞書による辞書検索を提供します。
/// `lindera` featureが有効な場合は形態素解析も利用可能です。
pub struct SystemDictionary {
    /// lindera トークナイザー（lindera feature有効時のみ）
    #[cfg(feature = "lindera")]
    tokenizer: Option<Tokenizer>,
    /// 読み→表層形のキャッシュ
    cache: Arc<RwLock<HashMap<String, Vec<Candidate>>>>,
}

impl SystemDictionary {
    /// 新しいシステム辞書を作成
    pub fn new() -> Result<Self> {
        #[cfg(feature = "lindera")]
        let tokenizer = match Self::create_tokenizer() {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!("lindera辞書の読み込みに失敗、フォールバック辞書を使用: {}", e);
                None
            }
        };

        Ok(Self {
            #[cfg(feature = "lindera")]
            tokenizer,
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// linderaトークナイザーを作成
    #[cfg(feature = "lindera")]
    fn create_tokenizer() -> Result<Tokenizer> {
        let dictionary = load_dictionary_from_kind(DictionaryKind::IPADIC)
            .map_err(|e| NukoError::Dictionary(format!("IPADIC辞書の読み込みに失敗: {}", e)))?;

        Tokenizer::new(Mode::Normal, dictionary, None)
            .map_err(|e| NukoError::Dictionary(format!("トークナイザーの作成に失敗: {}", e)))
    }

    /// 読みから候補を検索
    pub fn lookup(&self, reading: &str) -> Result<Vec<Candidate>> {
        // キャッシュを確認
        {
            let cache = self.cache.read();
            if let Some(candidates) = cache.get(reading) {
                return Ok(candidates.clone());
            }
        }

        // 静的辞書から検索
        let mut candidates = Vec::new();

        if let Some(surfaces) = READING_TO_SURFACE.get(reading) {
            for (i, (surface, pos)) in surfaces.iter().enumerate() {
                let candidate = Candidate::new(surface, reading)
                    .with_score(100 - i as i32)
                    .with_pos(pos);
                candidates.push(candidate);
            }
        }

        // キャッシュに保存
        if !candidates.is_empty() {
            let mut cache = self.cache.write();
            cache.insert(reading.to_string(), candidates.clone());
        }

        Ok(candidates)
    }

    /// 前方一致検索
    pub fn prefix_search(&self, prefix: &str) -> Result<Vec<(String, Vec<Candidate>)>> {
        let mut results = Vec::new();

        for (reading, surfaces) in READING_TO_SURFACE.iter() {
            if reading.starts_with(prefix) {
                let candidates: Vec<Candidate> = surfaces
                    .iter()
                    .enumerate()
                    .map(|(i, (surface, pos))| {
                        Candidate::new(surface, reading)
                            .with_score(100 - i as i32)
                            .with_pos(pos)
                    })
                    .collect();
                results.push((reading.clone(), candidates));
            }
        }

        // 読みでソート
        results.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(results)
    }

    /// テキストを形態素解析
    ///
    /// linderaを使用してテキストをトークンに分割します。
    /// `lindera` featureが無効な場合はエラーを返します。
    #[cfg(feature = "lindera")]
    pub fn analyze(&self, text: &str) -> Result<Vec<Token>> {
        let tokenizer = self
            .tokenizer
            .as_ref()
            .ok_or_else(|| NukoError::Dictionary("トークナイザーが利用できません".to_string()))?;

        let mut tokens = Vec::new();

        let lindera_tokens = tokenizer
            .tokenize(text)
            .map_err(|e| NukoError::Morphological(format!("形態素解析に失敗: {}", e)))?;

        for token in lindera_tokens {
            let details: Vec<&str> = token.details().iter().map(|s| s.as_str()).collect();

            tokens.push(Token {
                surface: token.text.to_string(),
                pos: details.first().map(|s| s.to_string()),
                reading: details.get(7).map(|s| s.to_string()),
                base_form: details.get(6).map(|s| s.to_string()),
            });
        }

        Ok(tokens)
    }

    /// テキストを形態素解析（lindera無効時はエラー）
    #[cfg(not(feature = "lindera"))]
    pub fn analyze(&self, _text: &str) -> Result<Vec<Token>> {
        Err(NukoError::Dictionary(
            "形態素解析にはlindera featureが必要です".to_string(),
        ))
    }

    /// トークナイザーが利用可能かどうか
    #[must_use]
    pub fn has_tokenizer(&self) -> bool {
        #[cfg(feature = "lindera")]
        {
            self.tokenizer.is_some()
        }
        #[cfg(not(feature = "lindera"))]
        {
            false
        }
    }

    /// 辞書のエントリ数を取得
    #[must_use]
    pub fn entry_count(&self) -> usize {
        READING_TO_SURFACE.len()
    }
}

impl Default for SystemDictionary {
    fn default() -> Self {
        Self::new().expect("Failed to create system dictionary")
    }
}

/// 形態素解析結果のトークン
#[derive(Debug, Clone)]
pub struct Token {
    /// 表層形
    pub surface: String,
    /// 品詞
    pub pos: Option<String>,
    /// 読み
    pub reading: Option<String>,
    /// 基本形
    pub base_form: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup() {
        let dict = SystemDictionary::new().unwrap();
        let candidates = dict.lookup("にほん").unwrap();

        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c.surface == "日本"));
    }

    #[test]
    fn test_lookup_not_found() {
        let dict = SystemDictionary::new().unwrap();
        let candidates = dict.lookup("xxxyyy").unwrap();

        assert!(candidates.is_empty());
    }

    #[test]
    fn test_prefix_search() {
        let dict = SystemDictionary::new().unwrap();
        let results = dict.prefix_search("にほん").unwrap();

        assert!(!results.is_empty());
        // "にほん", "にほんご", "にほんじん" などがマッチするはず
        assert!(results.iter().any(|(r, _)| r == "にほん"));
    }

    #[test]
    fn test_entry_count() {
        let dict = SystemDictionary::new().unwrap();
        assert!(dict.entry_count() > 100);
    }
}
