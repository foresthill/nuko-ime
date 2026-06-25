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

use crate::commit::CANDIDATE_PAGE_SIZE;

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

        // NSPanel を Borderless + NonactivatingPanel で生成。
        //
        // NonactivatingPanel を付けないと、NSPanel は「アプリを activate する
        // 普通のウィンドウ」扱いとなり、IME のように非アクティブアプリから
        // 浮かせて表示する用途には使えない (orderFront しても表示されないか、
        // 表示されても入力フォーカスを奪ってしまう)。
        //
        // 一次ソース: NSPanel docs of `NSWindowStyleMaskNonactivatingPanel`
        // (1<<7) と `setBecomesKeyOnlyIfNeeded:` 関連。
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

        // 浮遊パネルとして振舞わせる設定:
        // - setFloatingPanel(true) で他のウィンドウより前面に
        // - setBecomesKeyOnlyIfNeeded(true) で focus 奪取を最小化
        // - setLevel(NSPopUpMenuWindowLevel) で最前面 level に
        // - setHidesOnDeactivate(false) で別アプリ移動でも閉じない
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
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

    /// 候補リストと選択 index を更新し、パネルサイズを内容に合わせる (flat モード)。
    pub fn set_candidates(&self, items: &[String], selected: usize) {
        self.render(None, items, selected);
    }

    /// 候補リスト + **文節分割ヘッダ** を表示する (segmented モード)。
    ///
    /// パネル先頭に全文節を並べ、`focused` 文節を `【 】` + 背景色で強調する。
    /// 「今どの文節を変換しているか」をアプリの marked text 描画に依存せず示すため
    /// (Electron 等は selectionRange を描かない。実機検証 2026-06)。
    pub fn set_candidates_segmented(
        &self,
        items: &[String],
        selected: usize,
        segments: &[String],
        focused: usize,
    ) {
        self.render(Some((segments, focused)), items, selected);
    }

    /// 内部: ヘッダ有無を問わず描画 + 高さ調整。
    fn render(&self, header: Option<(&[String], usize)>, items: &[String], selected: usize) {
        if items.is_empty() {
            self.hide();
            return;
        }
        // 候補が多くても 1 ページ (CANDIDATE_PAGE_SIZE 件) ぶんだけ描く。
        // 実際に描画した行数を受け取って高さに使う。
        let (attr, line_count) = build_attributed_string(header, items, selected);
        self.label.setAttributedStringValue(&attr);

        let height = (line_count as f64) * LINE_HEIGHT + PADDING * 2.0;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(DEFAULT_WIDTH, height));
        self.label.setFrame(frame);
        self.panel.setContentSize(frame.size);
    }

    /// 指定の screen 座標 (top-left) にパネルを表示する。
    ///
    /// `orderFrontRegardless` を使うのは、IME プロセスが non-active な状態でも
    /// パネルを最前面に出すため。`orderFront:` は active アプリでなければ
    /// 表示されないケースがあるので、IME 用途では不適。
    ///
    /// 戻り値は呼び出し後の `isVisible()` 結果 (診断用)。
    pub fn show_at(&self, top_left: NSPoint) -> bool {
        self.panel.setFrameTopLeftPoint(top_left);
        self.panel.orderFrontRegardless();
        self.panel.isVisible()
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

/// 候補リスト (+任意の文節ヘッダ) を 1 つの NSAttributedString にフォーマットする。
///
/// 候補が `CANDIDATE_PAGE_SIZE` 件を超えるときは、**`selected` を含む 1 ページ分だけ**
/// 表示する (数字キー 1-9 に対応)。複数ページあるときは末尾にページ表示
/// (`p / total`) を付け、「まだ候補がある」ことを示す。戻り値は
/// `(属性文字列, 実際に描いた行数)` で、行数はパネル高さ計算に使う。
///
/// - `header = Some((segments, focused))` のとき、先頭行に全文節を並べ、focused 文節を
///   `【 】` で囲って背景色で強調する (segmented モードの「どこを変換中か」表示)
/// - 各候補行は「{ページ内番号 1-9}. {surface}」の形式
/// - 選択中の候補行には背景色 (`selectedTextBackgroundColor`) を付与
/// - 行頭マーカ ▶ も付ける (背景色が薄いテーマでも視覚的に分かるよう)
fn build_attributed_string(
    header: Option<(&[String], usize)>,
    items: &[String],
    selected: usize,
) -> (Retained<NSMutableAttributedString>, usize) {
    let mut combined = String::new();
    // 文節ヘッダの focused 範囲 / 選択候補行の範囲 (utf16: start, len)。背景強調に使う。
    let mut header_focus_range: Option<(usize, usize)> = None;
    let mut selected_line_range: Option<(usize, usize)> = None;
    let mut line_count: usize = 0;

    // 行間の改行を「2 行目以降の先頭」に入れるためのヘルパ。
    let newline_if_needed = |s: &mut String| {
        if !s.is_empty() {
            s.push('\n');
        }
    };

    // 1. 文節ヘッダ
    if let Some((segments, focused)) = header {
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                combined.push(' ');
            }
            if i == focused {
                let start = combined.encode_utf16().count();
                combined.push('【');
                combined.push_str(seg);
                combined.push('】');
                let len = combined.encode_utf16().count() - start;
                header_focus_range = Some((start, len));
            } else {
                combined.push_str(seg);
            }
        }
        line_count += 1;
    }

    // 2. 候補ページ (selected を含む 1 ページぶんだけ)
    let page = selected / CANDIDATE_PAGE_SIZE;
    let page_start = page * CANDIDATE_PAGE_SIZE;
    let page_end = (page_start + CANDIDATE_PAGE_SIZE).min(items.len());
    let page_count = items.len().div_ceil(CANDIDATE_PAGE_SIZE);
    let in_page_selected = selected - page_start;

    for (j, item) in items[page_start..page_end].iter().enumerate() {
        newline_if_needed(&mut combined);
        let marker = if j == in_page_selected { "▶ " } else { "  " };
        let line = format!("{marker}{}. {item}", j + 1);
        let start = combined.encode_utf16().count();
        combined.push_str(&line);
        let end = combined.encode_utf16().count();
        if j == in_page_selected {
            selected_line_range = Some((start, end - start));
        }
        line_count += 1;
    }

    // 3. ページ表示 (複数ページのときだけ「まだ候補がある」と分かるように)
    if page_count > 1 {
        newline_if_needed(&mut combined);
        combined.push_str(&format!("    {} / {}", page + 1, page_count));
        line_count += 1;
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

    // 文節ヘッダの focused 文節に背景色 (= どこを変換中かを強調)
    // 選択中候補の行に背景色
    for (start, len) in [header_focus_range, selected_line_range]
        .into_iter()
        .flatten()
    {
        if len > 0 {
            unsafe {
                let bg_color = NSColor::selectedTextBackgroundColor();
                let key = NSBackgroundColorAttributeName.copy();
                let value: &AnyObject = (*bg_color).as_ref();
                let dict = NSDictionary::from_slices(&[&*key], &[value]);
                attr_string.addAttributes_range(&dict, NSRange::new(start, len));
            }
        }
    }

    (attr_string, line_count)
}
