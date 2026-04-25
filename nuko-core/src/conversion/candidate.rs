//! 変換候補の定義

use serde::{Deserialize, Serialize};

/// 変換候補
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// 表層形（変換結果）
    pub surface: String,
    /// 読み
    pub reading: String,
    /// 品詞情報
    pub pos: Option<String>,
    /// スコア（高いほど優先）
    pub score: i32,
    /// 候補のソース
    pub source: CandidateSource,
}

impl Candidate {
    /// 新しい候補を作成
    #[must_use]
    pub fn new(surface: impl Into<String>, reading: impl Into<String>) -> Self {
        Self {
            surface: surface.into(),
            reading: reading.into(),
            pos: None,
            score: 0,
            source: CandidateSource::System,
        }
    }

    /// スコアを設定
    #[must_use]
    pub fn with_score(mut self, score: i32) -> Self {
        self.score = score;
        self
    }

    /// 品詞を設定
    #[must_use]
    pub fn with_pos(mut self, pos: impl Into<String>) -> Self {
        self.pos = Some(pos.into());
        self
    }

    /// ソースを設定
    #[must_use]
    pub fn with_source(mut self, source: CandidateSource) -> Self {
        self.source = source;
        self
    }
}

/// 候補のソース
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateSource {
    /// システム辞書
    System,
    /// ユーザー辞書
    User,
    /// 学習データ
    Learned,
}

/// 変換候補リスト
#[derive(Debug, Clone, Default)]
pub struct CandidateList {
    /// 候補のリスト
    candidates: Vec<Candidate>,
    /// 現在選択中のインデックス
    selected: usize,
}

impl CandidateList {
    /// 新しい候補リストを作成
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 候補を追加
    pub fn push(&mut self, candidate: Candidate) {
        self.candidates.push(candidate);
    }

    /// 候補をスコア順にソート
    pub fn sort_by_score(&mut self) {
        self.candidates
            .sort_by_key(|c| std::cmp::Reverse(c.score));
    }

    /// 候補数を取得
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// 空かどうか
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// 現在選択中の候補を取得
    #[must_use]
    pub fn selected(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    /// 次の候補を選択
    pub fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = (self.selected + 1) % self.candidates.len();
        }
    }

    /// 前の候補を選択
    pub fn select_prev(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = if self.selected == 0 {
                self.candidates.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// 指定インデックスの候補を選択
    pub fn select(&mut self, index: usize) {
        if index < self.candidates.len() {
            self.selected = index;
        }
    }

    /// すべての候補を取得
    #[must_use]
    pub fn all(&self) -> &[Candidate] {
        &self.candidates
    }

    /// イテレータを取得
    pub fn iter(&self) -> impl Iterator<Item = &Candidate> {
        self.candidates.iter()
    }
}

impl IntoIterator for CandidateList {
    type Item = Candidate;
    type IntoIter = std::vec::IntoIter<Candidate>;

    fn into_iter(self) -> Self::IntoIter {
        self.candidates.into_iter()
    }
}
