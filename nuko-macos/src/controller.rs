//! NukoInputController - IMKInputController サブクラス
//!
//! macOS InputMethodKit と nuko-core を橋渡しするコントローラ。
//! ユーザーのキー入力を受け取り、ローマ字→かな→漢字変換を行う。
//!
//! イベントディスパッチ:
//! - inputText:client: → 文字キー入力（"a", "b", " " 等）
//! - didCommandBySelector:client: → アクションキー（Enter, Escape, 矢印等）

use std::cell::RefCell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker};
use objc2_foundation::{NSArray, NSAttributedString, NSRange, NSString};
use objc2_input_method_kit::{kIMKLocateCandidatesBelowHint, IMKInputController, IMKServer};
use tracing::{debug, error, info, warn};

use nuko_core::conversion::Candidate;

use crate::state::{
    ensure_candidates_panel, with_candidates_panel, with_engine, with_engine_mut, InputState,
};

/// NSNotFound 相当値 (IMK の replacementRange で使用)
/// macOS ヘッダでは NSIntegerMax と定義されている
const NS_NOT_FOUND: usize = isize::MAX as usize;

/// デバッグログをファイルに書き出し（IMEプロセスのstdoutは見えないため）
fn debug_log(msg: &str) {
    use std::io::Write;
    let path = std::path::Path::new("/tmp/nuko-ime-debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(f, "[{now}] {msg}");
    }
}

// --- Ivars ---

pub struct NukoControllerIvars {
    state: RefCell<InputState>,
}

impl Default for NukoControllerIvars {
    fn default() -> Self {
        Self {
            state: RefCell::new(InputState::new()),
        }
    }
}

// --- Class Definition ---

define_class!(
    // IMKInputController を継承
    #[unsafe(super(IMKInputController))]
    #[name = "NukoInputController"]
    #[ivars = NukoControllerIvars]
    pub struct NukoInputController;

    unsafe impl NSObjectProtocol for NukoInputController {}

    impl NukoInputController {
        /// IMKServer から呼ばれる初期化メソッド
        /// これをオーバーライドしないと ivars が未初期化でクラッシュする
        #[unsafe(method_id(initWithServer:delegate:client:))]
        fn init_with_server(
            this: Allocated<Self>,
            server: Option<&IMKServer>,
            delegate: Option<&AnyObject>,
            client: Option<&AnyObject>,
        ) -> Option<Retained<Self>> {
            debug_log("initWithServer:delegate:client: called");
            // ivars を先にセットして Allocated → PartialInit に変換
            let this = this.set_ivars(NukoControllerIvars::default());
            // super の init を呼ぶ
            let this: Option<Retained<Self>> = unsafe {
                msg_send![super(this), initWithServer: server, delegate: delegate, client: client]
            };
            if this.is_some() {
                debug_log("initWithServer succeeded, ivars initialized");
                // 候補ウィンドウ singleton を初期化 (まだ未生成なら 1 度だけ作成)。
                // 失敗しても致命傷ではないので握りつぶす (旧挙動と同等で動作)。
                if let Some(server) = server {
                    let mtm = MainThreadMarker::new()
                        .expect("initWithServer must run on the main thread");
                    ensure_candidates_panel(server, mtm);
                }
            } else {
                debug_log("initWithServer returned nil!");
            }
            this
        }

        /// 文字キー入力を処理する
        /// キーバインディング経由で呼ばれる（"a", "k", " " 等の文字）
        #[unsafe(method(inputText:client:))]
        fn input_text_client(&self, string: Option<&NSString>, sender: Option<&AnyObject>) -> Bool {
            debug_log(&format!("inputText called: {:?}", string.map(|s| s.to_string())));
            self._input_text_impl(string, sender)
        }

        /// アクションセレクタを処理する
        /// Enter, Escape, 矢印キー等がここに来る
        #[unsafe(method(didCommandBySelector:client:))]
        fn did_command_by_selector(&self, selector: Sel, sender: Option<&AnyObject>) -> Bool {
            debug_log(&format!("didCommandBySelector called: {:?}", selector.name()));
            self._did_command_impl(selector, sender)
        }

        /// 候補リストを返す
        #[unsafe(method_id(candidates:))]
        fn candidates_for_sender(
            &self,
            _sender: Option<&AnyObject>,
        ) -> Option<Retained<NSArray>> {
            self._candidates_impl()
        }

        /// 入力メソッドがアクティブになった
        #[unsafe(method(activateServer:))]
        fn activate_server(&self, _sender: Option<&AnyObject>) {
            debug_log("=== NukoIME activateServer called ===");
            info!("NukoIME activated");
            // ソース切替ショートカットの Space キーが活性化直後に
            // inputText として漏れることがあるため、活性化時刻を記録して
            // input_text 側で短時間以内の Space を判定可能にする
            self.ivars().state.borrow_mut().activated_at =
                Some(std::time::Instant::now());
        }

        /// 入力メソッドが非アクティブになった
        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, sender: Option<&AnyObject>) {
            debug_log("=== NukoIME deactivateServer called ===");
            info!("NukoIME deactivated");
            let is_composing = self.ivars().state.borrow().is_composing;
            if is_composing {
                if let Some(client) = sender {
                    self.do_commit(client);
                }
            }
        }

        /// 組み立て中テキストを確定するよう要求された
        #[unsafe(method(commitComposition:))]
        fn commit_composition(&self, sender: Option<&AnyObject>) {
            let is_composing = self.ivars().state.borrow().is_composing;
            if is_composing {
                if let Some(client) = sender {
                    self.do_commit(client);
                }
            }
        }

        /// 候補ウィンドウで選択中の候補が変わった時に IMK から呼ばれる
        ///
        /// パネル上で矢印キー / Space で navigate した時に発火する。
        /// 確定ではなくプレビュー目的なので、state 側の選択インデックス更新のみ。
        #[unsafe(method(candidateSelectionChanged:))]
        fn candidate_selection_changed(
            &self,
            candidate_string: Option<&NSAttributedString>,
        ) {
            self._candidate_selection_changed_impl(candidate_string);
        }

        /// 候補ウィンドウで候補が最終確定された時に IMK から呼ばれる
        ///
        /// Return キー / 数字キー (1-9) で候補が選択された時に発火する。
        /// dismissesAutomatically=true (default) によりパネルは閉じた状態で来る。
        #[unsafe(method(candidateSelected:))]
        fn candidate_selected(&self, candidate_string: Option<&NSAttributedString>) {
            self._candidate_selected_impl(candidate_string);
        }
    }
);

// --- メソッド実装 ---

impl NukoInputController {
    /// inputText:client: の実装
    fn _input_text_impl(&self, string: Option<&NSString>, sender: Option<&AnyObject>) -> Bool {
        let Some(ns_str) = string else {
            return Bool::NO;
        };
        let text = ns_str.to_string();
        debug!("inputText: '{}'", text);

        let japanese_mode = self.ivars().state.borrow().japanese_mode;

        // 英数モードの場合パススルー
        if !japanese_mode {
            return Bool::NO;
        }

        let Some(client) = sender else {
            return Bool::NO;
        };

        let mut state = self.ivars().state.borrow_mut();

        // スペースキー:
        //   - 候補表示中 → 次候補へ巡回
        //   - 未確定文字列がある → 変換実行
        //   - **活性化直後 (150ms 以内) の Space → ソース切替の Ctrl+Space
        //     ショートカット由来の漏れと判定して破棄**
        //   - それ以外 → 全角スペース「　」(U+3000) を入力 (日本語モードの慣例)
        //
        // 半角スペースを入れるには英数モードに切り替えてから入力する。
        // 旧実装は Bool::NO で host にパススルーしていたが、ソース切替の
        // ショートカット (Ctrl+Space 等) で Space が漏れた時に意図しない半角が
        // 入ってしまうため、日本語モード中は常に消費する方針に変更
        // (Mozc / Google 日本語入力等の標準挙動)。
        if text == " " {
            // 活性化直後の Space は破棄 (ソース切替の漏れ対策)
            const ACTIVATION_GUARD_MS: u128 = 150;
            if let Some(activated_at) = state.activated_at {
                if activated_at.elapsed().as_millis() < ACTIVATION_GUARD_MS
                    && state.candidates.is_none()
                    && !state.is_composing
                {
                    state.activated_at = None; // 1 shot で消費
                    debug_log("space: discard (activation guard, likely source-switch leak)");
                    return Bool::YES;
                }
            }

            if state.candidates.is_some() {
                if let Some(ref mut candidates) = state.candidates {
                    candidates.select_next();
                    let surface = candidates
                        .selected()
                        .map(|s| s.surface.clone())
                        .unwrap_or_default();
                    debug_log(&format!("space: cycle to next candidate '{surface}'"));
                    drop(state);
                    Self::set_marked_text_on_client(client, &surface);
                }
                return Bool::YES;
            }
            if state.is_composing {
                drop(state);
                self.do_convert(client);
                return Bool::YES;
            }
            // 未確定状態: 全角スペースを直接挿入
            drop(state);
            debug_log("space: insert full-width space (no composition)");
            Self::insert_text_on_client(client, "\u{3000}");
            return Bool::YES;
        }

        // 候補選択中に文字を打ったら確定して新しい入力開始
        if state.candidates.is_some() {
            let commit_text = state
                .candidates
                .as_ref()
                .and_then(|c| c.selected())
                .map(|s| s.surface.clone())
                .unwrap_or_else(|| state.composition.clone());

            if let Some(ref candidates) = state.candidates {
                if let Some(selected) = candidates.selected() {
                    with_engine_mut(|engine| {
                        let _ = engine.commit(selected, &state.context);
                    });
                }
            }

            state.reset();
            drop(state);
            Self::hide_candidate_panel();
            Self::insert_text_on_client(client, &commit_text);

            // 新しい文字の入力を開始
            let mut state = self.ivars().state.borrow_mut();
            for c in text.chars() {
                if c.is_ascii_graphic() {
                    let kana = state.romaji.input(c);
                    if !kana.is_empty() {
                        state.composition.push_str(&kana);
                    }
                }
            }
            state.is_composing = true;
            let display = state.display_text();
            drop(state);
            Self::set_marked_text_on_client(client, &display);
            return Bool::YES;
        }

        // 通常の文字入力処理
        let mut any_processed = false;
        for c in text.chars() {
            if c.is_ascii_graphic() {
                let kana = state.romaji.input(c);
                if !kana.is_empty() {
                    state.composition.push_str(&kana);
                }
                any_processed = true;
            }
        }

        if !any_processed {
            return Bool::NO;
        }

        state.is_composing = true;

        let display = state.display_text();
        drop(state);
        Self::set_marked_text_on_client(client, &display);

        Bool::YES
    }

    /// didCommandBySelector:client: の実装
    fn _did_command_impl(&self, selector: Sel, sender: Option<&AnyObject>) -> Bool {
        let is_composing = self.ivars().state.borrow().is_composing;

        let Some(client) = sender else {
            return Bool::NO;
        };

        // かな/英数キーのセレクタ処理
        let sel_name = selector.name();

        // 未確定状態でない場合は基本パススルー
        if !is_composing {
            return Bool::NO;
        }

        // セレクタ名を C 文字列リテラルで比較
        let insert_newline = c"insertNewline:";
        let cancel_op = c"cancelOperation:";
        let delete_back = c"deleteBackward:";
        let move_down = c"moveDown:";
        let move_up = c"moveUp:";

        if sel_name == insert_newline {
            // Enter: 確定
            self.do_commit(client);
            Bool::YES
        } else if sel_name == cancel_op {
            // Escape: 取消
            self.do_cancel(client);
            Bool::YES
        } else if sel_name == delete_back {
            // Backspace: 削除
            self.do_backspace(client);
            Bool::YES
        } else if sel_name == move_down {
            // Down: 次候補
            let mut state = self.ivars().state.borrow_mut();
            if let Some(ref mut candidates) = state.candidates {
                candidates.select_next();
                if let Some(selected) = candidates.selected() {
                    let surface = selected.surface.clone();
                    drop(state);
                    Self::set_marked_text_on_client(client, &surface);
                }
            }
            Bool::YES
        } else if sel_name == move_up {
            // Up: 前候補
            let mut state = self.ivars().state.borrow_mut();
            if let Some(ref mut candidates) = state.candidates {
                candidates.select_prev();
                if let Some(selected) = candidates.selected() {
                    let surface = selected.surface.clone();
                    drop(state);
                    Self::set_marked_text_on_client(client, &surface);
                }
            }
            Bool::YES
        } else {
            debug_log(&format!("unhandled selector: {sel_name:?}"));
            // 未知のセレクタ: 確定してパススルー
            self.do_commit(client);
            Bool::NO
        }
    }

    fn _candidates_impl(&self) -> Option<Retained<NSArray>> {
        let state = self.ivars().state.borrow();
        let candidates = state.candidates.as_ref()?;

        if candidates.is_empty() {
            return None;
        }

        let ns_strings: Vec<Retained<NSString>> = candidates
            .iter()
            .map(|c| NSString::from_str(&c.surface))
            .collect();

        let array: Retained<NSArray<NSString>> = NSArray::from_retained_slice(&ns_strings);
        Some(unsafe { Retained::cast_unchecked(array) })
    }

    /// クライアントに setMarkedText を送信
    ///
    /// replacementRange.location = NSNotFound で「現在のマークテキストを置換」を指示。
    /// (0,0) を渡すと macOS はドキュメント先頭に書こうとするため未確定文字列が見えない。
    fn set_marked_text_on_client(client: &AnyObject, text: &str) {
        let ns_string = NSString::from_str(text);
        let text_len = text.encode_utf16().count();
        let sel_range = NSRange::new(text_len, 0);
        let rep_range = NSRange::new(NS_NOT_FOUND, 0);
        debug_log(&format!("setMarkedText: '{text}' (utf16_len={text_len})"));
        unsafe {
            let _: () = msg_send![
                client,
                setMarkedText: &*ns_string,
                selectionRange: sel_range,
                replacementRange: rep_range
            ];
        }
    }

    /// クライアントに insertText を送信
    ///
    /// replacementRange.location = NSNotFound で「マークテキストを置換して確定」を指示。
    fn insert_text_on_client(client: &AnyObject, text: &str) {
        let ns_string = NSString::from_str(text);
        let rep_range = NSRange::new(NS_NOT_FOUND, 0);
        debug_log(&format!("insertText: '{text}'"));
        unsafe {
            let _: () = msg_send![
                client,
                insertText: &*ns_string,
                replacementRange: rep_range
            ];
        }
    }

    /// 変換を実行
    fn do_convert(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();

        let remaining = state.romaji.flush();
        if !remaining.is_empty() {
            state.composition.push_str(&remaining);
        }

        if state.composition.is_empty() {
            debug_log("do_convert: composition empty, skipping");
            return;
        }

        let composition = state.composition.clone();
        debug_log(&format!("do_convert: input='{composition}'"));

        let result = with_engine(|engine| engine.convert(&composition, &state.context));
        match result {
            Ok(candidates) => {
                let count = candidates.iter().count();
                let preview: Vec<String> = candidates
                    .iter()
                    .take(5)
                    .map(|c| c.surface.clone())
                    .collect();
                debug_log(&format!("do_convert: got {count} candidates: {preview:?}"));

                if let Some(selected) = candidates.selected() {
                    let surface = selected.surface.clone();
                    state.candidates = Some(candidates);
                    drop(state);
                    Self::set_marked_text_on_client(client, &surface);
                    self.show_candidate_panel();
                } else {
                    debug_log("do_convert: no selected candidate, showing composition");
                    let display = state.display_text();
                    state.candidates = Some(candidates);
                    drop(state);
                    Self::set_marked_text_on_client(client, &display);
                    self.show_candidate_panel();
                }
            }
            Err(e) => {
                warn!("変換エラー: {e}");
                debug_log(&format!("do_convert: ERROR {e}"));
                let display = state.display_text();
                drop(state);
                Self::set_marked_text_on_client(client, &display);
            }
        }
    }

    /// 確定を実行
    fn do_commit(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();

        let commit_text = if let Some(ref candidates) = state.candidates {
            if let Some(selected) = candidates.selected() {
                let text = selected.surface.clone();
                with_engine_mut(|engine| {
                    if let Err(e) = engine.commit(selected, &state.context) {
                        error!("学習記録エラー: {}", e);
                    }
                });
                state.context.push_prev_word(&text);
                text
            } else {
                state.display_text()
            }
        } else {
            let remaining = state.romaji.flush();
            if !remaining.is_empty() {
                state.composition.push_str(&remaining);
            }
            let text = state.composition.clone();
            if !text.is_empty() {
                state.context.push_prev_word(&text);
            }
            text
        };

        state.reset();
        drop(state);

        Self::hide_candidate_panel();

        if !commit_text.is_empty() {
            Self::insert_text_on_client(client, &commit_text);
        }
    }

    /// 取消を実行
    fn do_cancel(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();
        state.reset();
        drop(state);

        Self::hide_candidate_panel();

        Self::insert_text_on_client(client, "");
    }

    /// バックスペース処理
    fn do_backspace(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();

        if state.candidates.is_some() {
            state.candidates = None;
            let display = state.display_text();
            drop(state);
            Self::hide_candidate_panel();
            Self::set_marked_text_on_client(client, &display);
            return;
        }

        if !state.romaji.buffer().is_empty() {
            state.romaji.clear();
            if state.composition.is_empty() {
                state.is_composing = false;
                drop(state);
                Self::insert_text_on_client(client, "");
            } else {
                let display = state.display_text();
                drop(state);
                Self::set_marked_text_on_client(client, &display);
            }
            return;
        }

        if !state.composition.is_empty() {
            state.composition.pop();
            if state.composition.is_empty() {
                state.is_composing = false;
                drop(state);
                Self::insert_text_on_client(client, "");
            } else {
                let display = state.display_text();
                drop(state);
                Self::set_marked_text_on_client(client, &display);
            }
            return;
        }

        state.is_composing = false;
        drop(state);
        Self::insert_text_on_client(client, "");
    }

    // --- 候補ウィンドウ (singleton IMKCandidates) -----------------------

    /// 候補ウィンドウを表示する。
    ///
    /// 既存の `candidates:` callback (= `_candidates_impl`) が現在の controller の
    /// `state.candidates` を NSArray にして返すので、`updateCandidates` 経由で
    /// IMK にパネル描画を任せる。`setCandidateData:` を使う旧経路 (PR #29) は
    /// **使わない** — Apple の "documented path" がこちらだから。
    fn show_candidate_panel(&self) {
        debug_log("show_candidate_panel: requesting updateCandidates + show");
        with_candidates_panel(|panel| {
            let Some(panel) = panel else {
                debug_log("show_candidate_panel: panel not initialized, skipping");
                return;
            };
            unsafe {
                panel.updateCandidates();
                panel.show(kIMKLocateCandidatesBelowHint as usize);
            }
        });
    }

    /// 候補ウィンドウを隠す (singleton; 全 controller から共有)
    ///
    /// `&self` を取らないのは、複数 controller から共通の singleton を触る関係上
    /// 「どの controller の hide か」を区別する必要が無いから (associated function)。
    fn hide_candidate_panel() {
        with_candidates_panel(|panel| {
            if let Some(panel) = panel {
                unsafe {
                    if panel.isVisible() {
                        debug_log("hide_candidate_panel");
                        panel.hide();
                    }
                }
            }
        });
    }

    /// `candidateSelectionChanged:` の実装
    ///
    /// パネル内で navigate された候補を `state.candidates.selected` に反映するのみ。
    /// 確定はしない。marked text 更新は client ハンドルがここに渡らないため省略
    /// (IMK は marked を保持したままパネル選択を更新する仕様)。
    fn _candidate_selection_changed_impl(&self, candidate_string: Option<&NSAttributedString>) {
        let Some(attr) = candidate_string else {
            return;
        };
        let surface = attr.string().to_string();
        debug_log(&format!("candidateSelectionChanged: '{surface}'"));

        let mut state = self.ivars().state.borrow_mut();
        if let Some(candidates) = state.candidates.as_mut() {
            let idx = candidates.iter().position(|c| c.surface == surface);
            if let Some(idx) = idx {
                candidates.select(idx);
            }
        }
    }

    /// `candidateSelected:` の実装
    ///
    /// パネル上で最終確定された候補を学習に反映し、IMK クライアントに `insertText:`
    /// する。`client` はメソッド引数で渡らないので `[self client]` で取得する
    /// (= IMK が ivar として保持している現在の client)。
    fn _candidate_selected_impl(&self, candidate_string: Option<&NSAttributedString>) {
        let Some(attr) = candidate_string else {
            return;
        };
        let surface = attr.string().to_string();
        debug_log(&format!("candidateSelected: '{surface}'"));

        let client_ptr: *mut AnyObject = unsafe { msg_send![self, client] };
        if client_ptr.is_null() {
            warn!("candidateSelected: client is nil, skipping insert");
            return;
        }
        let client: &AnyObject = unsafe { &*client_ptr };

        // borrow checker 回避: 学習に使う Candidate を先に取り出してから
        // 別スコープで with_engine_mut を呼ぶ
        let selected: Option<Candidate> = {
            let mut state = self.ivars().state.borrow_mut();
            if let Some(candidates) = state.candidates.as_mut() {
                let idx = candidates.iter().position(|c| c.surface == surface);
                if let Some(idx) = idx {
                    candidates.select(idx);
                    candidates.selected().cloned()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(ref selected) = selected {
            let ctx_snapshot = self.ivars().state.borrow().context.clone();
            with_engine_mut(|engine| {
                if let Err(e) = engine.commit(selected, &ctx_snapshot) {
                    error!("学習記録エラー (panel): {e}");
                }
            });
            self.ivars()
                .state
                .borrow_mut()
                .context
                .push_prev_word(&selected.surface);
        }

        self.ivars().state.borrow_mut().reset();
        // dismissesAutomatically=true (default) でこの時点でパネルは閉じている
        Self::insert_text_on_client(client, &surface);
    }
}
