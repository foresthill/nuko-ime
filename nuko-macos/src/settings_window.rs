//! BYOK 設定ウィンドウ (API キー → Keychain、provider/model → ai.toml)。
//!
//! 情報表示専用の [`crate::learning_panel`] と違い **テキスト入力** が要るので、
//! Titled な key window にして `activateIgnoringOtherApps` + `makeKeyAndOrderFront`
//! で前面＋キーボードフォーカスを取る。保存処理 ([`crate::state::save_settings`]) は
//! 同梱 `nuko config set-key` (stdin) と ai.toml 書き込みで行う。

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSSecureTextField, NSTextField, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const W: f64 = 440.0;
const H: f64 = 240.0;
const PAD: f64 = 20.0;
const FIELD_W: f64 = W - PAD * 2.0;

/// BYOK 設定ウィンドウ (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct SettingsWindow {
    window: Retained<NSWindow>,
    key_field: Retained<NSSecureTextField>,
    model_field: Retained<NSTextField>,
    status: Retained<NSTextField>,
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

impl SettingsWindow {
    /// ウィンドウとフィールドを生成する (非表示)。`save_target` が「保存」ボタンの target。
    pub fn new(mtm: MainThreadMarker, save_target: &AnyObject) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(W, H));
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
        window.setTitle(&NSString::from_str("ぬこIME — BYOK 設定"));

        let content: Retained<NSView> = NSView::initWithFrame(NSView::alloc(mtm), frame);

        let hint = make_label(
            mtm,
            "Anthropic の API キーとモデルを設定します (鍵は Keychain に保存)",
            NSRect::new(NSPoint::new(PAD, 205.0), NSSize::new(FIELD_W, 18.0)),
        );
        let key_label = make_label(
            mtm,
            "API キー (Anthropic):",
            NSRect::new(NSPoint::new(PAD, 178.0), NSSize::new(FIELD_W, 18.0)),
        );
        let key_field = NSSecureTextField::new(mtm);
        key_field.setFrame(NSRect::new(
            NSPoint::new(PAD, 152.0),
            NSSize::new(FIELD_W, 24.0),
        ));
        let model_label = make_label(
            mtm,
            "モデル (例: claude-sonnet-5-5):",
            NSRect::new(NSPoint::new(PAD, 122.0), NSSize::new(FIELD_W, 18.0)),
        );
        let model_field = NSTextField::new(mtm);
        model_field.setFrame(NSRect::new(
            NSPoint::new(PAD, 96.0),
            NSSize::new(FIELD_W, 24.0),
        ));

        let save = NSButton::new(mtm);
        save.setTitle(&NSString::from_str("保存"));
        save.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 100.0, 50.0),
            NSSize::new(100.0, 32.0),
        ));
        unsafe {
            save.setTarget(Some(save_target));
            save.setAction(Some(sel!(nukoSaveSettings:)));
        }

        let close = NSButton::new(mtm);
        close.setTitle(&NSString::from_str("閉じる"));
        close.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 210.0, 50.0),
            NSSize::new(100.0, 32.0),
        ));
        unsafe {
            let target: &AnyObject = &window;
            close.setTarget(Some(target));
            close.setAction(Some(sel!(orderOut:)));
        }

        let status = make_label(
            mtm,
            "",
            NSRect::new(NSPoint::new(PAD, 16.0), NSSize::new(FIELD_W, 18.0)),
        );

        content.addSubview(&hint);
        content.addSubview(&key_label);
        content.addSubview(&key_field);
        content.addSubview(&model_label);
        content.addSubview(&model_field);
        content.addSubview(&save);
        content.addSubview(&close);
        content.addSubview(&status);
        window.setContentView(Some(&content));

        Self {
            window,
            key_field,
            model_field,
            status,
        }
    }

    /// 前面に出してキーボードフォーカスを取る。`model_prefill` をモデル欄に入れる。
    pub fn show(&self, mtm: MainThreadMarker, model_prefill: &str) {
        self.model_field
            .setStringValue(&NSString::from_str(model_prefill));
        self.status.setStringValue(&NSString::from_str(""));
        self.window.center();
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }

    /// キー欄の現在値。
    pub fn key_value(&self) -> String {
        self.key_field.stringValue().to_string()
    }

    /// モデル欄の現在値。
    pub fn model_value(&self) -> String {
        self.model_field.stringValue().to_string()
    }

    /// ステータス行を更新する。
    pub fn set_status(&self, s: &str) {
        self.status.setStringValue(&NSString::from_str(s));
    }

    /// キー欄をクリアする (保存後にメモリ/画面へ残さない)。
    pub fn clear_key(&self) {
        self.key_field.setStringValue(&NSString::from_str(""));
    }
}
