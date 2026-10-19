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
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSButtonType,
    NSControlStateValueOn, NSSecureTextField, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const W: f64 = 440.0;
const H: f64 = 480.0;
const PAD: f64 = 20.0;
const FIELD_W: f64 = W - PAD * 2.0;

/// ぬこIME 設定ウィンドウ (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct SettingsWindow {
    window: Retained<NSWindow>,
    provider_checkbox: Retained<NSButton>,
    key_field: Retained<NSSecureTextField>,
    model_field: Retained<NSTextField>,
    reading_field: Retained<NSTextField>,
    surface_field: Retained<NSTextField>,
    status: Retained<NSTextField>,
    /// 表示前の activation policy (閉じたら戻す)。
    prev_policy: Cell<Option<NSApplicationActivationPolicy>>,
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

impl SettingsWindow {
    /// ウィンドウとフィールドを生成する (非表示)。`save_target` = controller。
    pub fn new(mtm: MainThreadMarker, save_target: &AnyObject) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(W, H));
        // Closable を付けない = 赤ボタンで閉じない → 必ず「閉じる」経由で policy を戻せる。
        let style = NSWindowStyleMask::Titled;
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
        let key_label = make_label(mtm, "API キー:", row(374.0, 16.0));
        let key_field = NSSecureTextField::new(mtm);
        key_field.setFrame(row(348.0, 24.0));
        let model_label = make_label(
            mtm,
            "モデル (Anthropic: claude-sonnet-5-5 / OpenRouter: anthropic/claude-...):",
            row(320.0, 16.0),
        );
        let model_field = NSTextField::new(mtm);
        model_field.setFrame(row(294.0, 24.0));
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
        let reading_field = NSTextField::new(mtm);
        reading_field.setFrame(row(172.0, 24.0));
        let surface_label = make_label(mtm, "表層（変換後の語）:", row(144.0, 16.0));
        let surface_field = NSTextField::new(mtm);
        surface_field.setFrame(row(118.0, 24.0));
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
        }
    }

    /// 前面に出してキーボードフォーカスを取る (policy を Regular に切替)。
    pub fn show(&self, mtm: MainThreadMarker, model_prefill: &str) {
        self.model_field
            .setStringValue(&NSString::from_str(model_prefill));
        self.status.setStringValue(&NSString::from_str(""));
        self.window.center();

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
