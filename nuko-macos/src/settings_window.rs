//! ぬこIME 設定ウィンドウ (BYOK 鍵/モデル/プロバイダ + 単語登録)。
//!
//! # IME 窓のフォーカス問題と対策
//! IME は通常 background agent なので、窓を出しても「アクティブなアプリ」にならず
//! **テキスト欄がキーボード入力を受け取れない** (フォーカス枠は出るのに打てない)。
//! そこで表示中だけ `setActivationPolicy(Regular)` で一時的に通常アプリ化して
//! `activate` し、閉じたら元の policy に戻す ([`SettingsWindow::show`] /
//! [`SettingsWindow::close`])。
//!
//! 2 セクション: 【BYOK】(provider/key/model) と 【単語登録】(読み/表層 → 即反映)。

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSComboBox, NSMenu,
    NSMenuItem, NSPopUpButton, NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

// 設定窓ボタンの **安定した** target。
//
// なぜ必要か: ボタンの target を IME controller にすると、設定窓にフォーカスが移った
// 際に IMK がその controller を破棄し、target が nil になってボタン (保存/閉じる) が
// 効かなくなる (2026-10 実機で「閉じるできない」)。そこで controller でなく、
// アプリ全体で 1 つだけ生成して保持し続ける本オブジェクトを target にする。
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "NukoSettingsTarget"]
    #[thread_kind = MainThreadOnly]
    struct SettingsTarget;

    unsafe impl NSObjectProtocol for SettingsTarget {}

    // 窓が閉じたら (✕ / 閉じる どちらでも) IME を background agent (.Accessory) に戻す
    // = dock アイコンを消す。
    unsafe impl NSWindowDelegate for SettingsTarget {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: Option<&AnyObject>) {
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm)
                    .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            }
        }
    }

    impl SettingsTarget {
        /// 「BYOK を保存」。
        #[unsafe(method(nukoSaveSettings:))]
        fn save(&self, _s: Option<&AnyObject>) {
            crate::state::save_settings();
        }
        /// 「辞書に登録」。
        #[unsafe(method(nukoAddDictEntry:))]
        fn add_dict(&self, _s: Option<&AnyObject>) {
            crate::state::add_dict_entry_from_window();
        }
        /// 「この学習を忘れる」(誤学習を観察ログから除去して即反映)。
        #[unsafe(method(nukoForgetLearning:))]
        fn forget_learning(&self, _s: Option<&AnyObject>) {
            crate::state::forget_learning_from_window();
        }
        /// プロバイダ選択が変わったら「現在のキー」表示をそのプロバイダのものに更新。
        #[unsafe(method(nukoProviderChanged:))]
        fn provider_changed(&self, _s: Option<&AnyObject>) {
            crate::state::refresh_key_display();
        }
        /// 「モデル取得」。実 API からモデル一覧を取ってコンボボックスに流し込む。
        #[unsafe(method(nukoFetchModels:))]
        fn fetch_models(&self, _s: Option<&AnyObject>) {
            crate::state::fetch_models();
        }
        /// 「AI接続テスト」。選んだ provider/model で実際に AI へ ping し応答を確認する。
        #[unsafe(method(nukoAiTest:))]
        fn ai_test(&self, _s: Option<&AnyObject>) {
            crate::state::ai_test();
        }
        /// 接続ステータスの🔄更新。/v1/models への軽量疎通で 🟢/🔴 を出し直す (課金ゼロ)。
        #[unsafe(method(nukoRefreshAiStatus:))]
        fn refresh_ai_status(&self, _s: Option<&AnyObject>) {
            crate::state::check_ai_status();
        }
        /// 「閉じる」(窓を隠して activation policy を戻す)。
        #[unsafe(method(nukoCloseSettings:))]
        fn close_action(&self, _s: Option<&AnyObject>) {
            if let Some(mtm) = MainThreadMarker::new() {
                crate::state::close_settings(mtm);
            }
        }
    }
);

impl SettingsTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

// OpenRouter の API キー (sk-or-v1-… 約73文字) が 1 行で全部見える幅
// (狭いとスクロールして一部しか見えず「気持ち悪い」とユーザー報告 2026-10)。
const W: f64 = 720.0;
const H: f64 = 530.0;
const PAD: f64 = 20.0;
const FIELD_W: f64 = W - PAD * 2.0;

/// ぬこIME 設定ウィンドウ (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct SettingsWindow {
    window: Retained<NSWindow>,
    /// プロバイダ選択 (ドロップダウン)。0=Anthropic / 1=OpenRouter / 2=Ollama。
    provider_popup: Retained<NSPopUpButton>,
    /// 最上部の「いま AI学習で使っている provider/model」行 (開くたびに ai.toml から更新)。
    active_label: Retained<NSTextField>,
    /// AI 接続ステータス行 (🟢接続OK / 🔴未接続)。開いた時と🔄更新で /v1/models 疎通を反映。
    /// SSH トンネルが落ちていると dreaming 実行時に繋がらないので、事前に一目で分かるように。
    conn_label: Retained<NSTextField>,
    /// Ollama のベース URL 入力欄 (ollama 選択時のみ ai.toml の ollama_base_url に保存)。
    ollama_base_url_field: Retained<NSTextField>,
    // NSSecureTextField だと secure input mode が IME プロセスで効かず入力不能になる
    // (2026-10 実機)。平文の NSTextField にして確実に打てるようにする (保存後にクリア)。
    key_field: Retained<NSTextField>,
    /// 選択プロバイダに保存済みのキーを伏せ字で示す読み取り専用ラベル。
    current_key_label: Retained<NSTextField>,
    /// モデル選択 (実 API から取得した一覧 + 自由入力可の NSComboBox)。
    model_combo: Retained<NSComboBox>,
    reading_field: Retained<NSTextField>,
    surface_field: Retained<NSTextField>,
    status: Retained<NSTextField>,
    /// 表示前の activation policy (閉じたら戻す)。
    prev_policy: Cell<Option<NSApplicationActivationPolicy>>,
    /// ボタンの安定した target (controller 破棄に影響されないよう保持し続ける)。
    _target: Retained<SettingsTarget>,
}

/// ラベル (非編集・枠なし・背景なしの NSTextField) を作る。
fn make_label(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = NSTextField::new(mtm);
    label.setFrame(frame);
    label.setStringValue(&NSString::from_str(text));
    label.setEditable(false);
    label.setSelectable(false);
    label.setBezeled(false);
    label.setDrawsBackground(false);
    label
}

/// 左寄せ 1 行の矩形。
fn row(y: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(PAD, y), NSSize::new(FIELD_W, h))
}

/// 単一行の編集可能テキスト欄を作る (折り返さず横スクロール。長い API キーが見やすい)。
fn editable_field(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSTextField> {
    let f = NSTextField::new(mtm);
    f.setFrame(frame);
    f.setUsesSingleLineMode(true);
    f
}

impl SettingsWindow {
    /// ウィンドウとフィールドを生成する (非表示)。ボタンの target は内部で生成する
    /// 安定オブジェクト ([`SettingsTarget`])。controller には依存しない。
    pub fn new(mtm: MainThreadMarker) -> Self {
        let target = SettingsTarget::new(mtm);
        let save_target: &AnyObject = &target;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(W, H));
        // Closable (赤ボタン) を付ける = 緊急の閉じ手段。通常は「閉じる」ボタンで policy も戻す。
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let window: Retained<NSWindow> = unsafe {
            let a = NSWindow::alloc(mtm);
            msg_send![
                a,
                initWithContentRect: frame,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        // 赤ボタンで閉じても window オブジェクトを破棄しない (singleton 保持のため)。
        unsafe { window.setReleasedWhenClosed(false) };
        // 閉じたら policy を戻す delegate を設定 (✕ / 閉じる / performClose どれでも効く)。
        window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        window.setTitle(&NSString::from_str("ぬこIME 設定"));

        let content: Retained<NSView> = NSView::initWithFrame(NSView::alloc(mtm), frame);

        // 最上部: いま AI学習(dreaming)で使っている provider/model を示す行 (左)。
        // 開くたびに ai.toml から [`SettingsWindow::set_active_line`] で更新する。
        let active_label = make_label(
            mtm,
            "AI学習: —",
            NSRect::new(NSPoint::new(PAD, 502.0), NSSize::new(380.0, 18.0)),
        );
        // 同じ行の右寄り: AI 接続ステータス (🟢/🔴)。右端に🔄更新ボタン。
        let conn_label = make_label(
            mtm,
            "接続: —",
            NSRect::new(NSPoint::new(PAD + 390.0, 502.0), NSSize::new(190.0, 18.0)),
        );
        let refresh_status = NSButton::new(mtm);
        refresh_status.setTitle(&NSString::from_str("🔄 更新"));
        refresh_status.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 90.0, 498.0),
            NSSize::new(90.0, 24.0),
        ));
        unsafe {
            refresh_status.setTarget(Some(save_target));
            refresh_status.setAction(Some(sel!(nukoRefreshAiStatus:)));
        }

        // ── BYOK セクション ──
        let byok_h = make_label(mtm, "【 BYOK（AI学習の鍵）】", row(476.0, 18.0));
        // プロバイダ選択ドロップダウン (0=Anthropic / 1=OpenRouter / 2=Ollama)。
        // チェックボックスより「それぞれに鍵を持てる」ことが直感的 (ユーザー要望 2026-10)。
        let provider_label = make_label(mtm, "プロバイダ:", row(454.0, 16.0));
        let provider_popup = {
            let p = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                NSRect::new(NSPoint::new(PAD + 90.0, 450.0), NSSize::new(280.0, 26.0)),
                false,
            );
            p.addItemWithTitle(&NSString::from_str("Anthropic（claude 直）"));
            p.addItemWithTitle(&NSString::from_str("OpenRouter"));
            p.addItemWithTitle(&NSString::from_str("Ollama（ローカル/自社）"));
            unsafe {
                p.setTarget(Some(save_target));
                p.setAction(Some(sel!(nukoProviderChanged:)));
            }
            p
        };
        // Ollama のベース URL (OpenAI 互換。既定 http://localhost:11434/v1、
        // 自社サーバーは SSH トンネル後の http://localhost:11435/v1 等)。
        // 他プロバイダ選択時は無視される (保存は ollama 選択時のみ ai.toml に反映)。
        let base_url_label = make_label(
            mtm,
            "Ollama ベースURL（例 http://localhost:11435/v1。既定は 11434）:",
            row(424.0, 16.0),
        );
        let ollama_base_url_field = editable_field(mtm, row(400.0, 24.0));
        let key_label = make_label(mtm, "API キー（平文入力・保存後クリア）:", row(374.0, 16.0));
        let key_field = editable_field(mtm, row(350.0, 24.0));
        // 選択プロバイダに保存済みのキーを伏せ字で常時表示 (保存後も残るので「空に見える」を解消)。
        let current_key_label = make_label(mtm, "現在のキー: —", row(324.0, 16.0));
        let model_label = make_label(
            mtm,
            "モデル（「モデル取得」で最新一覧を取得。入力して絞り込み可）:",
            row(300.0, 16.0),
        );
        // モデルは実 API から取得した一覧 + 自由入力の NSComboBox (ID 推測を避ける)。
        let model_combo = NSComboBox::new(mtm);
        model_combo.setFrame(NSRect::new(
            NSPoint::new(PAD, 276.0),
            NSSize::new(FIELD_W - 140.0, 26.0),
        ));
        model_combo.setUsesSingleLineMode(true);
        model_combo.setCompletes(true); // 入力途中で候補補完
        let fetch_models = NSButton::new(mtm);
        fetch_models.setTitle(&NSString::from_str("モデル取得"));
        fetch_models.setFrame(NSRect::new(
            NSPoint::new(PAD + FIELD_W - 130.0, 275.0),
            NSSize::new(130.0, 28.0),
        ));
        unsafe {
            fetch_models.setTarget(Some(save_target));
            fetch_models.setAction(Some(sel!(nukoFetchModels:)));
        }
        let byok_save = NSButton::new(mtm);
        byok_save.setTitle(&NSString::from_str("BYOK を保存"));
        byok_save.setFrame(NSRect::new(
            NSPoint::new(PAD, 244.0),
            NSSize::new(150.0, 30.0),
        ));
        unsafe {
            byok_save.setTarget(Some(save_target));
            byok_save.setAction(Some(sel!(nukoSaveSettings:)));
        }
        // 「AI接続テスト」: 選んだ provider/model で実際に推論が返るか 1 往復で確認。
        // モデル取得 (一覧が引けるか) より強い確認。保存の右隣に置く。
        let ai_test = NSButton::new(mtm);
        ai_test.setTitle(&NSString::from_str("AI接続テスト"));
        ai_test.setFrame(NSRect::new(
            NSPoint::new(PAD + 160.0, 244.0),
            NSSize::new(160.0, 30.0),
        ));
        unsafe {
            ai_test.setTarget(Some(save_target));
            ai_test.setAction(Some(sel!(nukoAiTest:)));
        }

        // ── 単語登録 / 学習の編集 セクション ──
        let dict_h = make_label(
            mtm,
            "【 単語登録 / 学習の編集（読み → 表層）】",
            row(222.0, 18.0),
        );
        let reading_label = make_label(mtm, "読み（かな）:", row(198.0, 16.0));
        let reading_field = editable_field(mtm, row(172.0, 24.0));
        let surface_label = make_label(
            mtm,
            "表層（変換後の語。「忘れる」では空＝その読みを全部忘れる）:",
            row(144.0, 16.0),
        );
        let surface_field = editable_field(mtm, row(118.0, 24.0));
        let dict_add = NSButton::new(mtm);
        dict_add.setTitle(&NSString::from_str("辞書に登録"));
        dict_add.setFrame(NSRect::new(
            NSPoint::new(PAD, 82.0),
            NSSize::new(150.0, 30.0),
        ));
        unsafe {
            dict_add.setTarget(Some(save_target));
            dict_add.setAction(Some(sel!(nukoAddDictEntry:)));
        }
        // 誤学習を忘れる (読み必須・表層任意)。辞書登録の右隣に置く。
        let forget = NSButton::new(mtm);
        forget.setTitle(&NSString::from_str("この学習を忘れる"));
        forget.setFrame(NSRect::new(
            NSPoint::new(PAD + 160.0, 82.0),
            NSSize::new(200.0, 30.0),
        ));
        unsafe {
            forget.setTarget(Some(save_target));
            forget.setAction(Some(sel!(nukoForgetLearning:)));
        }

        let close = NSButton::new(mtm);
        close.setTitle(&NSString::from_str("閉じる"));
        close.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 100.0, 82.0),
            NSSize::new(100.0, 30.0),
        ));
        unsafe {
            // window 自身の performClose: を呼ぶ = target 破棄やアクティブ状態に依存しない
            // 確実な閉じ (custom action target が効かなくなる問題の回避 2026-10)。
            let win_any: &AnyObject = &window;
            close.setTarget(Some(win_any));
            close.setAction(Some(sel!(performClose:)));
        }

        // ステータスは長文 (不一致警告など) が入るので 2 行まで折り返して全文見せる
        // (単一行だと右端で見切れると報告 2026-10)。
        let status = make_label(
            mtm,
            "",
            NSRect::new(NSPoint::new(PAD, 30.0), NSSize::new(FIELD_W, 40.0)),
        );
        status.setUsesSingleLineMode(false);

        for v in [
            &*active_label,
            &*conn_label,
            &*byok_h,
            &*provider_label,
            &*base_url_label,
            &*key_label,
            &*current_key_label,
            &*model_label,
            &*dict_h,
            &*reading_label,
            &*surface_label,
            &*status,
        ] {
            content.addSubview(v);
        }
        content.addSubview(&provider_popup);
        content.addSubview(&refresh_status);
        content.addSubview(&ollama_base_url_field);
        content.addSubview(&key_field);
        content.addSubview(&model_combo);
        content.addSubview(&fetch_models);
        content.addSubview(&ai_test);
        content.addSubview(&reading_field);
        content.addSubview(&surface_field);
        content.addSubview(&byok_save);
        content.addSubview(&dict_add);
        content.addSubview(&forget);
        content.addSubview(&close);
        window.setContentView(Some(&content));

        Self {
            window,
            provider_popup,
            active_label,
            conn_label,
            ollama_base_url_field,
            key_field,
            current_key_label,
            model_combo,
            reading_field,
            surface_field,
            status,
            prev_policy: Cell::new(None),
            _target: target,
        }
    }

    /// Cmd+C/V/X/A (コピー/ペースト/切り取り/全選択) を有効にする Edit メニューを設定する。
    ///
    /// 設定窓にはメニューバーが無いため、これが無いと Cmd+V 等が first responder
    /// (テキスト欄) に届かず **ペーストできない** (2026-10 実機)。target=nil で
    /// 標準アクション (paste: 等) を responder chain に流す。
    fn install_edit_menu(mtm: MainThreadMarker) {
        let app = NSApplication::sharedApplication(mtm);
        let main_menu = NSMenu::new(mtm);
        let edit_item = NSMenuItem::new(mtm);
        let edit_menu = NSMenu::new(mtm);
        edit_menu.setTitle(&NSString::from_str("編集"));
        unsafe {
            let items: [(&str, objc2::runtime::Sel, &str); 4] = [
                ("切り取り", sel!(cut:), "x"),
                ("コピー", sel!(copy:), "c"),
                ("ペースト", sel!(paste:), "v"),
                ("すべて選択", sel!(selectAll:), "a"),
            ];
            for (title, action, keq) in items {
                let _ = edit_menu.addItemWithTitle_action_keyEquivalent(
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(keq),
                );
            }
            edit_item.setSubmenu(Some(&edit_menu));
            main_menu.addItem(&edit_item);
            app.setMainMenu(Some(&main_menu));
        }
    }

    /// 前面に出してキーボードフォーカスを取る (policy を Regular に切替)。
    pub fn show(&self, mtm: MainThreadMarker, model_prefill: &str) {
        self.model_combo
            .setStringValue(&NSString::from_str(model_prefill));
        self.status.setStringValue(&NSString::from_str(""));
        self.window.center();
        // Cmd+V 等を有効化 (設定窓にメニューバーが無いとペーストできないため)。
        Self::install_edit_menu(mtm);

        let app = NSApplication::sharedApplication(mtm);
        // IME は background agent なので、設定中だけ通常アプリ化して active になる。
        self.prev_policy.set(Some(app.activationPolicy()));
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
        self.window.makeKeyWindow();
    }

    /// 閉じる: 窓を隠して activation policy を元に戻す (IME を background に戻す)。
    pub fn close(&self, mtm: MainThreadMarker) {
        self.window.orderOut(None);
        if let Some(p) = self.prev_policy.take() {
            NSApplication::sharedApplication(mtm).setActivationPolicy(p);
        }
    }

    /// 選択中プロバイダ ("anthropic" / "openrouter" / "ollama")。index で判定。
    pub fn selected_provider(&self) -> &'static str {
        match self.provider_popup.indexOfSelectedItem() {
            1 => "openrouter",
            2 => "ollama",
            _ => "anthropic",
        }
    }

    /// プロバイダのドロップダウンを指定プロバイダに合わせる (開いた時に ai.toml の値へ)。
    /// "openrouter"=1、"ollama"=2、それ以外 (anthropic)=0。
    pub fn set_provider(&self, provider: &str) {
        let idx: isize = match provider {
            "openrouter" => 1,
            "ollama" => 2,
            _ => 0,
        };
        self.provider_popup.selectItemAtIndex(idx);
    }

    /// Ollama ベース URL 欄の現在値。
    pub fn base_url_value(&self) -> String {
        self.ollama_base_url_field.stringValue().to_string()
    }

    /// Ollama ベース URL 欄を設定する (開いた時に ai.toml の値へ)。
    pub fn set_base_url(&self, s: &str) {
        self.ollama_base_url_field
            .setStringValue(&NSString::from_str(s));
    }

    /// 最上部の「AI学習で使用中」行を更新する。
    pub fn set_active_line(&self, s: &str) {
        self.active_label.setStringValue(&NSString::from_str(s));
    }

    /// AI 接続ステータス行 (🟢/🔴/⏳) を更新する。
    pub fn set_conn_status(&self, s: &str) {
        self.conn_label.setStringValue(&NSString::from_str(s));
    }

    /// 「現在のキー」行を更新する (選択プロバイダの保存済みキーを伏せ字で表示)。
    pub fn set_current_key(&self, s: &str) {
        self.current_key_label
            .setStringValue(&NSString::from_str(s));
    }

    /// キー欄の現在値。
    pub fn key_value(&self) -> String {
        self.key_field.stringValue().to_string()
    }

    /// モデル欄の現在値 (コンボボックスで選択または入力された文字列)。
    pub fn model_value(&self) -> String {
        self.model_combo.stringValue().to_string()
    }

    /// モデル一覧をコンボボックスに流し込む (実 API から取得した id 群)。
    pub fn set_model_items(&self, items: &[String]) {
        self.model_combo.removeAllItems();
        for it in items {
            let s = NSString::from_str(it);
            let obj: &AnyObject = s.as_ref();
            unsafe { self.model_combo.addItemWithObjectValue(obj) };
        }
    }

    /// 読み欄の現在値。
    pub fn reading_value(&self) -> String {
        self.reading_field.stringValue().to_string()
    }

    /// 表層欄の現在値。
    pub fn surface_value(&self) -> String {
        self.surface_field.stringValue().to_string()
    }

    /// ステータス行を更新する。
    pub fn set_status(&self, s: &str) {
        self.status.setStringValue(&NSString::from_str(s));
    }

    /// キー欄をクリアする。
    pub fn clear_key(&self) {
        self.key_field.setStringValue(&NSString::from_str(""));
    }

    /// 単語登録の入力欄をクリアする。
    pub fn clear_dict_fields(&self) {
        self.reading_field.setStringValue(&NSString::from_str(""));
        self.surface_field.setStringValue(&NSString::from_str(""));
    }

    /// 単語登録セクションの「読み」欄を事前入力し、「表層」欄へフォーカスする。
    ///
    /// 入力中に「この読みで単語登録」から開いたときに使う。読みが既に入っている
    /// ので、ユーザーは単語 (表層) だけ打って「辞書に登録」すれば済む。
    pub fn set_reading_prefill(&self, reading: &str) {
        self.reading_field
            .setStringValue(&NSString::from_str(reading));
        self.surface_field.setStringValue(&NSString::from_str(""));
        // 表層欄にキーボードフォーカスを移す (読みは入力済みなので単語を打てる)。
        self.window.makeFirstResponder(Some(&self.surface_field));
    }
}
