use nuko_core::conversion::{CandidateList, ConversionContext};
use nuko_core::prelude::*;
use std::cell::RefCell;

// セッション共有の `ConversionEngine` を thread-local で保持する。
//
// なぜ thread_local か:
// `akaza` feature 有効時、`LibakazaBackend` 内部の `Rc<...>` 由来で
// `ConversionEngine` が `!Send` になる。`LazyLock<Mutex<>>` は
// `T: Send` を要求するため使えない。詳細は
// `docs/spikes/libakaza-send-constraint.md` (spike-3) を参照。
//
// macOS IMK callback はメインスレッド (NSApplication run loop) で
// dispatch されるため、thread_local でも単一インスタンスで稼働する。
// `akaza` feature 無効時も同じパターンで動作させ、コードパスを統一する。
//
// 直接 `ENGINE` を触らず、`with_engine` / `with_engine_mut` を経由すること。
thread_local! {
    static ENGINE: RefCell<ConversionEngine> =
        RefCell::new(build_engine().expect("ConversionEngine の初期化に失敗"));
}

/// `ConversionEngine` への immutable アクセスを提供する。
///
/// クロージャ内でのみ engine を借用できる。返り値はクロージャの出力。
pub fn with_engine<F, R>(f: F) -> R
where
    F: FnOnce(&ConversionEngine) -> R,
{
    ENGINE.with(|cell| f(&cell.borrow()))
}

/// `ConversionEngine` への mutable アクセスを提供する。
///
/// クロージャ内でのみ engine を可変借用できる。学習 (`commit`) などで使用。
pub fn with_engine_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut ConversionEngine) -> R,
{
    ENGINE.with(|cell| f(&mut cell.borrow_mut()))
}

#[cfg(feature = "akaza")]
fn build_engine() -> nuko_core::error::Result<ConversionEngine> {
    let model_dir = libakaza_model_dir();
    tracing::info!(
        model_dir = %model_dir.display(),
        "libakaza モデルディレクトリを試行 (akaza feature 有効)"
    );
    ConversionEngine::with_libakaza(model_dir)
}

#[cfg(not(feature = "akaza"))]
fn build_engine() -> nuko_core::error::Result<ConversionEngine> {
    ConversionEngine::new()
}

/// macOS 上の libakaza モデルディレクトリのデフォルトパス。
///
/// 優先順位:
/// 1. 環境変数 `NUKO_AKAZA_MODEL_DIR` (開発/テスト用)
/// 2. `$HOME/Library/Application Support/nuko-ime/akaza-model/`
///
/// モデル未配置時は `with_libakaza` が内部で警告ログを出して
/// 静的辞書フォールバックする (`ConversionEngine` の契約)。
#[cfg(feature = "akaza")]
fn libakaza_model_dir() -> std::path::PathBuf {
    if let Ok(override_path) = std::env::var("NUKO_AKAZA_MODEL_DIR") {
        return std::path::PathBuf::from(override_path);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("nuko-ime")
        .join("akaza-model")
}

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
