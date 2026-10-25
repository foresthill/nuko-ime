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
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSButtonType,
    NSControlStateValueOn, NSMenu, NSMenuItem, NSTextField, NSView, NSWindow, NSWindowStyleMask,
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
    struct SettingsTarget;

    unsafe impl NSObjectProtocol for SettingsTarget {}

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
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

// OpenRouter の API キー (sk-or-v1-… 約73文字) が 1 行で全部見える幅にする
// (狭いとスクロールして一部しか見えず「気持ち悪い」とユーザー報告 2026-10)。
const W: f64 = 720.0;
const H: f64 = 480.0;
const PAD: f64 = 20.0;
const FIELD_W: f64 = W - PAD * 2.0;

/// ぬこIME 設定ウィンドウ (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct SettingsWindow {
    window: Retained<NSWindow>,
    provider_checkbox: Retained<NSButton>,
    // NSSecureTextField だと secure input mode が IME プロセスで効かず入力不能になる
    // (2026-10 実機)。平文の NSTextField にして確実に打てるようにする (保存後にクリア)。
    key_field: Retained<NSTextField>,
    model_field: Retained<NSTextField>,
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
        let target = SettingsTarget::new();
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
        window.setTitle(&NSString::from_str("ぬこIME 設定"));

        let content: Retained<NSView> = NSView::initWithFrame(NSView::alloc(mtm), frame);

        let hint = make_label(mtm, "ぬこIME 設定 — BYOK と 単語登録", row(452.0, 18.0));

        // ── BYOK セクション ──
        let byok_h = make_label(mtm, "【 BYOK（AI学習の鍵）】", row(426.0, 18.0));
        let provider_checkbox = NSButton::new(mtm);
        provider_checkbox.setButtonType(NSButtonType::Switch);
        provider_checkbox.setTitle(&NSString::from_str(
            "OpenRouter を使う（オフ = Anthropic 直）",
        ));
        provider_checkbox.setFrame(row(400.0, 20.0));
        let key_label = make_label(mtm, "API キー（平文表示・保存後クリア）:", row(374.0, 16.0));
        let key_field = editable_field(mtm, row(348.0, 24.0));
        let model_label = make_label(
            mtm,
            "モデル (Anthropic: claude-sonnet-5-5 / OpenRouter: anthropic/claude-...):",
            row(320.0, 16.0),
        );
        let model_field = editable_field(mtm, row(294.0, 24.0));
        let byok_save = NSButton::new(mtm);
        byok_save.setTitle(&NSString::from_str("BYOK を保存"));
        byok_save.setFrame(NSRect::new(
            NSPoint::new(PAD, 258.0),
            NSSize::new(150.0, 30.0),
        ));
        unsafe {
            byok_save.setTarget(Some(save_target));
            byok_save.setAction(Some(sel!(nukoSaveSettings:)));
        }

        // ── 単語登録 セクション ──
        let dict_h = make_label(mtm, "【 単語登録（読み → 表層）】", row(222.0, 18.0));
        let reading_label = make_label(mtm, "読み（かな）:", row(198.0, 16.0));
        let reading_field = editable_field(mtm, row(172.0, 24.0));
        let surface_label = make_label(mtm, "表層（変換後の語）:", row(144.0, 16.0));
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

        let close = NSButton::new(mtm);
        close.setTitle(&NSString::from_str("閉じる"));
        close.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 100.0, 82.0),
            NSSize::new(100.0, 30.0),
        ));
        unsafe {
            close.setTarget(Some(save_target));
            close.setAction(Some(sel!(nukoCloseSettings:)));
        }

        let status = make_label(mtm, "", row(48.0, 18.0));

        for v in [
            &*hint,
            &*byok_h,
            &*key_label,
            &*model_label,
            &*dict_h,
            &*reading_label,
            &*surface_label,
            &*status,
        ] {
            content.addSubview(v);
        }
        content.addSubview(&provider_checkbox);
        content.addSubview(&key_field);
        content.addSubview(&model_field);
        content.addSubview(&reading_field);
        content.addSubview(&surface_field);
        content.addSubview(&byok_save);
        content.addSubview(&dict_add);
        content.addSubview(&close);
        window.setContentView(Some(&content));

        Self {
            window,
            provider_checkbox,
            key_field,
            model_field,
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
        self.model_field
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

    /// 選択中プロバイダ ("anthropic" / "openrouter")。
    pub fn selected_provider(&self) -> &'static str {
        if self.provider_checkbox.state() == NSControlStateValueOn {
            "openrouter"
        } else {
            "anthropic"
        }
    }

    /// キー欄の現在値。
    pub fn key_value(&self) -> String {
        self.key_field.stringValue().to_string()
    }

    /// モデル欄の現在値。
    pub fn model_value(&self) -> String {
        self.model_field.stringValue().to_string()
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
}
