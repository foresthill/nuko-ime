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
use objc2_app_kit::{NSEvent, NSScreen};
use objc2_foundation::{NSArray, NSPoint, NSRange, NSString};
use objc2_input_method_kit::{IMKInputController, IMKServer};
use tracing::{debug, error, info, warn};

use nuko_core::conversion::CandidateList;

use crate::commit::{BackspaceAction, CommandAction, SpaceAction};
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

        // 注意: `handleEvent:client:` は **実装しない**。
        //
        // IMKServerInput の入力受信は 3 方式があり「1 つだけ」を選ぶ排他設計
        // (Apple 公式: "An input method should choose one of those ways")。
        // 本 IME は方式 1 = `inputText:client:` + `didCommandBySelector:client:`
        // を採用している。ここに方式 3 の `handleEvent:client:` を被せると IMK は
        // inputText: を呼ばなくなり日本語が打てなくなる (PR #51 の大デグレ)。
        // さらに `IMKInputController` は `handleEvent:client:` の実装を持たないため
        // `super` 呼び出しは "method not found" で abort する (PR #52 のクラッシュ)。
        // 矢印キーは方式 1 の `didCommandBySelector:` (moveLeft:/moveRight:) で受ける。
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
            // Space キーの分岐決定は純粋関数 `crate::commit::decide_space_action`
            // に委譲 (テスト基盤 #5)。経過時間の計測だけここで行い、閾値比較を
            // 含む判定ロジックは純粋関数側に集約する。
            let activation_elapsed_ms = state.activated_at.map(|t| t.elapsed().as_millis());
            let kana_elapsed_ms = state.kana_pressed_at.map(|t| t.elapsed().as_millis());
            let action = crate::commit::decide_space_action(
                state.candidates.is_some(),
                state.is_composing,
                activation_elapsed_ms,
                kana_elapsed_ms,
            );

            match action {
                SpaceAction::DiscardActivationGuard => {
                    // 活性化直後の Space は破棄 (ソース切替の漏れ対策)
                    state.activated_at = None; // 1 shot で消費
                    debug_log("space: discard (activation guard, likely source-switch leak)");
                    return Bool::YES;
                }
                SpaceAction::DiscardKanaGuard => {
                    // 「かな」キー押下直後の Space leak も破棄 (2026-06-10 報告)
                    state.kana_pressed_at = None; // 1 shot で消費
                    debug_log("space: discard (kana key guard, likely kana key leak)");
                    return Bool::YES;
                }
                SpaceAction::CycleNextCandidate => {
                    let mut new_idx = None;
                    if let Some(ref mut candidates) = state.candidates {
                        candidates.select_next();
                        new_idx = Some(candidates.selected_index());
                    }
                    drop(state);
                    // segmented モードでは全文表示 (他文節を消さない)。
                    self.refresh_marked_after_selection(client);
                    // パネル内部の青ハイライトも同期 (IMK の default routing が
                    // 効かない環境向けに明示的に呼ぶ)
                    if let Some(idx) = new_idx {
                        self.sync_panel_selection(idx);
                    }
                    return Bool::YES;
                }
                SpaceAction::Convert => {
                    drop(state);
                    self.do_convert(client);
                    return Bool::YES;
                }
                SpaceAction::PassThrough => {
                    // 未確定状態 (= 入力中でない、候補も無い): ホストに任せる。
                    // = macOS 標準の半角スペース挿入 (ことえり相当)。
                    drop(state);
                    debug_log("space: passthrough to host (no composition)");
                    return Bool::NO;
                }
            }
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
                {
                    let mut state = self.ivars().state.borrow_mut();
                    if let Some(ref mut candidates) = state.candidates {
                        candidates.select_next();
                        new_idx = Some(candidates.selected_index());
                    }
                }
                // segmented モードでは全文表示 (他文節を消さない)。
                self.refresh_marked_after_selection(client);
                if let Some(idx) = new_idx {
                    self.sync_panel_selection(idx);
                }
                Bool::YES
            }
            CommandAction::SelectPrev => {
                // Up: 前候補
                let mut new_idx = None;
                {
                    let mut state = self.ivars().state.borrow_mut();
                    if let Some(ref mut candidates) = state.candidates {
                        candidates.select_prev();
                        new_idx = Some(candidates.selected_index());
                    }
                }
                // segmented モードでは全文表示 (他文節を消さない)。
                self.refresh_marked_after_selection(client);
                if let Some(idx) = new_idx {
                    self.sync_panel_selection(idx);
                }
                Bool::YES
            }
            CommandAction::ResizeSegmentLeft => {
                // Shift+Left: focused 文節を縮める (segmented モードのみ)
                self.handle_segment_resize(client, /*extend_right=*/ false)
            }
            CommandAction::ResizeSegmentRight => {
                // Shift+Right: focused 文節を伸ばす (segmented モードのみ)
                self.handle_segment_resize(client, /*extend_right=*/ true)
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
        let text_len = text.encode_utf16().count();
        // カーソルを末尾に置く (= フォーカス文節という概念がない flat 表示)
        Self::set_marked_text_with_selection(client, text, NSRange::new(text_len, 0));
    }

    /// segmented モード用: フォーカス文節を `selectionRange` で示して描画する。
    ///
    /// `focus_start` / `focus_len` は marked text 内の UTF-16 範囲
    /// ([`SegmentedConversion::focused_surface_range_utf16`])。多くのアプリは
    /// この範囲を太線/ハイライトで描き、「今どの文節を編集中か」が分かる。
    fn set_marked_text_focused(
        client: &AnyObject,
        text: &str,
        focus_start: usize,
        focus_len: usize,
    ) {
        Self::set_marked_text_with_selection(client, text, NSRange::new(focus_start, focus_len));
    }

    /// 候補選択 (Space / ↑↓) が変わった後に marked text を更新する。
    ///
    /// **日本語 IME の大原則: 変換中の他文節はそのまま、変換対象の文節のみ変わる。**
    /// そのため segmented モードでは、選択を focused 文節に反映した上で
    /// **全文節を連結した文** を表示する (focused をハイライト)。
    /// 選択候補の surface 単体を表示すると他の文節が画面から消えてしまう
    /// (実機バグ報告 2026-06)。flat モードでは選択候補をそのまま表示する。
    fn refresh_marked_after_selection(&self, client: &AnyObject) {
        let mut state = self.ivars().state.borrow_mut();
        let Some(sel_idx) = state.candidates.as_ref().map(CandidateList::selected_index) else {
            return;
        };

        if let Some(segmented) = state.segmented.as_mut() {
            // segmented: focused 文節に選択を反映 → 全文表示 (focused をハイライト)
            if let Some(seg) = segmented.focused_segment_mut() {
                seg.select(sel_idx);
            }
            let surface = segmented.current_surface();
            let (start, len) = segmented.focused_surface_range_utf16();
            drop(state);
            Self::set_marked_text_focused(client, &surface, start, len);
        } else if let Some(surface) = state
            .candidates
            .as_ref()
            .and_then(CandidateList::selected)
            .map(|s| s.surface.clone())
        {
            // flat: 選択候補そのまま
            drop(state);
            Self::set_marked_text_on_client(client, &surface);
        }
    }

    /// `setMarkedText:selectionRange:replacementRange:` の共通ラッパ。
    fn set_marked_text_with_selection(client: &AnyObject, text: &str, sel_range: NSRange) {
        let ns_string = NSString::from_str(text);
        let rep_range = NSRange::new(NS_NOT_FOUND, 0);
        debug_log(&format!(
            "setMarkedText: '{text}' sel=({},{})",
            sel_range.location, sel_range.length
        ));
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
                let (focus_start, focus_len) = segmented.focused_surface_range_utf16();
                let focused_candidates =
                    Self::candidate_list_from_segment(&segmented, segmented.focused);
                state.segmented = Some(segmented);
                state.candidates = Some(focused_candidates);
                drop(state);
                Self::set_marked_text_focused(client, &surface, focus_start, focus_len);
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

        // 削除対象の決定は純粋関数 `crate::commit::decide_backspace` に委譲
        // (テスト基盤 #6)。実際の mutation と描画はここで行う。
        let action = crate::commit::decide_backspace(
            state.candidates.is_some() || state.segmented.is_some(),
            state.romaji.buffer().is_empty(),
            state.composition.chars().count(),
        );

        match action {
            BackspaceAction::ClearConversion => {
                // 変換結果 (flat / segmented 両方) をクリアして未確定表示に戻す
                state.candidates = None;
                state.segmented = None;
                let display = state.display_text();
                drop(state);
                Self::hide_candidate_panel();
                Self::set_marked_text_on_client(client, &display);
            }
            BackspaceAction::ClearRomajiEndComposing => {
                state.romaji.clear();
                state.is_composing = false;
                drop(state);
                Self::insert_text_on_client(client, "");
            }
            BackspaceAction::ClearRomajiRedisplay => {
                state.romaji.clear();
                let display = state.display_text();
                drop(state);
                Self::set_marked_text_on_client(client, &display);
            }
            BackspaceAction::PopCompositionEndComposing => {
                state.composition.pop();
                state.is_composing = false;
                drop(state);
                Self::insert_text_on_client(client, "");
            }
            BackspaceAction::PopCompositionRedisplay => {
                state.composition.pop();
                let display = state.display_text();
                drop(state);
                Self::set_marked_text_on_client(client, &display);
            }
            BackspaceAction::EndComposing => {
                state.is_composing = false;
                drop(state);
                Self::insert_text_on_client(client, "");
            }
        }
    }

    // --- 候補ウィンドウ (singleton CustomCandidatePanel) ---------------

    /// 候補ウィンドウを表示する (自前 NSPanel)
    ///
    /// 現在の `state.candidates` を CustomPanel に流し、`client` の
    /// `firstRectForCharacterRange:actualRange:` で marked text の screen 座標を
    /// 取得してパネルを直下に配置する。
    fn show_candidate_panel(&self, client: &AnyObject) {
        let snapshot = {
            let state = self.ivars().state.borrow();
            state.candidates.as_ref().map(|c| {
                let items: Vec<String> = c.iter().map(|x| x.surface.clone()).collect();
                (items, c.selected_index(), Self::panel_segments(&state))
            })
        };
        let Some((items, selected, seg_info)) = snapshot else {
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
                if let Some((segs, focused)) = &seg_info {
                    panel.set_candidates_segmented(&items, selected, segs, *focused);
                } else {
                    panel.set_candidates(&items, selected);
                }
                let visible = panel.show_at(position);
                debug_log(&format!(
                    "show_candidate_panel: after show_at isVisible={visible}"
                ));
            } else {
                debug_log("show_candidate_panel: panel not initialized");
            }
        });
    }

    /// segmented モードのとき、パネルヘッダ用に全文節の現在 surface と focused を返す。
    /// 単一文節 (segment 数 1) は分割表示の意味がないので `None` (= ヘッダ無し)。
    fn panel_segments(state: &InputState) -> Option<(Vec<String>, usize)> {
        let segmented = state.segmented.as_ref()?;
        if segmented.segments.len() < 2 {
            return None;
        }
        let segs: Vec<String> = segmented
            .segments
            .iter()
            .map(|s| s.surface().unwrap_or_default().to_string())
            .collect();
        Some((segs, segmented.focused))
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
        let snapshot = {
            let state = self.ivars().state.borrow();
            state.candidates.as_ref().map(|c| {
                let items: Vec<String> = c.iter().map(|x| x.surface.clone()).collect();
                (items, c.selected_index(), Self::panel_segments(&state))
            })
        };
        let Some((items, selected, seg_info)) = snapshot else {
            return;
        };
        debug_log(&format!("sync_panel_selection: selected={selected}"));
        with_custom_panel(|panel| {
            if let Some(panel) = panel {
                if panel.is_visible() {
                    if let Some((segs, focused)) = &seg_info {
                        panel.set_candidates_segmented(&items, selected, segs, *focused);
                    } else {
                        panel.set_candidates(&items, selected);
                    }
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
        let mut new_focus_range: Option<(usize, usize)> = None;

        {
            let mut state = self.ivars().state.borrow_mut();
            // sync back + focus 移動の判定は純粋関数
            // `crate::commit::apply_segment_focus_shift` に委譲 (テスト基盤 #7)。
            // (borrow checker 回避のため、まず candidates から index を取り出してから segmented を mut 借用)
            let candidates_sel = state.candidates.as_ref().map(|c| c.selected_index());
            if let Some(segmented) = state.segmented.as_mut() {
                let (focused, surface) =
                    crate::commit::apply_segment_focus_shift(segmented, candidates_sel, forward);
                new_focused = Some(focused);
                new_surface = Some(surface);
                new_focus_range = Some(segmented.focused_surface_range_utf16());
                new_candidates = Some(Self::candidate_list_from_segment(segmented, focused));
            }
            if let Some(list) = new_candidates.take() {
                state.candidates = Some(list);
            }
        }

        if let (Some(focused), Some(surface), Some((start, len))) =
            (new_focused, new_surface, new_focus_range)
        {
            debug_log(&format!(
                "handle_segment_focus_shift: forward={forward} new_focused={focused}"
            ));
            Self::set_marked_text_focused(client, &surface, start, len);
            // panel を新文節の候補で再描画
            self.show_candidate_panel(client);
        }

        Bool::YES
    }

    /// focused 文節を伸縮する (Shift+→ で伸長 / Shift+← で縮小、segmented モードのみ)。
    ///
    /// libakaza に `force_ranges` で再変換させ、新しい `SegmentedConversion` に
    /// 差し替えて marked text と panel を更新する。
    ///
    /// segmented でない / これ以上伸縮できない / akaza 無効の場合は何もせず
    /// `Bool::YES` で消費する (未確定中に host の選択範囲拡張を呼ばないため)。
    fn handle_segment_resize(&self, client: &AnyObject, extend_right: bool) -> Bool {
        // 1. 現在の segmented を clone で取り出す (engine 呼び出し中に state を借りないため)
        let segmented = {
            let state = self.ivars().state.borrow();
            state.segmented.clone()
        };
        let Some(segmented) = segmented else {
            // segmented でない (単一文節 / flat) → no-op 消費
            return Bool::YES;
        };

        // 2. エンジンで伸縮再変換 (akaza 有効時のみ実効)
        #[cfg(feature = "akaza")]
        let resized = with_engine(|engine| engine.resize_segment(&segmented, extend_right));
        #[cfg(not(feature = "akaza"))]
        let resized: nuko_core::error::Result<
            Option<nuko_core::conversion::SegmentedConversion>,
        > = {
            let _ = (&segmented, extend_right);
            Ok(None)
        };

        let new_seg = match resized {
            Ok(Some(s)) => s,
            Ok(None) => return Bool::YES, // 伸縮不可: 消費して no-op
            Err(e) => {
                debug_log(&format!("handle_segment_resize: error {e}"));
                return Bool::YES;
            }
        };

        // 3. state 差し替え + UI 更新
        let surface = new_seg.current_surface();
        let focused = new_seg.focused;
        let (focus_start, focus_len) = new_seg.focused_surface_range_utf16();
        let focused_candidates = Self::candidate_list_from_segment(&new_seg, focused);
        {
            let mut state = self.ivars().state.borrow_mut();
            state.segmented = Some(new_seg);
            state.candidates = Some(focused_candidates);
        }
        debug_log(&format!(
            "handle_segment_resize: extend_right={extend_right} focused={focused} surface='{surface}'"
        ));
        Self::set_marked_text_focused(client, &surface, focus_start, focus_len);
        self.show_candidate_panel(client);
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
