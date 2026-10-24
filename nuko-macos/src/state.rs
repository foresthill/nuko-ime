use nuko_core::conversion::{CandidateList, ConversionContext, SegmentedConversion};
use nuko_core::learning::{extract_corrections, CorrectionStore, ObservationLog};
use nuko_core::prelude::*;
use objc2::MainThreadMarker;
use std::cell::RefCell;
use std::time::Instant;

use crate::candidate_panel::CustomCandidatePanel;
use crate::learning_panel::LearningStatusPanel;
use crate::settings_window::SettingsWindow;

// 「かな」キー押下時刻を **全 controller インスタンス共有** で保持する。
//
// 落とし穴 #4: IMKInputController はタブ/アプリ切替で複数インスタンス生成される。
// setValue:forTag:(かな検知) を受ける controller と、直後の漏れ Space の inputText:
// を受ける controller が **別インスタンス** になり得るため、controller ごとの ivar
// (旧 kana_pressed_at) では「かなガード」が発火しなかった (2026-09 実機ログで確認)。
// engine/パネルと同様に thread_local (= プロセス内共有) にして横断的に効かせる。
thread_local! {
    static LAST_KANA_PRESS: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// 「かな」モードへの切替を検知したら呼ぶ (setValue:forTag: 経由)。
pub fn note_kana_press() {
    LAST_KANA_PRESS.with(|c| c.set(Some(Instant::now())));
}

/// 直近の「かな」押下からの経過ミリ秒 (かなガード判定用)。未記録なら `None`。
pub fn kana_press_elapsed_ms() -> Option<u128> {
    LAST_KANA_PRESS.with(|c| c.get().map(|t| t.elapsed().as_millis()))
}

/// かなガード発火時に 1 shot で消費する。
pub fn clear_kana_press() {
    LAST_KANA_PRESS.with(|c| c.set(None));
}

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
/// メニュー「AI ログを見る」用: BYOK 呼び出しの累計コスト＋直近の記録をテキストで返す。
pub fn ai_log_text() -> String {
    let all = nuko_core::ai_log::read_all();
    if all.is_empty() {
        return "📊 AI ログ\n\nまだありません。\n「🤖 AIで学習を研ぎ直す（BYOK）」を実行すると、\n送信内容・応答・トークン・コストが記録されます。\n（全文は ~/Library/Application Support/nuko-ime/ai_log.jsonl / `nuko ai-log --full`）".to_string();
    }
    let total_cost: f64 = all.iter().map(|e| e.cost_usd).sum();
    let total_in: u64 = all.iter().map(|e| e.input_tokens).sum();
    let total_out: u64 = all.iter().map(|e| e.output_tokens).sum();
    let mut s = String::from("📊 AI ログ（BYOK 呼び出し）\n");
    s.push_str(&format!(
        "累計 {} 回 ／ 入力 {total_in} tok ／ 出力 {total_out} tok ／ 概算 ${total_cost:.4}\n",
        all.len()
    ));
    s.push_str("──────────\n");
    for e in all.iter().rev().take(20) {
        let model = e.resolved_model.as_deref().unwrap_or(e.model.as_str());
        s.push_str(&format!(
            "{} [{}] {}\n　in {} / out {} tok ／ ${:.4} ／ {}ms\n",
            e.time, e.provider, model, e.input_tokens, e.output_tokens, e.cost_usd, e.ms
        ));
    }
    s.push_str("──────────\n全文は `nuko ai-log --full` / ai_log.jsonl を参照。");
    s
}

/// メニューのヘッダに出す 1 行サマリ (例: "ぬこIME ・ 学習 42 選好 ・ 観察 1200 件（ON）")。
///
/// `learning_status_text` と同じデータ源を参照するが、メニュー項目タイトル用に 1 行へ圧縮する。
pub fn menu_status_line() -> String {
    let Some(dir) = nuko_app_support_dir() else {
        return "ぬこIME".to_string();
    };
    let enabled = dir.join("OBSERVE_ENABLED").exists();
    let count = ObservationLog::new(true, dir.join("observations.jsonl"))
        .count()
        .unwrap_or(0);
    let prefs = CorrectionStore::load(dir.join("corrections.toml"))
        .map(|s| s.len())
        .unwrap_or(0);
    let obs = if enabled { "ON" } else { "OFF" };
    format!("ぬこIME ・ 学習 {prefs} 選好 ・ 観察 {count} 件（{obs}）")
}

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
        // パネルはスクロール可能なので全件表示する (よく使う順 = seen 降順)。
        s.push_str(&format!(
            "学習した変換選好（{} 件・よく使う順）:\n",
            store.len()
        ));
        let mut prefs: Vec<_> = store.preferences.iter().collect();
        prefs.sort_by(|a, b| b.seen.cmp(&a.seen).then_with(|| a.reading.cmp(&b.reading)));
        for p in &prefs {
            s.push_str(&format!("　{} → {}（{}回）\n", p.reading, p.prefer, p.seen));
        }
    }

    // ── 文節の切り方の学習 (2026-10 追加) ──
    let seg_store = nuko_core::learning::SegmentationStore::load(dir.join("segmentations.toml"))
        .unwrap_or_default();
    s.push_str("──────────\n");
    if seg_store.is_empty() {
        s.push_str("文節の切り方の学習: まだありません\n");
        s.push_str("　（変換中に Shift+←→ で文節を切り直して確定すると覚えます）\n");
    } else {
        s.push_str(&format!("文節の切り方の学習（{} 件）:\n", seg_store.len()));
        let mut entries: Vec<_> = seg_store.entries.iter().collect();
        entries.sort_by(|a, b| a.reading.cmp(&b.reading));
        for e in &entries {
            let boxed: String = e.segments.iter().map(|s| format!("[{s}]")).collect();
            s.push_str(&format!("　{} → {}\n", e.reading, boxed));
        }
    }

    // ── ここに乗るロジックのヘルプ (ユーザー要望) ──
    s.push_str("──────────\n");
    s.push_str(&format!(
        "💡 同じ変換を{MIN_SEEN}回以上、または既定でない候補を選ぶと学習され、\n"
    ));
    s.push_str("　次からその変換が上位に来ます。文節は Shift+←→ で直すと切り方を覚えます。");
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

// 直近の確定を **最大 2 件** in-memory で保持する (新しいものが末尾)。
// 完全に揮発性 (プロセス内のみ・永続化しない)。
// - ② 打ち直し検知: 直前 (末尾) と同じ読み・別表層なら訂正。
// - 単語登録: 「1つ前の読み → 直前の語」を登録候補にする (下記参照)。
thread_local! {
    static RECENT_COMMITS: RefCell<Vec<(String, String, Instant)>> = const { RefCell::new(Vec::new()) };
}

/// 直前の確定を記録しつつ、**今回が「打ち直し (訂正)」なら直前の表層を返す**。
///
/// 訂正の条件: **同じ読み** を **別の表層** で **`window` 以内** に確定し直した。
/// 返り値 `Some(prev_surface)` のとき、呼び出し側は
/// `Correction(reading, prev_surface, surface)` を観察ログに記録する。
///
/// バックスペース検知に依存せず、確定の連続だけで訂正を捉える (堅牢)。
pub fn note_commit_detect_correction(
    reading: &str,
    surface: &str,
    window: std::time::Duration,
) -> Option<String> {
    RECENT_COMMITS.with(|cell| {
        let mut v = cell.borrow_mut();
        let correction = match v.last() {
            Some((r, s, t)) if r == reading && s != surface && t.elapsed() <= window => {
                Some(s.clone())
            }
            _ => None,
        };
        v.push((reading.to_string(), surface.to_string(), Instant::now()));
        // 直近 2 件だけ保持
        if v.len() > 2 {
            let start = v.len() - 2;
            v.drain(0..start);
        }
        correction
    })
}

/// 単語登録の候補 `(読み, 表層)` を返す。
///
/// 直近 2 件の確定 `[1つ前, 直前]` から **「1つ前の読み → 直前の表層」** を提案する。
/// 自然な流れ「こまや→駒屋(違う) → こまたに→駒谷(正解)」の直後に
/// 「こまや→駒谷」を登録できる。**未確定の composition に依存しない**ので、
/// 入力メニューを開いた瞬間に composition が確定されても壊れない (旧方式の不具合の修正)。
///
/// 2 件揃っていて、読みが異なり、表層も異なるときだけ `Some`。
pub fn registration_candidate() -> Option<(String, String)> {
    RECENT_COMMITS.with(|cell| {
        let v = cell.borrow();
        if v.len() < 2 {
            return None;
        }
        let prev = &v[v.len() - 2];
        let last = &v[v.len() - 1];
        let (prev_reading, _prev_surface, _) = prev;
        let (_last_reading, last_surface, _) = last;
        if prev_reading.is_empty()
            || last_surface.is_empty()
            || prev_reading == _last_reading
            || _prev_surface == last_surface
        {
            return None;
        }
        Some((prev_reading.clone(), last_surface.clone()))
    })
}

/// 確定のたびに観察ログから訂正選好を **再生成して稼働エンジンへ即反映** する。
///
/// これが無いと ①(picked-index)/②(打ち直し) の学習は観察ログに溜まるだけで、
/// 手動 relearn するまで変換に効かなかった (ユーザー報告「5回選んでも治らない」)。
/// 確定ごとに `extract_corrections` を回して corrections.toml を更新＋live 反映する。
///
/// **オプトイン無効 (観察ログ無効) のときは何もしない** — 観察が空なので relearn
/// すると既存 corrections を消してしまうため。決定論なので churn は生まない。
pub fn auto_relearn_after_commit() {
    // 観察ログが有効なときだけ (無効なら既存 corrections を壊さない)。
    if !with_observation(ObservationLog::is_enabled) {
        return;
    }
    let Some(dir) = nuko_app_support_dir() else {
        return;
    };
    let log = ObservationLog::new(true, dir.join("observations.jsonl"));
    let Ok(events) = log.read_all() else {
        return;
    };
    let store = extract_corrections(&events, MIN_SEEN);
    let _ = store.save(dir.join("corrections.toml"));
    with_engine_mut(|engine| engine.set_corrections(store));
}

/// 指定の読みに対する単語登録候補の表層を返す (候補ウィンドウの Tab ヒント/登録用)。
///
/// [`registration_candidate`] の「1つ前の読み」が `reading` と一致するときだけ `Some`。
/// = 「今打っている読みが、直前に別読みで出した語の登録対象」のときにヒントを出せる。
pub fn pending_registration_for(reading: &str) -> Option<String> {
    registration_candidate().and_then(|(r, s)| if r == reading { Some(s) } else { None })
}

/// 単語登録: 稼働エンジンのユーザー辞書に `(reading → surface)` を追加し保存する。
///
/// ユーザー辞書候補は変換で最優先 (USER_DICT_BOOST) になるので、登録後は
/// その読みで登録語が 1 位に出る。保存先は `~/…/nuko-ime/user_dict.json`。
pub fn register_word(reading: &str, surface: &str) -> nuko_core::error::Result<()> {
    with_engine_mut(|engine| {
        engine
            .dictionary_mut()
            .user_dictionary_mut()
            .add(nuko_core::dictionary::UserEntry::new(surface, reading))?;
        engine.dictionary_mut().save_user_dictionary()
    })
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
    setup_user_dictionary(&mut engine);
    engine.set_kyujitai_mode(kyujitai_enabled());
    Ok(engine)
}

#[cfg(not(feature = "akaza"))]
fn build_engine() -> nuko_core::error::Result<ConversionEngine> {
    let mut engine = ConversionEngine::new()?;
    setup_learning_persistence(&mut engine);
    setup_corrections(&mut engine);
    setup_user_dictionary(&mut engine);
    engine.set_kyujitai_mode(kyujitai_enabled());
    Ok(engine)
}

/// 旧字体モードが有効か (app support dir の `KYUJITAI_ENABLED` マーカーの有無)。既定 OFF。
#[must_use]
pub fn kyujitai_enabled() -> bool {
    nuko_app_support_dir()
        .map(|d| d.join("KYUJITAI_ENABLED").exists())
        .unwrap_or(false)
}

/// 旧字体モードをトグルして **新しい状態** を返す。
///
/// マーカーファイル `KYUJITAI_ENABLED` を作成/削除し、稼働中エンジンへも即反映する。
/// dir 取得不可時は false を返す (変更なし)。
pub fn toggle_kyujitai() -> bool {
    let Some(dir) = nuko_app_support_dir() else {
        return false;
    };
    let marker = dir.join("KYUJITAI_ENABLED");
    let now_on = if marker.exists() {
        let _ = std::fs::remove_file(&marker);
        false
    } else {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&marker, b"");
        true
    };
    with_engine_mut(|engine| engine.set_kyujitai_mode(now_on));
    tracing::info!(enabled = now_on, "旧字体モードを切り替え");
    now_on
}

/// 単語登録の永続辞書 (`~/…/nuko-ime/user_dict.json`) を起動時にロードする。
///
/// ファイルが無くても path を設定するため、以後の [`register_word`] の保存が効く。
fn setup_user_dictionary(engine: &mut ConversionEngine) {
    let Some(dir) = nuko_app_support_dir() else {
        return;
    };
    let path = dir.join("user_dict.json");
    if let Err(e) = engine.dictionary_mut().load_user_dictionary(&path) {
        tracing::warn!(error = %e, "ユーザー辞書 (単語登録) の load 失敗。空で継続");
    }
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
    // 文節位置の学習 (ユーザーが Shift で切り直した切り方) を load する。
    if let Err(e) = engine.load_segmentations(dir.join("segmentations.toml")) {
        tracing::warn!(error = %e, "segmentations.toml load 失敗 (文節学習なしで継続)");
    }
}

/// 文節の「切り方」を学習して保存し、稼働エンジンへ即反映する。
///
/// ユーザーが Shift+←→ で文節を切り直して確定したとき、controller が呼ぶ。
/// `segments` は各文節の読み。連結が `reading` に一致しない等は学習されない
/// ([`ConversionEngine::learn_segmentation`] の規約)。
pub fn learn_segmentation_and_save(reading: &str, segments: &[String]) {
    let Some(dir) = nuko_app_support_dir() else {
        return;
    };
    with_engine_mut(|engine| {
        if engine.learn_segmentation(reading, segments) {
            if let Err(e) = engine.segmentations().save(dir.join("segmentations.toml")) {
                tracing::warn!(error = %e, "segmentations.toml 保存失敗");
            }
        }
    });
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

/// 同梱/インストール済みの `nuko` CLI を探す。
///
/// 1) `.app` に同梱 (current_exe の隣: `Contents/MacOS/nuko`)
/// 2) よくあるインストール先 (`~/.cargo/bin` / `/usr/local/bin` / `/opt/homebrew/bin`)
fn resolve_nuko_binary() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("nuko");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        cands.push(std::path::PathBuf::from(home).join(".cargo/bin/nuko"));
    }
    cands.push(std::path::PathBuf::from("/usr/local/bin/nuko"));
    cands.push(std::path::PathBuf::from("/opt/homebrew/bin/nuko"));
    cands.into_iter().find(|p| p.exists())
}

/// corrections.toml を稼働中エンジンへ再読込する (AI dreaming 適用後など)。
fn reload_corrections() {
    if let Some(dir) = nuko_app_support_dir() {
        let path = dir.join("corrections.toml");
        if let Err(e) = with_engine_mut(|engine| engine.load_corrections(&path)) {
            tracing::warn!(error = %e, "corrections 再読込に失敗");
        }
    }
}

/// メニュー「AIで学習を研ぎ直す」: 同梱 `nuko dream --apply` を実行し結果テキストを返す。
///
/// 設定 (provider/model) は `ai.toml`、API キーは Keychain を `nuko` 側が読む。
/// 成功時は corrections.toml を再読込して稼働エンジンへ即反映する。
///
/// 注: 同期実行のため API 呼び出し中は数秒ブロックする (将来 非同期化)。
pub fn run_ai_dreaming() -> std::result::Result<String, String> {
    let Some(nuko) = resolve_nuko_binary() else {
        return Err(
            "nuko CLI が見つかりません。NukoIME.app に同梱されていない可能性があります\n\
             (install.sh を更新して再インストールしてください)。"
                .to_string(),
        );
    };
    let out = std::process::Command::new(&nuko)
        .args(["dream", "--apply"])
        .output()
        .map_err(|e| format!("nuko 実行に失敗: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        reload_corrections();
        Ok(format!(
            "🤖 AIで学習を研ぎ直しました\n\n{}\n\n✅ 学習に反映しました (再起動不要)。",
            stdout.trim()
        ))
    } else {
        Err(format!(
            "AI dreaming に失敗しました:\n\n{}\n{}\n\n\
             ・キー未設定なら: ターミナルで `nuko config set-key anthropic`\n\
             ・ai.toml に enabled=true / provider=\"anthropic\" / model=\"...\" を設定",
            stdout.trim(),
            stderr.trim()
        ))
    }
}

// BYOK 設定ウィンドウ (アプリ全体で 1 つ)。
thread_local! {
    static SETTINGS_WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
}

/// 設定ウィンドウを前面に表示する (初回は生成)。ボタンの target は窓が内部で保持する
/// 安定オブジェクトなので controller は不要。
pub fn show_settings(mtm: MainThreadMarker) {
    SETTINGS_WINDOW.with(|cell| {
        let mut w = cell.borrow_mut();
        if w.is_none() {
            *w = Some(SettingsWindow::new(mtm));
        }
        let model = current_ai_model().unwrap_or_else(|| "claude-sonnet-5-5".to_string());
        if let Some(win) = w.as_ref() {
            win.show(mtm, &model);
        }
    });
}

/// 設定ウィンドウの「保存」: キーを Keychain、provider/model を ai.toml に保存する。
pub fn save_settings() {
    SETTINGS_WINDOW.with(|cell| {
        let w = cell.borrow();
        let Some(win) = w.as_ref() else {
            return;
        };
        let provider = win.selected_provider();
        // API キーは貼り付け時の空白・改行を完全除去する (フィールドで折り返して見えても確実に保存)。
        let key: String = win
            .key_value()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let model = win.model_value();
        let model = model.trim();

        // キーが入力されていれば Keychain へ (同梱 nuko config set-key を stdin 経由)。
        if !key.is_empty() {
            if let Err(e) = save_key_via_cli(provider, &key) {
                win.set_status(&format!("⚠️ キー保存失敗: {e}"));
                return;
            }
            win.clear_key();
        }

        // provider/model を ai.toml に保存 (enabled=true)。
        if let Err(e) = write_ai_toml(provider, model) {
            win.set_status(&format!("⚠️ ai.toml 書込失敗: {e}"));
            return;
        }

        win.set_status(&format!(
            "✅ 保存しました（{provider}）。メニューの「🤖 AIで学習」が使えます"
        ));
    });
}

/// 設定ウィンドウを閉じる (activation policy を元に戻して IME を background へ)。
pub fn close_settings(mtm: MainThreadMarker) {
    SETTINGS_WINDOW.with(|cell| {
        if let Some(win) = cell.borrow().as_ref() {
            win.close(mtm);
        }
    });
}

/// 設定ウィンドウの「辞書に登録」: 読み＋表層を稼働中エンジンへ追加＋保存する (即反映)。
pub fn add_dict_entry_from_window() {
    SETTINGS_WINDOW.with(|cell| {
        let w = cell.borrow();
        let Some(win) = w.as_ref() else {
            return;
        };
        let reading = win.reading_value();
        let surface = win.surface_value();
        let reading = reading.trim();
        let surface = surface.trim();
        if reading.is_empty() || surface.is_empty() {
            win.set_status("⚠️ 読みと表層の両方を入力してください");
            return;
        }
        // 稼働中エンジンのユーザー辞書に追加＋保存 (user_dict.json)。CLI と違い即反映。
        let result = with_engine_mut(|engine| {
            let ud = engine.dictionary_mut().user_dictionary_mut();
            ud.add(nuko_core::dictionary::UserEntry::new(surface, reading))?;
            ud.save()
        });
        match result {
            Ok(()) => {
                win.clear_dict_fields();
                win.set_status(&format!(
                    "✅ 「{reading}」→「{surface}」を登録しました（即反映）"
                ));
            }
            Err(e) => win.set_status(&format!("⚠️ 登録失敗: {e}")),
        }
    });
}

/// ai.toml から model を読む (簡易パース。無ければ None)。
fn current_ai_model() -> Option<String> {
    let dir = nuko_app_support_dir()?;
    let text = std::fs::read_to_string(dir.join("ai.toml")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("model") {
            if let Some(eq) = rest.find('=') {
                let val = rest[eq + 1..].trim().trim_matches('"').trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// 同梱 `nuko config set-key <provider>` を stdin 経由で呼びキーを Keychain に保存する。
fn save_key_via_cli(provider: &str, key: &str) -> std::result::Result<(), String> {
    use std::io::Write;
    let nuko = resolve_nuko_binary().ok_or_else(|| "nuko CLI が見つかりません".to_string())?;
    let mut child = std::process::Command::new(&nuko)
        .args(["config", "set-key", provider])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("nuko 起動失敗: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(key.as_bytes())
            .map_err(|e| format!("stdin 書込失敗: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("nuko 実行失敗: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// ai.toml を書き出す (enabled=true + provider + model)。
fn write_ai_toml(provider: &str, model: &str) -> std::io::Result<()> {
    let Some(dir) = nuko_app_support_dir() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "app support dir なし",
        ));
    };
    std::fs::create_dir_all(&dir)?;
    let model = if model.is_empty() {
        "claude-sonnet-5-5"
    } else {
        model
    };
    let content = format!("enabled = true\nprovider = \"{provider}\"\nmodel = \"{model}\"\n");
    std::fs::write(dir.join("ai.toml"), content)
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
    /// この入力で文節を Shift+←→ で切り直したか。
    ///
    /// `true` のまま確定したら「ユーザーが明示的に直した切り方」とみなし、
    /// 文節位置を学習する ([`learn_segmentation_and_save`])。既定の切り方は学習しない。
    pub segment_resized: bool,
    /// 現在の composition に対して**打った生ローマ字 (ascii)** の連結。
    ///
    /// F9/F10 (全角/半角英数) は「かな」でなく打った ascii を変換するため保持する
    /// (例「howaito」→ ホワイト だが F10 では howaito を出す)。新規入力で積み、
    /// reset で消える。
    pub raw_input: String,
    /// F9/F10 連打の大小文字サイクル段階 (0=小文字 / 1=大文字 / 2=先頭大文字)。
    ///
    /// 同じ英数 F キーを連打すると 3 段で循環する。新しい文字入力が来たら 0 に戻す。
    pub fkey_alnum_stage: u8,
    // 注: 「かな」キー押下時刻は controller 横断で効かせる必要があるため
    // InputState (per-controller ivar) ではなく thread_local `LAST_KANA_PRESS`
    // (このファイル上部) で保持する。落とし穴 #4 (controller 複数生成) 対策。
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
            segment_resized: false,
            raw_input: String::new(),
            fkey_alnum_stage: 0,
        }
    }

    /// 状態をリセット（確定・取消後）
    pub fn reset(&mut self) {
        self.romaji.clear();
        self.composition.clear();
        self.candidates = None;
        self.segmented = None;
        self.is_composing = false;
        self.raw_input.clear();
        self.fkey_alnum_stage = 0;
        self.segment_resized = false;
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
