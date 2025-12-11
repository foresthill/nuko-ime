//! 変換コンテキスト

/// 変換時のコンテキスト情報
#[derive(Debug, Clone, Default)]
pub struct ConversionContext {
    /// 直前に確定した単語
    pub prev_words: Vec<String>,
    /// 変換モード
    pub mode: ConversionMode,
    /// 最大候補数
    pub max_candidates: usize,
}

impl ConversionContext {
    /// 新しいコンテキストを作成
    #[must_use]
    pub fn new() -> Self {
        Self {
            prev_words: Vec::new(),
            mode: ConversionMode::Normal,
            max_candidates: 9,
        }
    }

    /// 直前の単語を追加
    pub fn push_prev_word(&mut self, word: impl Into<String>) {
        self.prev_words.push(word.into());
        // 最大5単語まで保持
        if self.prev_words.len() > 5 {
            self.prev_words.remove(0);
        }
    }

    /// コンテキストをクリア
    pub fn clear(&mut self) {
        self.prev_words.clear();
    }
}

/// 変換モード
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConversionMode {
    /// 通常変換
    #[default]
    Normal,
    /// カタカナ変換
    Katakana,
    /// 半角カタカナ変換
    HalfwidthKatakana,
    /// 全角英数変換
    FullwidthAlphanumeric,
    /// 半角英数変換
    HalfwidthAlphanumeric,
}
