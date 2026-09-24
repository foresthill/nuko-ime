//! 学習状況パネル (NSPanel ベース)
//!
//! メニュー「学習状況を見る…」「学習を今すぐ研ぎ直す」の結果を、IME プロセスが
//! non-active な状態でも最前面に表示するための小さな情報ウィンドウ。
//!
//! 仕組みは [`crate::candidate_panel`] と同一:
//! borderless + `NonactivatingPanel` + `orderFrontRegardless` で、フォーカスを
//! 奪わずに浮遊表示する (IMKCandidates を避けた自前 NSPanel 方式)。
//! 候補ウィンドウと違い候補描画はせず、複数行のプレーンテキストを 1 枚出すだけ。

use objc2::rc::Retained;
use objc2::{msg_send, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSTextField,
    NSView, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const WIDTH: f64 = 360.0;
const FONT_SIZE: f64 = 13.0;
const LINE_HEIGHT: f64 = 20.0;
const PADDING: f64 = 14.0;

/// 学習状況を表示する浮遊パネル (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct LearningStatusPanel {
    panel: Retained<NSPanel>,
    label: Retained<NSTextField>,
}

impl LearningStatusPanel {
    /// 空の状態で新規作成 (非表示)。
    pub fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, 120.0));

        // candidate_panel と同じ borderless + NonactivatingPanel。
        // NonactivatingPanel が無いと IME (非アクティブアプリ) から浮かせられない。
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel: Retained<NSPanel> = unsafe {
            let allocated = NSPanel::alloc(mtm);
            msg_send![
                allocated,
                initWithContentRect: frame,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setLevel(NSPopUpMenuWindowLevel);
        panel.setHidesOnDeactivate(false);
        panel.setHasShadow(true);
        let bg = NSColor::controlBackgroundColor();
        panel.setBackgroundColor(Some(&bg));

        let label = NSTextField::new(mtm);
        label.setEditable(false);
        label.setSelectable(false);
        label.setBezeled(false);
        label.setBordered(false);
        label.setDrawsBackground(false);
        label.setFrame(NSRect::new(
            NSPoint::new(PADDING, PADDING),
            NSSize::new(WIDTH - PADDING * 2.0, 120.0 - PADDING * 2.0),
        ));
        if let Some(cell) = label.cell() {
            cell.setUsesSingleLineMode(false);
            cell.setWraps(true);
        }
        let font = NSFont::systemFontOfSize(FONT_SIZE);
        label.setFont(Some(&font));
        // NSTextField → NSControl → NSView
        let view: &NSView = label.as_super().as_super();
        panel.setContentView(Some(view));

        Self { panel, label }
    }

    /// テキストを表示する (内容に合わせて高さ調整 + 画面中央へ配置 + 最前面表示)。
    pub fn show_text(&self, text: &str) {
        let ns = NSString::from_str(text);
        self.label.setStringValue(&ns);

        let lines = text.lines().count().max(1);
        let height = (lines as f64) * LINE_HEIGHT + PADDING * 2.0;
        let content = NSSize::new(WIDTH, height);
        self.panel.setContentSize(content);
        self.label.setFrame(NSRect::new(
            NSPoint::new(PADDING, PADDING),
            NSSize::new(WIDTH - PADDING * 2.0, height - PADDING * 2.0),
        ));

        if let Some(pt) = center_top_left(content) {
            self.panel.setFrameTopLeftPoint(pt);
        }
        self.panel.orderFrontRegardless();
    }

    /// パネルを隠す (次の打鍵時などに呼ぶ)。
    pub fn hide(&self) {
        if self.panel.isVisible() {
            self.panel.orderOut(None);
        }
    }
}

/// メインスクリーンの可視領域中央に置くための top-left 座標を返す。
fn center_top_left(size: NSSize) -> Option<NSPoint> {
    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    let vf = screen.visibleFrame();
    let x = vf.origin.x + (vf.size.width - size.width) / 2.0;
    // macOS 座標系は左下原点。top-left の y は中央 + 高さの半分。
    let y = vf.origin.y + (vf.size.height + size.height) / 2.0;
    Some(NSPoint::new(x, y))
}
