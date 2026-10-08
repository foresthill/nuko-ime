//! 学習状況パネル (NSPanel + NSScrollView + NSTextView ベース)
//!
//! メニュー「学習状況を見る…」「学習を今すぐ研ぎ直す」「単語登録」の結果を、IME プロセスが
//! non-active な状態でも最前面に表示する情報ウィンドウ。
//!
//! 浮遊方式は [`crate::candidate_panel`] と同一 (borderless + NonactivatingPanel +
//! orderFrontRegardless)。中身は **スクロール可能なテキストビュー** にして、
//! 学習選好が数百件に増えても画面を超えず、スクロールで全件読め、選択コピーもできる
//! (2026-10 ユーザー要望「リッチな画面」/「画面いっぱいで閉じられない」の修正)。

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSColor, NSFont, NSPanel, NSPopUpMenuWindowLevel, NSScreen,
    NSScrollView, NSTextView, NSView, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const WIDTH: f64 = 460.0;
const FONT_SIZE: f64 = 13.0;
const LINE_HEIGHT: f64 = 19.0;
const PADDING: f64 = 12.0;
const CLOSE_SIZE: f64 = 22.0;
const CLOSE_ROW: f64 = CLOSE_SIZE + 10.0; // ✕ ボタン行の高さ
const MIN_HEIGHT: f64 = 120.0;

/// 学習状況を表示する浮遊パネル (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct LearningStatusPanel {
    panel: Retained<NSPanel>,
    scroll: Retained<NSScrollView>,
    text_view: Retained<NSTextView>,
    close: Retained<NSButton>,
}

impl LearningStatusPanel {
    /// 空の状態で新規作成 (非表示)。
    pub fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, MIN_HEIGHT));

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

        let container: Retained<NSView> = NSView::initWithFrame(NSView::alloc(mtm), frame);

        // スクロールビュー (縦スクロールのみ)。
        let scroll: Retained<NSScrollView> =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
        scroll.setHasVerticalScroller(true);
        scroll.setHasHorizontalScroller(false);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);

        // テキストビュー (読み取り専用・選択可)。縦に伸びてスクロールする設定。
        let text_view: Retained<NSTextView> =
            NSTextView::initWithFrame(NSTextView::alloc(mtm), frame);
        text_view.setEditable(false);
        text_view.setSelectable(true);
        text_view.setDrawsBackground(false);
        let font = NSFont::systemFontOfSize(FONT_SIZE);
        text_view.setFont(Some(&font));
        text_view.setVerticallyResizable(true);
        text_view.setHorizontallyResizable(false);
        text_view.setTextContainerInset(NSSize::new(2.0, 4.0));
        if let Some(tc) = unsafe { text_view.textContainer() } {
            tc.setWidthTracksTextView(true);
        }
        scroll.setDocumentView(Some(&text_view));

        // ✕ 閉じるボタン。target = panel / action = orderOut: (単体で閉じられる)。
        let close = NSButton::new(mtm);
        close.setTitle(&NSString::from_str("✕"));
        close.setBordered(false);
        close.setFont(Some(&font));
        unsafe {
            let target: &AnyObject = &panel;
            close.setTarget(Some(target));
            close.setAction(Some(sel!(orderOut:)));
        }

        let scroll_view: &NSView = &scroll;
        let close_view: &NSView = close.as_super().as_super();
        container.addSubview(scroll_view);
        container.addSubview(close_view);
        panel.setContentView(Some(&container));

        let this = Self {
            panel,
            scroll,
            text_view,
            close,
        };
        this.layout(MIN_HEIGHT);
        this
    }

    /// 内部: content 高さに合わせてスクロールビューと ✕ ボタンを再配置する。
    fn layout(&self, height: f64) {
        // スクロールビューは下 PADDING から、上は ✕ 行ぶん空ける。
        self.scroll.setFrame(NSRect::new(
            NSPoint::new(PADDING, PADDING),
            NSSize::new(WIDTH - PADDING * 2.0, height - CLOSE_ROW - PADDING),
        ));
        // ✕ は右上。
        self.close.setFrame(NSRect::new(
            NSPoint::new(WIDTH - CLOSE_SIZE - 6.0, height - CLOSE_SIZE - 6.0),
            NSSize::new(CLOSE_SIZE, CLOSE_SIZE),
        ));
    }

    /// テキストを表示する (高さは画面内にクランプ、溢れたらスクロール、最前面表示)。
    pub fn show_text(&self, text: &str) {
        let ns = NSString::from_str(text);
        self.text_view.setString(&ns);

        let lines = text.lines().count().max(1);
        let content_h = (lines as f64) * LINE_HEIGHT + CLOSE_ROW + PADDING * 2.0;
        // 画面可視高さの 70% を上限、MIN_HEIGHT を下限。超過分はスクロール。
        let max_height = screen_visible_height().map_or(640.0, |h| h * 0.70);
        let height = content_h.clamp(MIN_HEIGHT, max_height);

        let content = NSSize::new(WIDTH, height);
        self.panel.setContentSize(content);
        self.layout(height);

        if let Some(pt) = center_top_left(content) {
            self.panel.setFrameTopLeftPoint(pt);
        }
        self.panel.orderFrontRegardless();
    }

    /// パネルを隠す。
    pub fn hide(&self) {
        if self.panel.isVisible() {
            self.panel.orderOut(None);
        }
    }
}

/// メインスクリーンの可視領域の高さ (メニューバー等を除く)。
fn screen_visible_height() -> Option<f64> {
    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    Some(screen.visibleFrame().size.height)
}

/// メインスクリーンの可視領域中央に置くための top-left 座標を返す。
fn center_top_left(size: NSSize) -> Option<NSPoint> {
    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    let vf = screen.visibleFrame();
    let x = vf.origin.x + (vf.size.width - size.width) / 2.0;
    let y = vf.origin.y + (vf.size.height + size.height) / 2.0;
    Some(NSPoint::new(x, y))
}
