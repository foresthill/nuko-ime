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
use objc2::{define_class, msg_send, DefinedClass};
use objc2_foundation::{NSArray, NSRange, NSString};
use objc2_input_method_kit::{IMKInputController, IMKServer};
use tracing::{debug, error, info, warn};

use crate::state::{InputState, ENGINE};

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
        }

        /// 入力メソッドが非アクティブになった
        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, sender: Option<&AnyObject>) {
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
        //   - それ以外 → パススルー (半角スペース入力)
        if text == " " {
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
            } else {
                return Bool::NO;
            }
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
                    let mut engine = ENGINE.lock();
                    let _ = engine.commit(selected, &state.context);
                }
            }

            state.reset();
            drop(state);
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

        let engine = ENGINE.lock();
        match engine.convert(&composition, &state.context) {
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
                    drop(engine);
                    Self::set_marked_text_on_client(client, &surface);
                } else {
                    debug_log("do_convert: no selected candidate, showing composition");
                    let display = state.display_text();
                    state.candidates = Some(candidates);
                    drop(state);
                    drop(engine);
                    Self::set_marked_text_on_client(client, &display);
                }
            }
            Err(e) => {
                warn!("変換エラー: {e}");
                debug_log(&format!("do_convert: ERROR {e}"));
                let display = state.display_text();
                drop(state);
                drop(engine);
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
                let mut engine = ENGINE.lock();
                if let Err(e) = engine.commit(selected, &state.context) {
                    error!("学習記録エラー: {}", e);
                }
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

        if !commit_text.is_empty() {
            Self::insert_text_on_client(client, &commit_text);
        }
    }

    /// 取消を実行
    fn do_cancel(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();
        state.reset();
        drop(state);

        Self::insert_text_on_client(client, "");
    }

    /// バックスペース処理
    fn do_backspace(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();

        if state.candidates.is_some() {
            state.candidates = None;
            let display = state.display_text();
            drop(state);
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
}
