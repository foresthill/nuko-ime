//! システム辞書

use crate::conversion::Candidate;
use crate::error::{NukoError, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// システム辞書
///
/// 形態素解析器（lindera）をラップし、基本的な辞書検索を提供します。
pub struct SystemDictionary {
    /// 簡易辞書（デモ用）
    /// 本番ではlinderaを使用
    entries: Arc<RwLock<HashMap<String, Vec<Candidate>>>>,
}

impl SystemDictionary {
    /// 新しいシステム辞書を作成
    pub fn new() -> Result<Self> {
        let mut entries = HashMap::new();

        // デモ用の基本エントリ
        Self::add_entry(&mut entries, "にほん", vec!["日本", "二本"]);
        Self::add_entry(&mut entries, "にほんご", vec!["日本語"]);
        Self::add_entry(&mut entries, "ぬこ", vec!["ぬこ", "猫"]);
        Self::add_entry(&mut entries, "いめ", vec!["IME", "イメ"]);
        Self::add_entry(&mut entries, "へんかん", vec!["変換", "返還", "編纂"]);
        Self::add_entry(&mut entries, "にゅうりょく", vec!["入力"]);
        Self::add_entry(&mut entries, "かんじ", vec!["漢字", "幹事", "感じ"]);
        Self::add_entry(&mut entries, "ひらがな", vec!["平仮名", "ひらがな"]);
        Self::add_entry(&mut entries, "かたかな", vec!["片仮名", "カタカナ"]);
        Self::add_entry(&mut entries, "とうきょう", vec!["東京"]);
        Self::add_entry(&mut entries, "おおさか", vec!["大阪"]);
        Self::add_entry(&mut entries, "きょうと", vec!["京都"]);
        Self::add_entry(&mut entries, "なごや", vec!["名古屋"]);
        Self::add_entry(&mut entries, "ふくおか", vec!["福岡"]);
        Self::add_entry(&mut entries, "ほっかいどう", vec!["北海道"]);
        Self::add_entry(&mut entries, "こんにちは", vec!["こんにちは", "今日は"]);
        Self::add_entry(&mut entries, "ありがとう", vec!["ありがとう", "有難う"]);
        Self::add_entry(&mut entries, "すみません", vec!["すみません", "済みません"]);
        Self::add_entry(&mut entries, "おはよう", vec!["おはよう", "お早う"]);
        Self::add_entry(&mut entries, "こんばんは", vec!["こんばんは", "今晩は"]);
        Self::add_entry(&mut entries, "さようなら", vec!["さようなら", "左様なら"]);
        Self::add_entry(&mut entries, "わたし", vec!["私", "わたし"]);
        Self::add_entry(&mut entries, "あなた", vec!["あなた", "貴方", "貴女"]);
        Self::add_entry(&mut entries, "かれ", vec!["彼"]);
        Self::add_entry(&mut entries, "かのじょ", vec!["彼女"]);
        Self::add_entry(&mut entries, "これ", vec!["これ", "此れ"]);
        Self::add_entry(&mut entries, "それ", vec!["それ", "其れ"]);
        Self::add_entry(&mut entries, "あれ", vec!["あれ", "彼れ"]);
        Self::add_entry(&mut entries, "いま", vec!["今", "居間"]);
        Self::add_entry(&mut entries, "きょう", vec!["今日", "京", "興"]);
        Self::add_entry(&mut entries, "あした", vec!["明日"]);
        Self::add_entry(&mut entries, "きのう", vec!["昨日", "機能"]);
        Self::add_entry(&mut entries, "らいしゅう", vec!["来週"]);
        Self::add_entry(&mut entries, "せんしゅう", vec!["先週"]);
        Self::add_entry(&mut entries, "らいげつ", vec!["来月"]);
        Self::add_entry(&mut entries, "せんげつ", vec!["先月"]);
        Self::add_entry(&mut entries, "らいねん", vec!["来年"]);
        Self::add_entry(&mut entries, "きょねん", vec!["去年"]);
        Self::add_entry(&mut entries, "ことし", vec!["今年"]);
        Self::add_entry(&mut entries, "ぷろぐらみんぐ", vec!["プログラミング"]);
        Self::add_entry(&mut entries, "かいはつ", vec!["開発"]);
        Self::add_entry(&mut entries, "せっけい", vec!["設計"]);
        Self::add_entry(&mut entries, "じっそう", vec!["実装"]);
        Self::add_entry(&mut entries, "てすと", vec!["テスト"]);
        Self::add_entry(&mut entries, "でばっぐ", vec!["デバッグ"]);
        Self::add_entry(&mut entries, "りりーす", vec!["リリース"]);
        Self::add_entry(&mut entries, "ぱふぉーまんす", vec!["パフォーマンス"]);

        Ok(Self {
            entries: Arc::new(RwLock::new(entries)),
        })
    }

    /// エントリを追加（内部ヘルパー）
    fn add_entry(map: &mut HashMap<String, Vec<Candidate>>, reading: &str, surfaces: Vec<&str>) {
        let candidates: Vec<Candidate> = surfaces
            .into_iter()
            .enumerate()
            .map(|(i, s)| Candidate::new(s, reading).with_score(100 - i as i32))
            .collect();
        map.insert(reading.to_string(), candidates);
    }

    /// 読みから候補を検索
    pub fn lookup(&self, reading: &str) -> Result<Vec<Candidate>> {
        let entries = self.entries.read();

        if let Some(candidates) = entries.get(reading) {
            Ok(candidates.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 前方一致検索
    pub fn prefix_search(&self, prefix: &str) -> Result<Vec<(String, Vec<Candidate>)>> {
        let entries = self.entries.read();
        let results: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Ok(results)
    }

    // TODO: linderaを使用した本格的な形態素解析の実装
    // pub fn analyze(&self, text: &str) -> Result<Vec<Token>> { ... }
}

impl Default for SystemDictionary {
    fn default() -> Self {
        Self::new().expect("Failed to create system dictionary")
    }
}
