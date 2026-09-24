use nuko_core::conversion::{CandidateList, ConversionContext, SegmentedConversion};
use nuko_core::learning::{extract_corrections, CorrectionStore, ObservationLog};
use nuko_core::prelude::*;
use objc2::MainThreadMarker;
use std::cell::RefCell;
use std::time::Instant;

use crate::candidate_panel::CustomCandidatePanel;
use crate::learning_panel::LearningStatusPanel;

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

// 学習状況パネル (NSPanel ベース) も **アプリ全体で 1 つ** だけ保持する。
// メニュー「学習状況を見る…」「学習を今すぐ研ぎ直す」から表示する。
thread_local! {
    static LEARNING_PANEL: RefCell<Option<LearningStatusPanel>> = const { RefCell::new(None) };
}

/// 学習状況パネルを **必要に応じて** 生成する (まだ未生成なら 1 度だけ)。
pub fn ensure_learning_panel(mtm: MainThreadMarker) {
    LEARNING_PANEL.with(|cell| {
        if cell.borrow().is_some() {
            return;
        }
        *cell.borrow_mut() = Some(LearningStatusPanel::new(mtm));
        tracing::info!("LearningStatusPanel created (singleton)");
    });
}

/// 学習状況パネルへのアクセサ (未生成時は `None`)。
pub fn with_learning_panel<F, R>(f: F) -> R
where
    F: FnOnce(Option<&LearningStatusPanel>) -> R,
{
    LEARNING_PANEL.with(|cell| f(cell.borrow().as_ref()))
}

/// 学習状況を人間可読なテキストにまとめる (パネル表示用)。
///
/// CLI `nuko learn show` と同じ情報 (オプトイン状態・観察件数・訂正選好) を、
/// GUI パネル向けにプレーンテキストで返す。
pub fn learning_status_text() -> String {
    let Some(dir) = nuko_app_support_dir() else {
        return "🐈 学習データの場所を取得できませんでした".to_string();
    };
    let enabled = dir.join("OBSERVE_ENABLED").exists();
    let count = ObservationLog::new(true, dir.join("observations.jsonl"))
        .count()
        .unwrap_or(0);
    let store = CorrectionStore::load(dir.join("corrections.toml")).unwrap_or_default();

    let mut s = String::from("🐈 ぬこIME 学習状況\n");
    let obs_state = if enabled {
        "ON"
    } else {
        "OFF（収集なし）"
    };
    s.push_str(&format!("観察ログ: {obs_state}／観察 {count} 件\n"));
    if store.is_empty() {
        s.push_str("学習した変換選好: まだありません\n");
    } else {
        s.push_str(&format!("学習した変換選好（{} 件）:\n", store.len()));
        for p in &store.preferences {
            s.push_str(&format!("　{} → {}（{}回）\n", p.reading, p.prefer, p.seen));
        }
    }
    // ── ここに乗るロジックのヘルプ (ユーザー要望) ──
    s.push_str("──────────\n");
    s.push_str(&format!(
        "💡 同じ変換を{MIN_SEEN}回以上、または既定でない候補を選ぶと学習され、\n"
    ));
    s.push_str("　次からその変換が上位に来ます（読み＝そのままの確定は対象外）。");
    s
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

// --- Layer 1: 観察ログ (オプトイン・ローカル) ---------------------------
//
// docs/LEARNING_ARCHITECTURE.md の Layer 1。確定/訂正イベントをローカルの
// observations.jsonl に追記する。**プライバシー既定は「何も記録しない」**。
//
// オプトイン方式 (MVP・依存ゼロ): app support dir に marker ファイル
// `OBSERVE_ENABLED` が存在すれば有効。無ければ無効 (既定)。
//   有効化: touch "$HOME/Library/Application Support/nuko-ime/OBSERVE_ENABLED"
//   無効化: 上記ファイルを削除
// (トグル UI は将来。docs/LEARNING_ARCHITECTURE.md §7 参照)
thread_local! {
    static OBSERVATION_LOG: ObservationLog = build_observation_log();
}

/// `~/Library/Application Support/nuko-ime/` を返す (HOME 取得失敗時 None)。
fn nuko_app_support_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("nuko-ime"),
    )
}

fn build_observation_log() -> ObservationLog {
    let Some(dir) = nuko_app_support_dir() else {
        return ObservationLog::disabled();
    };
    let enabled = dir.join("OBSERVE_ENABLED").exists();
    tracing::info!(enabled, "観察ログ (Layer 1) オプトイン状態");
    ObservationLog::new(enabled, dir.join("observations.jsonl"))
}

/// 観察ログ (Layer 1) へのアクセスを提供する。
///
/// `record` はオプトイン無効時 (既定) は完全な no-op(ファイルも作らない)。
pub fn with_observation<F, R>(f: F) -> R
where
    F: FnOnce(&ObservationLog) -> R,
{
    OBSERVATION_LOG.with(f)
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
    setup_corrections(&mut engine);
    Ok(engine)
}

#[cfg(not(feature = "akaza"))]
fn build_engine() -> nuko_core::error::Result<ConversionEngine> {
    let mut engine = ConversionEngine::new()?;
    setup_learning_persistence(&mut engine);
    setup_corrections(&mut engine);
    Ok(engine)
}

/// この回数以上コミットされた (reading→surface) だけ選好化する (tunable)。
/// 起動時の [`setup_corrections`] とメニューからの [`relearn_now`] で共有する。
const MIN_SEEN: u32 = 2;

/// Layer 2: 訂正選好 (corrections.toml) を engine に設定する。
///
/// `RELEARN` marker があれば observations.jsonl から corrections.toml を **決定論的に
/// 再生成** (冪等) してから marker を消す。その後 corrections.toml を load。
/// marker が無ければ既存の corrections.toml をそのまま使う (手編集を尊重)。
///
///   再学習: touch "$HOME/Library/Application Support/nuko-ime/RELEARN" → NukoIME 再起動
fn setup_corrections(engine: &mut ConversionEngine) {
    let Some(dir) = nuko_app_support_dir() else {
        return;
    };
    let corrections_path = dir.join("corrections.toml");
    let relearn_marker = dir.join("RELEARN");

    if relearn_marker.exists() {
        let log = ObservationLog::new(true, dir.join("observations.jsonl"));
        match log.read_all() {
            Ok(events) => {
                let store = extract_corrections(&events, MIN_SEEN);
                match store.save(&corrections_path) {
                    Ok(()) => tracing::info!(
                        count = store.len(),
                        "RELEARN: 観察ログから訂正選好 (Layer 2) を再生成"
                    ),
                    Err(e) => tracing::warn!(error = %e, "corrections.toml 保存失敗"),
                }
            }
            Err(e) => tracing::warn!(error = %e, "観察ログ読み込み失敗 (RELEARN)"),
        }
        let _ = std::fs::remove_file(&relearn_marker);
    }

    if let Err(e) = engine.load_corrections(&corrections_path) {
        tracing::warn!(error = %e, "corrections.toml load 失敗 (選好なしで継続)");
    }
}

/// メニュー「学習を今すぐ研ぎ直す」から呼ぶライブ再学習。
///
/// 観察ログ (Layer 1) → 訂正選好 (Layer 2) を **決定論的に再生成**・保存し、
/// 稼働中の [`ConversionEngine`] へ即時反映する (再起動不要のホットリロード)。
/// 反映した選好件数を返す。観察ログが無い / 空なら 0 件。
///
/// CLI の `nuko learn relearn` と同じ抽出ロジック ([`extract_corrections`]) を
/// 使うので、両者は同じ結果を返す (churn-free)。
pub fn relearn_now() -> nuko_core::error::Result<usize> {
    let Some(dir) = nuko_app_support_dir() else {
        return Ok(0);
    };
    let corrections_path = dir.join("corrections.toml");
    let log = ObservationLog::new(true, dir.join("observations.jsonl"));
    let events = log.read_all()?;
    let store = extract_corrections(&events, MIN_SEEN);
    let count = store.len();
    store.save(&corrections_path)?;
    with_engine_mut(|engine| engine.load_corrections(&corrections_path))?;
    Ok(count)
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
    ///
    /// libakaza が動かない場合 (= フォールバック) や、文節別変換が無効な場合に使用。
    pub candidates: Option<CandidateList>,
    /// 文節別変換結果 (Phase 1.3 Step 3, libakaza が利用可能なときのみ)
    ///
    /// 複数文節入力で焦点文節を切り替えながら個別に候補を選べるようにする。
    /// `candidates` と排他的に使う想定 (= どちらか一方が `Some`)。
    pub segmented: Option<SegmentedConversion>,
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
    /// 直近の「かな」キー押下時刻 (handleEvent: で keyCode 104 を検知して記録)
    ///
    /// macOS Japanese keyboard の「かな」キーを押した直後、なぜか Space イベント
    /// が inputText: に漏れて入ることが確認された (2026-06-09 ユーザー報告)。
    /// 「かな」キー押下から短時間以内の Space は「漏れ」と判定して破棄するため記録。
    pub kana_pressed_at: Option<Instant>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            romaji: RomajiConverter::new(),
            composition: String::new(),
            candidates: None,
            segmented: None,
            context: ConversionContext::new(),
            is_composing: false,
            japanese_mode: true, // デフォルトは日本語入力モード
            activated_at: None,
            kana_pressed_at: None,
        }
    }

    /// 状態をリセット（確定・取消後）
    pub fn reset(&mut self) {
        self.romaji.clear();
        self.composition.clear();
        self.candidates = None;
        self.segmented = None;
        self.is_composing = false;
    }

    /// 変換結果が存在するか (`candidates` か `segmented` のどちらかが Some)
    #[allow(dead_code)] // 将来用 (今は直接 state.candidates / state.segmented を見る)
    #[must_use]
    pub fn has_conversion(&self) -> bool {
        self.candidates.is_some() || self.segmented.is_some()
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
