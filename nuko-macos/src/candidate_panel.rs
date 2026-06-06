//! 自前候補ウィンドウ (NSPanel ベース、Phase 1.3 Step 2 v3 / C 案)
//!
//! ## なぜ自前か
//!
//! `IMKCandidates` には実機で動かない部分が複数あった (PR #29, #31, #32):
//! - panel.show() が別 `IMKInputController` の生成を引き起こす (PR #29)
//! - `selectCandidate:` を呼んでも青ハイライトが描画されない (PR #32 後)
//! - 既知の framework バグが [Shiki Suen の Gist](https://gist.github.com/ShikiSuen/73b7a55526c9fadd2da2a16d94ec5b49)
//!   や ["macOS Input Method Development Guidelines for 2026"](https://shikisuen.medium.com/macos-input-method-development-guidelines-for-2026-5123461fa53b)
//!   で「IMKCandidates is ancient rubbish」と評されている
//!
//! vChewing / Mozc 等の実装でも採用される **borderless NSPanel + NSTextField** で
//! 自前描画する。IMK の event routing には頼らず、controller 側のロジックで
//! 「state 更新 → panel.set_selected(idx)」を明示呼びする。

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackgroundColorAttributeName, NSBackingStoreType, NSColor, NSFont,
    NSForegroundColorAttributeName, NSPanel, NSPopUpMenuWindowLevel, NSTextField, NSView,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSCopying, NSDictionary, NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString,
};

/// 候補ウィンドウのサイズ計算用定数
const DEFAULT_WIDTH: f64 = 280.0;
const LINE_HEIGHT: f64 = 22.0;
const PADDING: f64 = 8.0;
const FONT_SIZE: f64 = 14.0;

/// 自前の候補ウィンドウ
///
/// thread_local で 1 つだけ保持される想定 ([`crate::state`] の `CUSTOM_PANEL`)。
pub struct CustomCandidatePanel {
    panel: Retained<NSPanel>,
    label: Retained<NSTextField>,
}

impl CustomCandidatePanel {
    /// 候補ウィンドウを新規作成 (空の状態で非表示)
    pub fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(DEFAULT_WIDTH, 60.0));

        // NSPanel を Borderless で生成
        let panel: Retained<NSPanel> = unsafe {
            let allocated = NSPanel::alloc(mtm);
            msg_send![
                allocated,
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };

        // 最前面・別アプリへのフォーカス移動でも閉じない設定
        panel.setLevel(NSPopUpMenuWindowLevel);
        panel.setHidesOnDeactivate(false);
        panel.setHasShadow(true);
        let bg = NSColor::controlBackgroundColor();
        panel.setBackgroundColor(Some(&bg));

        // NSTextField を multi-line 表示用に設定
        let label = NSTextField::new(mtm);
        label.setEditable(false);
        label.setSelectable(false);
        label.setBezeled(false);
        label.setBordered(false);
        label.setDrawsBackground(false);
        label.setFrame(frame);
        if let Some(cell) = label.cell() {
            cell.setUsesSingleLineMode(false);
            cell.setWraps(true);
        }
        let font = NSFont::systemFontOfSize(FONT_SIZE);
        label.setFont(Some(&font));
        // NSTextField → NSControl → NSView の継承チェーンを as_super で辿る
        let view: &NSView = label.as_super().as_super();
        panel.setContentView(Some(view));

        Self { panel, label }
    }

    /// 候補リストと選択 index を更新し、パネルサイズを内容に合わせる
    pub fn set_candidates(&self, items: &[String], selected: usize) {
        if items.is_empty() {
            self.hide();
            return;
        }
        let attr = build_attributed_string(items, selected);
        self.label.setAttributedStringValue(&attr);

        // 行数に合わせて高さ調整。幅は固定。
        let height = (items.len() as f64) * LINE_HEIGHT + PADDING * 2.0;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(DEFAULT_WIDTH, height));
        self.label.setFrame(frame);
        self.panel.setContentSize(frame.size);
    }

    /// 指定の screen 座標 (top-left) にパネルを表示する
    pub fn show_at(&self, top_left: NSPoint) {
        self.panel.setFrameTopLeftPoint(top_left);
        self.panel.orderFront(None);
    }

    /// パネルを隠す
    pub fn hide(&self) {
        if self.panel.isVisible() {
            self.panel.orderOut(None);
        }
    }

    /// 現在表示中か
    pub fn is_visible(&self) -> bool {
        self.panel.isVisible()
    }
}

/// 候補リストを 1 つの NSAttributedString にフォーマットする。
///
/// - 各行は「{1-9 で選べる場合は番号}. {surface}」の形式
/// - 選択中の行には背景色 (`selectedTextBackgroundColor`) を付与
/// - 行頭マーカ ▶ も付ける (背景色が薄いテーマでも視覚的に分かるよう)
fn build_attributed_string(
    items: &[String],
    selected: usize,
) -> Retained<NSMutableAttributedString> {
    let mut combined = String::new();
    let mut line_ranges: Vec<(usize, usize)> = Vec::with_capacity(items.len());

    for (i, item) in items.iter().enumerate() {
        let marker = if i == selected { "▶ " } else { "  " };
        let num = if i < 9 {
            format!("{}. ", i + 1)
        } else {
            "   ".to_string()
        };
        let line = format!("{marker}{num}{item}");
        let start_utf16 = combined.encode_utf16().count();
        combined.push_str(&line);
        let end_utf16 = combined.encode_utf16().count();
        if i + 1 < items.len() {
            combined.push('\n');
        }
        line_ranges.push((start_utf16, end_utf16));
    }

    let ns_str = NSString::from_str(&combined);
    let attr_string: Retained<NSMutableAttributedString> = unsafe {
        let allocated = NSMutableAttributedString::alloc();
        msg_send![allocated, initWithString: &*ns_str]
    };

    // NSAttributedStringKey (= NSString) → NSCopying 経由でコピーした
    // 静的キーを NSDictionary に詰める。属性値は NSColor を AnyObject にキャストして渡す。

    // 全体に前景色 (labelColor)
    let total_len = combined.encode_utf16().count();
    if total_len > 0 {
        unsafe {
            let fg_color = NSColor::labelColor();
            let key = NSForegroundColorAttributeName.copy();
            let value: &AnyObject = (*fg_color).as_ref();
            let dict = NSDictionary::from_slices(&[&*key], &[value]);
            attr_string.addAttributes_range(&dict, NSRange::new(0, total_len));
        }
    }

    // 選択中候補の行に背景色を上書き
    if let Some((start, end)) = line_ranges.get(selected) {
        let length = end - start;
        if length > 0 {
            unsafe {
                let bg_color = NSColor::selectedTextBackgroundColor();
                let key = NSBackgroundColorAttributeName.copy();
                let value: &AnyObject = (*bg_color).as_ref();
                let dict = NSDictionary::from_slices(&[&*key], &[value]);
                attr_string.addAttributes_range(&dict, NSRange::new(*start, length));
            }
        }
    }

    attr_string
}
