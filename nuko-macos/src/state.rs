use nuko_core::conversion::{CandidateList, ConversionContext};
use nuko_core::prelude::*;
use objc2::MainThreadMarker;
use std::cell::RefCell;
use std::time::Instant;

use crate::candidate_panel::CustomCandidatePanel;

// 自前候補ウィンドウ (NSPanel ベース) を **アプリ全体で 1 つ** だけ保持する。
//
// 経緯: PR #29 / #31 / #32 で IMKCandidates を試したが、Apple 公式 IMK は
// 「ancient rubbish」(Shiki Suen) と評される程度の framework バグを抱えており、
// panel が出ても event routing / 青ハイライト同期に難ありで実用に至らず。
// C 案 = vChewing スタイルの自前 NSPanel + NSTextField 描画に移行した
// (PR #33 = `feat/custom-candidate-panel`)。
//
// `CustomCandidatePanel` は `NSPanel` / `NSTextField` (= `NSResponder` 系)
// を内部に持つため `MainThreadMarker` が必要。`ensure_custom_panel` の
// シグネチャで強制する。
thread_local! {
    static CUSTOM_PANEL: RefCell<Option<CustomCandidatePanel>> =
        const { RefCell::new(None) };
}

/// 自前候補ウィンドウを **必要に応じて** 生成する (まだ未生成なら 1 度だけ)
pub fn ensure_custom_panel(mtm: MainThreadMarker) {
    CUSTOM_PANEL.with(|cell| {
        if cell.borrow().is_some() {
            return;
        }
        let panel = CustomCandidatePanel::new(mtm);
        tracing::info!("CustomCandidatePanel created (singleton)");
        *cell.borrow_mut() = Some(panel);
    });
}

/// 自前候補ウィンドウへのアクセサ。`f` には panel への参照が渡される (未生成時は `None`)
pub fn with_custom_panel<F, R>(f: F) -> R
where
    F: FnOnce(Option<&CustomCandidatePanel>) -> R,
{
    CUSTOM_PANEL.with(|cell| {
        let borrow = cell.borrow();
        f(borrow.as_ref())
    })
}

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
    let mut engine = ConversionEngine::with_libakaza(model_dir)?;
    setup_learning_persistence(&mut engine);
    Ok(engine)
}

#[cfg(not(feature = "akaza"))]
fn build_engine() -> nuko_core::error::Result<ConversionEngine> {
    let mut engine = ConversionEngine::new()?;
    setup_learning_persistence(&mut engine);
    Ok(engine)
}

/// 学習データの永続化パスを設定する
///
/// 保存先: `~/Library/Application Support/nuko-ime/learning.json`
/// 失敗 (= ホームディレクトリ取得 / load 失敗) は warn のみで起動を続ける。
fn setup_learning_persistence(engine: &mut ConversionEngine) {
    let Ok(home) = std::env::var("HOME") else {
        tracing::warn!("HOME 環境変数が取得できないため学習永続化を無効化");
        return;
    };
    let path = std::path::PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("nuko-ime")
        .join("learning.json");
    if let Err(e) = engine.set_learning_path(&path) {
        tracing::warn!(
            path = %path.display(),
            error = %e,
            "学習データ load 失敗。in-memory のみで継続"
        );
    }
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
    /// 直近の activateServer 呼び出し時刻
    ///
    /// macOS のソース切替ショートカット (Ctrl+Space 等) で NukoIME が
    /// 活性化された直後、押下中の Space キーが本 IME に漏れて
    /// inputText: Some(" ") として届くことがある (実観測 2026-06-04)。
    /// 活性化から短時間以内の Space は「ショートカットの漏れ」と判定して
    /// 破棄する目的で記録する。
    pub activated_at: Option<Instant>,
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
            activated_at: None,
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
