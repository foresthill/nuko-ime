use nuko_core::conversion::{CandidateList, ConversionContext};
use nuko_core::prelude::*;
use parking_lot::Mutex;
use std::sync::LazyLock;

/// 共有 ConversionEngine（全入力セッションで共有するシングルトン）
pub static ENGINE: LazyLock<Mutex<ConversionEngine>> = LazyLock::new(|| {
    Mutex::new(ConversionEngine::new().expect("ConversionEngine の初期化に失敗"))
});

/// セッションごとの入力状態（IMKInputController インスタンスごとに1つ）
pub struct InputState {
    /// ローマ字→かな変換器
    pub romaji: RomajiConverter,
    /// 現在のかな組み立て文字列
    pub composition: String,
    /// 変換候補（None = 変換モードではない）
    pub candidates: Option<CandidateList>,
    /// 変換コンテキスト（学習・文脈用）
    pub context: ConversionContext,
    /// 未確定文字列を表示中かどうか
    pub is_composing: bool,
    /// 日本語入力モード（false = 英数直接入力モード）
    pub japanese_mode: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            romaji: RomajiConverter::new(),
            composition: String::new(),
            candidates: None,
            context: ConversionContext::new(),
            is_composing: false,
            japanese_mode: true, // デフォルトは日本語入力モード
        }
    }

    /// 状態をリセット（確定・取消後）
    pub fn reset(&mut self) {
        self.romaji.clear();
        self.composition.clear();
        self.candidates = None;
        self.is_composing = false;
    }

    /// 表示用テキストを取得（かな組み立て + ローマ字バッファ）
    ///
    /// バッファ "n" の描画ルール:
    /// - composition が既に「ん」で終わっている場合 → バッファを描画しない
    ///   (nn ルールで既にん出力済み。kanna 入力中の "kann" 時点で "かんん" と見せない)
    /// - それ以外 → "n" を "ん" として描画 (単独の "hen" 等で「へん」と見せる)
    ///
    /// 内部バッファは "n" のまま保持されるため、続けて "a" 等が来れば "な" に正しく繋がる。
    pub fn display_text(&self) -> String {
        let mut text = self.composition.clone();
        let buf = self.romaji.buffer();
        if buf == "n" {
            if !text.ends_with('ん') {
                text.push('ん');
            }
        } else {
            text.push_str(buf);
        }
        text
    }
}
