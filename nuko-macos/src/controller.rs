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
use objc2_app_kit::{NSEvent, NSEventType, NSScreen};
use objc2_foundation::{NSArray, NSPoint, NSRange, NSString};
use objc2_input_method_kit::{IMKInputController, IMKServer};
use tracing::{debug, error, info, warn};

use nuko_core::conversion::CandidateList;

use crate::commit::CommandAction;
use crate::state::{
    ensure_custom_panel, with_custom_panel, with_engine, with_engine_mut, InputState,
};

/// NSNotFound 相当値 (IMK の replacementRange で使用)
/// macOS ヘッダでは NSIntegerMax と定義されている
const NS_NOT_FOUND: usize = isize::MAX as usize;

/// ASCII 句読点・記号を 全角 に変換 (日本語入力モードでの自動変換用)。
///
/// 主に JIS 標準で 全角が一般的なものに絞っている:
/// - 句読点 (, .)
/// - 疑問符・感嘆符 (? !)
/// - 括弧類 (( ) [ ] { })
/// - 中黒・波 (~)
/// - その他よく使う記号 (; : @ # $ % & * + =)
///
/// 含まれないもの (= 半角のままがよく使われる):
/// - 演算子 (- / \ < >)
/// - 引用符 (' " `)
/// - アンダースコア _
/// - パイプ |
/// - キャレット ^
///
/// 戻り値が `None` の場合は変換不要 = romaji buffer か通常入力に流れる。
fn ascii_to_fullwidth_punctuation(c: char) -> Option<&'static str> {
    match c {
        ',' => Some("、"),
        '.' => Some("。"),
        '?' => Some("？"),
        '!' => Some("！"),
        '~' => Some("〜"),
        '(' => Some("（"),
        ')' => Some("）"),
        '[' => Some("「"),
        ']' => Some("」"),
        '{' => Some("『"),
        '}' => Some("』"),
        ';' => Some("；"),
        ':' => Some("："),
        '@' => Some("＠"),
        '#' => Some("＃"),
        '$' => Some("＄"),
        '%' => Some("％"),
        '&' => Some("＆"),
        '*' => Some("＊"),
        '+' => Some("＋"),
        '=' => Some("＝"),
        _ => None,
    }
}

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
                // 自前候補ウィンドウ singleton を初期化 (まだ未生成なら 1 度だけ)。
                let mtm = MainThreadMarker::new()
                    .expect("initWithServer must run on the main thread");
                ensure_custom_panel(mtm);
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

        /// 生 NSEvent を受け取って独自処理する。
        ///
        /// IMK は通常 keyDown を `inputText:`/`didCommandBySelector:` に変換するが、
        /// 矢印キーや「かな」キーなど特殊キーは届かない / 別経路に流れる場合がある
        /// (実機検証 2026-06-10):
        ///
        /// - **← →**: composition + panel 表示中でも `moveLeft:` / `moveRight:` セレクタが
        ///   ログに現れない → ここで直接捕まえて文節フォーカス移動を実装
        /// - **「かな」キー (keyCode 104)**: 押下直後に Space イベントが漏れて
        ///   `inputText: " "` が来る現象あり → ここで keyDown を検知してガード設定
        ///
        /// 戻り値 Bool::YES = 完全に処理した (IMK の以降の処理を停止)
        /// Bool::NO = 通常の IMK 処理に流す
        #[unsafe(method(handleEvent:client:))]
        fn handle_event(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> Bool {
            self._handle_event_impl(event, sender)
        }
    }
);

// --- メソッド実装 ---

/// 「かな」キー押下から Space leak を破棄する時間 (ms)
const KANA_GUARD_MS: u128 = 300;

/// macOS Japanese keyboard の物理キー keyCode
const KEY_CODE_LEFT_ARROW: u16 = 123;
const KEY_CODE_RIGHT_ARROW: u16 = 124;
const KEY_CODE_KANA: u16 = 104;

impl NukoInputController {
    /// handleEvent:client: の実装
    ///
    /// ## 重要: 処理しないキーは必ず super に流すこと
    ///
    /// IMKInputController のデフォルト `handleEvent:` 実装は内部で
    /// `inputText:` / `didCommandBySelector:` を派生させる。
    /// **処理しない場合は `call_super_handle_event` を呼ぶ** 必要がある。
    ///
    /// PR #51 で super 呼出を忘れたため `inputText:` が呼ばれなくなり、
    /// **「日本語が打てない」大デグレ** を引き起こした (2026-06-10 ユーザー報告)。
    fn _handle_event_impl(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> Bool {
        let Some(event) = event else { return Bool::NO };

        let event_type = event.r#type();
        if event_type != NSEventType::KeyDown {
            return self.call_super_handle_event(event, sender);
        }

        let key_code = event.keyCode();
        debug_log(&format!("handleEvent keyDown: keyCode={key_code}"));

        match key_code {
            KEY_CODE_LEFT_ARROW => {
                // segmented モード中のみ反応 (= 単一文節時はホストに矢印移動を任せる)
                let in_segmented = self.ivars().state.borrow().segmented.is_some();
                if !in_segmented {
                    return self.call_super_handle_event(event, sender);
                }
                if let Some(client) = sender {
                    self.handle_segment_focus_shift(client, /*forward=*/ false);
                }
                Bool::YES
            }
            KEY_CODE_RIGHT_ARROW => {
                let in_segmented = self.ivars().state.borrow().segmented.is_some();
                if !in_segmented {
                    return self.call_super_handle_event(event, sender);
                }
                if let Some(client) = sender {
                    self.handle_segment_focus_shift(client, /*forward=*/ true);
                }
                Bool::YES
            }
            KEY_CODE_KANA => {
                // 「かな」キー押下を記録 → 直後の Space leak をガードで破棄
                self.ivars().state.borrow_mut().kana_pressed_at = Some(std::time::Instant::now());
                debug_log("kana key (keyCode 104) detected, setting guard");
                // 入力ソース切替は OS に委ねるため super に流す
                self.call_super_handle_event(event, sender)
            }
            _ => self.call_super_handle_event(event, sender),
        }
    }

    /// `super.handleEvent:client:` を呼ぶ
    ///
    /// IMK のデフォルト実装は内部で `inputText:` / `didCommandBySelector:` を派生
    /// させるので、**処理しないキーはこれを呼ばないとアプリに何も入力されない**。
    fn call_super_handle_event(&self, event: &NSEvent, sender: Option<&AnyObject>) -> Bool {
        unsafe { msg_send![super(self), handleEvent: event, client: sender] }
    }

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
        //   - それ以外 → **Bool::NO でホストにパススルー** (= macOS 標準の半角スペース)
        //
        // 経緯: PR #26 で日本語モード時の Space を U+3000 (全角) 挿入にしていたが
        // (Mozc / Google 日本語入力の慣例に合わせていた)、ユーザーから
        // 「『かな』キーを押すたびに全角スペースが入って不便」「Mac の Space は
        // 本当のスペースの時だけ」(2026-06-09) との指摘を受けて macOS 標準
        // (ことえり相当) の挙動に戻した。全角が欲しい場合は変換中の Space で
        // 候補リストから「　」(全角スペース) を選ぶか、英数 + 全角モード等で。
        //
        // 150ms の活性化ガードは引き続き有効 ("かな" キーが本物の Space と
        // 紛らわしいパスでホストに渡る場合の防御)。
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

            // 「かな」キー押下直後の Space leak も破棄 (2026-06-10 報告)
            if let Some(kana_at) = state.kana_pressed_at {
                if kana_at.elapsed().as_millis() < KANA_GUARD_MS {
                    state.kana_pressed_at = None; // 1 shot で消費
                    debug_log("space: discard (kana key guard, likely kana key leak)");
                    return Bool::YES;
                }
            }

            if state.candidates.is_some() {
                let mut new_idx = None;
                if let Some(ref mut candidates) = state.candidates {
                    candidates.select_next();
                    new_idx = Some(candidates.selected_index());
                    let surface = candidates
                        .selected()
                        .map(|s| s.surface.clone())
                        .unwrap_or_default();
                    debug_log(&format!("space: cycle to next candidate '{surface}'"));
                    drop(state);
                    Self::set_marked_text_on_client(client, &surface);
                }
                // パネル内部の青ハイライトも同期 (IMK の default routing が
                // 効かない環境向けに明示的に呼ぶ)
                if let Some(idx) = new_idx {
                    self.sync_panel_selection(idx);
                }
                return Bool::YES;
            }
            if state.is_composing {
                drop(state);
                self.do_convert(client);
                return Bool::YES;
            }
            // 未確定状態 (= 入力中でない、候補も無い): ホストに任せる。
            // = macOS 標準の半角スペース挿入 (ことえり相当)。
            drop(state);
            debug_log("space: passthrough to host (no composition)");
            return Bool::NO;
        }

        // 数字キー 1-9: 候補表示中なら該当 line の候補を確定する
        // (一般的な日本語 IME の慣例。IMKCandidates のデフォルト selectionKeys と一致)
        //
        // 決定ロジックは純粋関数 `crate::commit::decide_digit_select_and_commit` に
        // 委譲。`unit test` でカバー済み (segmented mode のデータ消失防止含む)。
        if state.candidates.is_some() && text.chars().count() == 1 {
            if let Some(digit_char) = text.chars().next() {
                if let Some(decision) =
                    crate::commit::decide_digit_select_and_commit(&state, digit_char)
                {
                    let line_idx = (digit_char as usize) - ('1' as usize);

                    // 副作用: 状態を decide の決定に合わせて更新
                    if let Some(candidates) = state.candidates.as_mut() {
                        candidates.select(line_idx);
                    }
                    if let Some(segmented) = state.segmented.as_mut() {
                        let focused = segmented.focused;
                        if let Some(seg) = segmented.segments.get_mut(focused) {
                            seg.select(line_idx);
                        }
                    }

                    // 副作用: 学習
                    let ctx_snapshot = state.context.clone();
                    for c in &decision.learn_targets {
                        with_engine_mut(|engine| {
                            let _ = engine.commit(c, &ctx_snapshot);
                        });
                    }
                    state.context.push_prev_word(&decision.commit_text);
                    state.reset();
                    drop(state);
                    Self::hide_candidate_panel();
                    Self::insert_text_on_client(client, &decision.commit_text);
                    debug_log(&format!(
                        "digit-{digit_char}: committed line {line_idx} = '{}'",
                        decision.commit_text
                    ));
                    return Bool::YES;
                }
                // 数字 1-9 だが候補数を超える等 → fallthrough
            }
        }

        // 数字 0-9 は IME 変換対象外。
        //
        // 旧挙動 (バグ): 数字を romaji.input に渡していたため、buffer に "1"
        // が滞留して "1tu" → buffer="1tu" → flush で composition に "1tu" 注入
        // → engine.convert("1tu") で意味不明な変換が出ていた
        // (ユーザー報告 2026-06-07: 「1tu」と打ちたいのに「統治体のに」になる)。
        //
        // 修正: 候補表示なし & 単一の半角数字なら、
        //   - 現在の composition があれば flush + 確定して insert
        //   - そのあと数字を直接 insertText でホストに渡す
        // 一般的な日本語 IME (Google 日本語入力 / ATOK / ことえり) と同じ挙動。
        if state.candidates.is_none() && text.chars().count() == 1 {
            if let Some(ch) = text.chars().next() {
                if ch.is_ascii_digit() {
                    let mut commit_text = String::new();
                    if state.is_composing || !state.romaji.buffer().is_empty() {
                        let remaining = state.romaji.flush();
                        if !remaining.is_empty() {
                            state.composition.push_str(&remaining);
                        }
                        commit_text = state.composition.clone();
                        if !commit_text.is_empty() {
                            state.context.push_prev_word(&commit_text);
                        }
                        state.reset();
                    }
                    drop(state);
                    if !commit_text.is_empty() {
                        Self::insert_text_on_client(client, &commit_text);
                    }
                    Self::insert_text_on_client(client, &text);
                    debug_log(&format!(
                        "digit-passthrough: prev_commit='{commit_text}' digit='{text}'"
                    ));
                    return Bool::YES;
                }

                // 全角記号変換: ASCII 句読点・記号を 全角 に置き換えて挿入。
                // ユーザー報告 (2026-06-10): 「全角記号が打てない」。
                //
                // 一般的な日本語 IME (Google 日本語入力 / ATOK / ことえり) 同様、
                // composition が無い時に「.」を打つと「。」、「?」 → 「？」 等。
                // composition がある場合は flush + commit してから記号挿入。
                if let Some(fullwidth) = ascii_to_fullwidth_punctuation(ch) {
                    let mut commit_text = String::new();
                    if state.is_composing || !state.romaji.buffer().is_empty() {
                        let remaining = state.romaji.flush();
                        if !remaining.is_empty() {
                            state.composition.push_str(&remaining);
                        }
                        commit_text = state.composition.clone();
                        if !commit_text.is_empty() {
                            state.context.push_prev_word(&commit_text);
                        }
                        state.reset();
                    }
                    drop(state);
                    if !commit_text.is_empty() {
                        Self::insert_text_on_client(client, &commit_text);
                    }
                    Self::insert_text_on_client(client, fullwidth);
                    debug_log(&format!(
                        "fullwidth-punct: '{ch}' → '{fullwidth}' (prev_commit='{commit_text}')"
                    ));
                    return Bool::YES;
                }
            }
        }

        // 候補選択中に文字を打ったら確定して新しい入力開始
        //
        // 決定ロジックは純粋関数 `crate::commit::decide_commit` に委譲。
        // segmented モード時の全文連結 + 全文節の個別学習は decide_commit が保証。
        //
        // 旧コード (PR #51) では commit_text は segmented.current_surface() を使っていたが
        // 学習は candidates.selected (= focused 文節だけ) しか記録しない潜在バグがあった。
        // decide_commit 経由にすることで **全 segment の選択候補を個別に学習** するように。
        if state.candidates.is_some() {
            let decision = crate::commit::decide_commit(&state);

            // 副作用: 各 segment (segmented 時) ないし selected (flat 時) を学習
            let ctx_snapshot = state.context.clone();
            for c in &decision.learn_targets {
                with_engine_mut(|engine| {
                    let _ = engine.commit(c, &ctx_snapshot);
                });
            }
            if !decision.commit_text.is_empty() {
                state.context.push_prev_word(&decision.commit_text);
            }

            state.reset();
            drop(state);
            Self::hide_candidate_panel();
            Self::insert_text_on_client(client, &decision.commit_text);

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

        // セレクタ名 + composing 状態 → アクションの決定は純粋関数
        // `crate::commit::decide_command` に委譲 (テスト基盤 #4)。
        // 非 composing 時は必ず PassThrough になる不変条件もそこで保証。
        let sel_name = selector.name();
        let action = crate::commit::decide_command(sel_name, is_composing);

        match action {
            CommandAction::PassThrough => Bool::NO,
            CommandAction::Commit => {
                // Enter: 確定
                self.do_commit(client);
                Bool::YES
            }
            CommandAction::Cancel => {
                // Escape: 取消
                self.do_cancel(client);
                Bool::YES
            }
            CommandAction::Backspace => {
                // Backspace: 削除
                self.do_backspace(client);
                Bool::YES
            }
            CommandAction::FocusShiftLeft => {
                // Left: 文節フォーカスを前へ (segmented モードのみ)
                self.handle_segment_focus_shift(client, /*forward=*/ false)
            }
            CommandAction::FocusShiftRight => {
                // Right: 文節フォーカスを後ろへ (segmented モードのみ)
                self.handle_segment_focus_shift(client, /*forward=*/ true)
            }
            CommandAction::SelectNext => {
                // Down: 次候補
                let mut new_idx = None;
                let mut state = self.ivars().state.borrow_mut();
                if let Some(ref mut candidates) = state.candidates {
                    candidates.select_next();
                    new_idx = Some(candidates.selected_index());
                    if let Some(selected) = candidates.selected() {
                        let surface = selected.surface.clone();
                        drop(state);
                        Self::set_marked_text_on_client(client, &surface);
                    }
                }
                if let Some(idx) = new_idx {
                    self.sync_panel_selection(idx);
                }
                Bool::YES
            }
            CommandAction::SelectPrev => {
                // Up: 前候補
                let mut new_idx = None;
                let mut state = self.ivars().state.borrow_mut();
                if let Some(ref mut candidates) = state.candidates {
                    candidates.select_prev();
                    new_idx = Some(candidates.selected_index());
                    if let Some(selected) = candidates.selected() {
                        let surface = selected.surface.clone();
                        drop(state);
                        Self::set_marked_text_on_client(client, &surface);
                    }
                }
                if let Some(idx) = new_idx {
                    self.sync_panel_selection(idx);
                }
                Bool::YES
            }
            CommandAction::CommitAndPassThrough => {
                debug_log(&format!("unhandled selector: {sel_name:?}"));
                // 未知のセレクタ: 確定してパススルー
                self.do_commit(client);
                Bool::NO
            }
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
    ///
    /// libakaza が利用可能なら `convert_segmented` で文節別の結果を取り、
    /// `state.segmented` に保存。`state.candidates` は **focused 文節の候補リスト**
    /// として用意する (= 既存の Space/↓↑/1-9 ハンドラがそのまま動く)。
    ///
    /// libakaza が無効ないし複数文節を返さない場合は、従来通り `engine.convert`
    /// で flat な候補リストを取得して `state.candidates` のみ設定する。
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

        // 1. 文節別変換を試す (libakaza available + 単一文節超え)
        #[cfg(feature = "akaza")]
        let segmented_result = with_engine(|engine| engine.convert_segmented(&composition));
        #[cfg(not(feature = "akaza"))]
        let segmented_result: nuko_core::error::Result<
            Option<nuko_core::conversion::SegmentedConversion>,
        > = Ok(None);

        if let Ok(Some(segmented)) = segmented_result {
            if segmented.segments.len() >= 2 {
                debug_log(&format!(
                    "do_convert: segmented mode, {} segments",
                    segmented.segments.len()
                ));
                let surface = segmented.current_surface();
                let focused_candidates =
                    Self::candidate_list_from_segment(&segmented, segmented.focused);
                state.segmented = Some(segmented);
                state.candidates = Some(focused_candidates);
                drop(state);
                Self::set_marked_text_on_client(client, &surface);
                self.show_candidate_panel(client);
                return;
            }
            // 単一文節時は従来の flat 経路の方が候補揃いが豊富 (k-best + 静的辞書 + かな variants) なので fall through
        }

        // 2. 従来の flat な変換 (libakaza 無効 / 単一文節 / segmented 失敗時)
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
                    state.segmented = None;
                    state.candidates = Some(candidates);
                    drop(state);
                    Self::set_marked_text_on_client(client, &surface);
                    self.show_candidate_panel(client);
                } else {
                    debug_log("do_convert: no selected candidate, showing composition");
                    let display = state.display_text();
                    state.segmented = None;
                    state.candidates = Some(candidates);
                    drop(state);
                    Self::set_marked_text_on_client(client, &display);
                    self.show_candidate_panel(client);
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

    /// 指定文節の候補を `CandidateList` に変換 (panel 表示と Space cycle 用)
    fn candidate_list_from_segment(
        segmented: &nuko_core::conversion::SegmentedConversion,
        seg_idx: usize,
    ) -> CandidateList {
        let mut list = CandidateList::new();
        if let Some(segment) = segmented.segments.get(seg_idx) {
            for c in &segment.candidates {
                list.push(c.clone());
            }
            list.select(segment.selected);
        }
        list
    }

    /// 確定を実行
    ///
    /// テスト可能な純粋関数 [`crate::commit::decide_commit`] に commit_text と
    /// learn_targets の決定を委譲する設計。本関数は副作用 (= 学習記録、
    /// state.reset、insertText) だけを担う。
    fn do_commit(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();

        // segmented モード: focused segment の selected を state.candidates から sync
        // (decide_commit は state を mut しないので呼び出し前に sync しておく)
        let candidates_sel = state.candidates.as_ref().map(|c| c.selected_index());
        if let Some(segmented) = state.segmented.as_mut() {
            if let Some(sel) = candidates_sel {
                let focused = segmented.focused;
                if let Some(seg) = segmented.segments.get_mut(focused) {
                    seg.select(sel);
                }
            }
        }

        // 未変換のかなだけのときに romaji buffer を flush
        // (decide_commit は state を mut しないので、ここで先に flush しておく)
        if state.segmented.is_none() && state.candidates.is_none() {
            let remaining = state.romaji.flush();
            if !remaining.is_empty() {
                state.composition.push_str(&remaining);
            }
        }

        // 純粋関数で commit 決定 (← unit test でカバーする本丸)
        let decision = crate::commit::decide_commit(&state);

        // 学習記録 (副作用)
        let ctx_snapshot = state.context.clone();
        for c in &decision.learn_targets {
            with_engine_mut(|engine| {
                if let Err(e) = engine.commit(c, &ctx_snapshot) {
                    error!("学習記録エラー: {e}");
                }
            });
        }
        if !decision.commit_text.is_empty() {
            state.context.push_prev_word(&decision.commit_text);
        }

        state.reset();
        drop(state);

        Self::hide_candidate_panel();

        if !decision.commit_text.is_empty() {
            Self::insert_text_on_client(client, &decision.commit_text);
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

        if state.candidates.is_some() || state.segmented.is_some() {
            // 変換結果 (flat / segmented 両方) をクリアして未確定文字列の表示に戻す
            state.candidates = None;
            state.segmented = None;
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

    // --- 候補ウィンドウ (singleton CustomCandidatePanel) ---------------

    /// 候補ウィンドウを表示する (自前 NSPanel)
    ///
    /// 現在の `state.candidates` を CustomPanel に流し、`client` の
    /// `firstRectForCharacterRange:actualRange:` で marked text の screen 座標を
    /// 取得してパネルを直下に配置する。
    fn show_candidate_panel(&self, client: &AnyObject) {
        let snapshot: Option<(Vec<String>, usize)> = {
            let state = self.ivars().state.borrow();
            state.candidates.as_ref().map(|c| {
                let items: Vec<String> = c.iter().map(|x| x.surface.clone()).collect();
                (items, c.selected_index())
            })
        };
        let Some((items, selected)) = snapshot else {
            return;
        };
        let position = Self::caret_screen_point(client);
        debug_log(&format!(
            "show_candidate_panel: items={} selected={selected} pos=({:.1},{:.1})",
            items.len(),
            position.x,
            position.y
        ));
        with_custom_panel(|panel| {
            if let Some(panel) = panel {
                panel.set_candidates(&items, selected);
                let visible = panel.show_at(position);
                debug_log(&format!(
                    "show_candidate_panel: after show_at isVisible={visible}"
                ));
            } else {
                debug_log("show_candidate_panel: panel not initialized");
            }
        });
    }

    /// 候補ウィンドウを隠す (singleton; 全 controller から共有)
    fn hide_candidate_panel() {
        with_custom_panel(|panel| {
            if let Some(panel) = panel {
                if panel.is_visible() {
                    debug_log("hide_candidate_panel");
                    panel.hide();
                }
            }
        });
    }

    /// `state.candidates.selected` の変化をパネル表示に反映する。
    ///
    /// `set_candidates` を呼び直して再描画する (selected も同時に反映)。
    /// state borrow と panel borrow を別スコープに分けて RefCell の二重借用を回避。
    fn sync_panel_selection(&self, _line_number: usize) {
        let snapshot: Option<(Vec<String>, usize)> = {
            let state = self.ivars().state.borrow();
            state.candidates.as_ref().map(|c| {
                let items: Vec<String> = c.iter().map(|x| x.surface.clone()).collect();
                (items, c.selected_index())
            })
        };
        let Some((items, selected)) = snapshot else {
            return;
        };
        debug_log(&format!("sync_panel_selection: selected={selected}"));
        with_custom_panel(|panel| {
            if let Some(panel) = panel {
                if panel.is_visible() {
                    panel.set_candidates(&items, selected);
                }
            }
        });
    }

    /// 文節フォーカスを前後に動かす (segmented モードのみ)。
    ///
    /// `forward = true` で次の文節、`false` で前の文節へ。
    /// 現在の focused の selected を保存してから focus を動かし、
    /// 新しい focused の candidates を `state.candidates` に load する。
    /// marked text は全文節の current_surface() で更新、panel は新文節の候補で再描画。
    ///
    /// segmented モードでない (= 単一文節 or libakaza fail) 場合は Bool::YES を
    /// 返してイベントを消費するだけ (= 何もしない、host にも渡さない)。
    /// これは未確定中に host のカーソル移動を呼ばないため。
    fn handle_segment_focus_shift(&self, client: &AnyObject, forward: bool) -> Bool {
        let mut new_focused: Option<usize> = None;
        let mut new_surface: Option<String> = None;
        let mut new_candidates: Option<CandidateList> = None;

        {
            let mut state = self.ivars().state.borrow_mut();
            // 1. 現在の focused に candidates の selected を sync back
            //    (borrow checker 回避のため、まず candidates から index を取り出してから segmented を mut 借用)
            let candidates_sel = state.candidates.as_ref().map(|c| c.selected_index());
            if let Some(segmented) = state.segmented.as_mut() {
                if let Some(sel) = candidates_sel {
                    let focused = segmented.focused;
                    if let Some(seg) = segmented.segments.get_mut(focused) {
                        seg.select(sel);
                    }
                }
                // 2. focus を動かす (wrap)
                if forward {
                    segmented.focus_next();
                } else {
                    segmented.focus_prev();
                }
                new_focused = Some(segmented.focused);
                new_surface = Some(segmented.current_surface());
                new_candidates = Some(Self::candidate_list_from_segment(
                    segmented,
                    segmented.focused,
                ));
            }
            if let Some(list) = new_candidates.take() {
                state.candidates = Some(list);
            }
        }

        if let (Some(focused), Some(surface)) = (new_focused, new_surface) {
            debug_log(&format!(
                "handle_segment_focus_shift: forward={forward} new_focused={focused}"
            ));
            Self::set_marked_text_on_client(client, &surface);
            // panel を新文節の候補で再描画
            self.show_candidate_panel(client);
        }

        Bool::YES
    }

    /// パネル表示位置を返す (マルチスクリーン対応、マウス近くに配置)。
    ///
    /// ## 経緯
    ///
    /// - PR #34 / #36: `firstRectForCharacterRange:` 経由のカーソル取得 →
    ///   実機で `rect=(0,0,0,0)` を返す client が多く失敗
    /// - PR #37: 常に `NSScreen::mainScreen()` 中央 → マルチスクリーンで
    ///   user が見ていない screen に panel が出る可能性 (ユーザー指摘)
    /// - 本 PR (PR #38, v3.3): **マウスカーソルがある screen を特定して、
    ///   マウス位置の少し下にパネルを置く**
    ///
    /// `NSEvent::mouseLocation()` は **window server から見た global 座標** を返す。
    /// 全 NSScreen をスキャンして、frame に mouseLocation が含まれる screen を
    /// マウスのある screen と判定する。
    ///
    /// `client` 引数は将来の caret 直接追従用に残す (現状は未使用)。
    fn caret_screen_point(_client: &AnyObject) -> NSPoint {
        let mtm =
            MainThreadMarker::new().expect("controller callbacks must run on the main thread");
        let mouse = NSEvent::mouseLocation();
        debug_log(&format!(
            "caret_screen_point: mouseLocation=({:.1},{:.1})",
            mouse.x, mouse.y
        ));

        // 全 screen を走査してマウスのある screen を見つける
        let screens = NSScreen::screens(mtm);
        let target_screen = (0..screens.count())
            .find_map(|i| {
                let screen = screens.objectAtIndex(i);
                let frame = screen.frame();
                if mouse.x >= frame.origin.x
                    && mouse.x < frame.origin.x + frame.size.width
                    && mouse.y >= frame.origin.y
                    && mouse.y < frame.origin.y + frame.size.height
                {
                    Some(screen)
                } else {
                    None
                }
            })
            .or_else(|| NSScreen::mainScreen(mtm));

        let Some(screen) = target_screen else {
            debug_log("caret_screen_point: no screen found, using (400, 400)");
            return NSPoint::new(400.0, 400.0);
        };

        let visible = screen.visibleFrame();
        debug_log(&format!(
            "caret_screen_point: target screen visibleFrame origin=({:.1},{:.1}) size=({:.1},{:.1})",
            visible.origin.x, visible.origin.y, visible.size.width, visible.size.height
        ));

        // パネル top-left をマウス位置の **下** に置く。
        // setFrameTopLeftPoint は y-up 座標 (= y が上方向) なので、
        // マウスの y より小さい y を渡せばマウスの「下」になる。
        // マウス直下 (= y を 4px 下げる) で目障りでない程度に近く。
        let mut x = mouse.x;
        let mut y = mouse.y - 4.0;

        // 画面端でクランプ (panel 幅 280, 想定高さ 220 程度)
        const PANEL_WIDTH: f64 = 280.0;
        const PANEL_HEIGHT_ESTIMATE: f64 = 220.0;
        if x + PANEL_WIDTH > visible.origin.x + visible.size.width {
            x = visible.origin.x + visible.size.width - PANEL_WIDTH;
        }
        if x < visible.origin.x {
            x = visible.origin.x;
        }
        // y は top-left。y - panel_height が画面下 (= visible.origin.y) より
        // 下にならないようにクランプ
        if y - PANEL_HEIGHT_ESTIMATE < visible.origin.y {
            y = visible.origin.y + PANEL_HEIGHT_ESTIMATE;
        }
        // 画面上 (= visible.origin.y + visible.size.height) より上にも行かないよう
        if y > visible.origin.y + visible.size.height {
            y = visible.origin.y + visible.size.height;
        }

        debug_log(&format!("caret_screen_point: final pos=({x:.1},{y:.1})"));
        NSPoint::new(x, y)
    }
}
