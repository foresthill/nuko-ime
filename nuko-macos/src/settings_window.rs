//! ぬこIME 設定ウィンドウ (BYOK 鍵/モデル + 単語登録)。
//!
//! 情報表示専用の [`crate::learning_panel`] と違い **テキスト入力** が要るので、
//! Titled な key window にして `activateIgnoringOtherApps` + `makeKeyAndOrderFront`
//! で前面＋キーボードフォーカスを取る。
//!
//! 2 セクション:
//! - **BYOK**: API キー (→ Keychain、同梱 `nuko config set-key` を stdin 経由) と
//!   モデル (→ ai.toml)。保存は [`crate::state::save_settings`]。
//! - **単語登録**: 読み＋表層を稼働中エンジンに追加して即反映 (CLI と違い再起動不要)。
//!   保存は [`crate::state::add_dict_entry_from_window`]。

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSSecureTextField, NSTextField, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const W: f64 = 440.0;
const H: f64 = 430.0;
const PAD: f64 = 20.0;
const FIELD_W: f64 = W - PAD * 2.0;

/// ぬこIME 設定ウィンドウ (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct SettingsWindow {
    window: Retained<NSWindow>,
    key_field: Retained<NSSecureTextField>,
    model_field: Retained<NSTextField>,
    reading_field: Retained<NSTextField>,
    surface_field: Retained<NSTextField>,
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

/// 行ラベル用の矩形 (左寄せ・1 行)。
fn row(y: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(PAD, y), NSSize::new(FIELD_W, h))
}

impl SettingsWindow {
    /// ウィンドウとフィールドを生成する (非表示)。
    /// `save_target` が「BYOK を保存」「辞書に登録」ボタンの target (= controller)。
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
        window.setTitle(&NSString::from_str("ぬこIME 設定"));

        let content: Retained<NSView> = NSView::initWithFrame(NSView::alloc(mtm), frame);

        let hint = make_label(mtm, "ぬこIME 設定 — BYOK と 単語登録", row(398.0, 18.0));

        // ── BYOK セクション ──
        let byok_h = make_label(mtm, "【 BYOK（AI学習の鍵）】", row(372.0, 18.0));
        let key_label = make_label(mtm, "API キー (Anthropic):", row(348.0, 16.0));
        let key_field = NSSecureTextField::new(mtm);
        key_field.setFrame(row(322.0, 24.0));
        let model_label = make_label(mtm, "モデル (例: claude-sonnet-5-5):", row(294.0, 16.0));
        let model_field = NSTextField::new(mtm);
        model_field.setFrame(row(268.0, 24.0));
        let byok_save = NSButton::new(mtm);
        byok_save.setTitle(&NSString::from_str("BYOK を保存"));
        byok_save.setFrame(NSRect::new(
            NSPoint::new(PAD, 232.0),
            NSSize::new(150.0, 30.0),
        ));
        unsafe {
            byok_save.setTarget(Some(save_target));
            byok_save.setAction(Some(sel!(nukoSaveSettings:)));
        }

        // ── 単語登録 セクション ──
        let dict_h = make_label(mtm, "【 単語登録（読み → 表層）】", row(198.0, 18.0));
        let reading_label = make_label(mtm, "読み（かな）:", row(174.0, 16.0));
        let reading_field = NSTextField::new(mtm);
        reading_field.setFrame(row(148.0, 24.0));
        let surface_label = make_label(mtm, "表層（変換後の語）:", row(120.0, 16.0));
        let surface_field = NSTextField::new(mtm);
        surface_field.setFrame(row(94.0, 24.0));
        let dict_add = NSButton::new(mtm);
        dict_add.setTitle(&NSString::from_str("辞書に登録"));
        dict_add.setFrame(NSRect::new(
            NSPoint::new(PAD, 58.0),
            NSSize::new(150.0, 30.0),
        ));
        unsafe {
            dict_add.setTarget(Some(save_target));
            dict_add.setAction(Some(sel!(nukoAddDictEntry:)));
        }

        let close = NSButton::new(mtm);
        close.setTitle(&NSString::from_str("閉じる"));
        close.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 100.0, 58.0),
            NSSize::new(100.0, 30.0),
        ));
        unsafe {
            let target: &AnyObject = &window;
            close.setTarget(Some(target));
            close.setAction(Some(sel!(orderOut:)));
        }

        let status = make_label(mtm, "", row(28.0, 18.0));

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
            key_field,
            model_field,
            reading_field,
            surface_field,
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

    /// キー欄をクリアする (保存後にメモリ/画面へ残さない)。
    pub fn clear_key(&self) {
        self.key_field.setStringValue(&NSString::from_str(""));
    }

    /// 単語登録の入力欄をクリアする (登録後)。
    pub fn clear_dict_fields(&self) {
        self.reading_field.setStringValue(&NSString::from_str(""));
        self.surface_field.setStringValue(&NSString::from_str(""));
    }
}
