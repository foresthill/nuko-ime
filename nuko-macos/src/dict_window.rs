//! ぬこIME マイ辞書ウィンドウ (#103 再設計)。
//!
//! 登録一覧 + 追加/更新 + 編集 + 削除 を **1 つの動線** にまとめた、BYOK 設定とは
//! **独立したウィンドウ**。ユーザー要望 (2026-11):「BYOK の中に辞書があるのは変。
//! 追加も編集/削除と同じ動線に」。
//!
//! 実装は設定ウィンドウ ([`crate::settings_window`]) の実証済みパターンを踏襲する:
//! - ボタン target は controller 破棄に影響されない安定オブジェクト ([`DictTarget`])。
//! - IME は background agent なので表示中だけ activation policy を Regular に。
//! - 設定窓と同じ Edit メニュー (Cmd+V 等) を使う。
//! - 一覧は cell-based NSTableView、データは thread_local [`DICT_ROWS`] から供給。

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSScrollView,
    NSTableColumn, NSTableView, NSTableViewDataSource, NSTextField, NSView, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::settings_window::{editable_field, make_label, SettingsWindow};

thread_local! {
    /// マイ辞書ウィンドウ (NSTableView) の表示データ (読み, 表層)。
    /// データソース ([`DictTarget`]) が参照し、[`DictWindow::refresh`] が更新 + reloadData する。
    static DICT_ROWS: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

const W: f64 = 720.0;
const H: f64 = 480.0;
const PAD: f64 = 20.0;
const FIELD_W: f64 = W - PAD * 2.0;

// マイ辞書ウィンドウの安定した target (ボタンアクション + テーブルのデータソース)。
// 設定窓の `SettingsTarget` と同じ理由で controller でなく独立オブジェクトにする
// (フォーカス移動で controller が破棄され target=nil になる問題の回避)。
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "NukoDictTarget"]
    #[thread_kind = MainThreadOnly]
    struct DictTarget;

    unsafe impl NSObjectProtocol for DictTarget {}

    // マイ辞書一覧のデータ供給 (cell-based)。選択はボタン側 (selectedRow) に任せるので
    // delegate は実装しない (objc2 面を最小化、設定窓と同方針)。
    unsafe impl NSTableViewDataSource for DictTarget {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _tv: &NSTableView) -> isize {
            DICT_ROWS.with(|r| r.borrow().len() as isize)
        }

        #[unsafe(method(tableView:objectValueForTableColumn:row:))]
        fn object_value(
            &self,
            _tv: &NSTableView,
            col: Option<&NSTableColumn>,
            row: isize,
        ) -> *mut NSString {
            // objc2 0.6 のメソッド戻り値は Encode 必須なので *mut で返し、autorelease に委ねる。
            let text: Option<String> = DICT_ROWS.with(|r| {
                let rows = r.borrow();
                let (reading, surface) = rows.get(usize::try_from(row).ok()?)?;
                let want_reading = col
                    .map(|c| c.identifier().to_string() == "reading")
                    .unwrap_or(true);
                Some(if want_reading {
                    reading.clone()
                } else {
                    surface.clone()
                })
            });
            match text {
                Some(t) => Retained::autorelease_return(NSString::from_str(&t)),
                None => std::ptr::null_mut(),
            }
        }
    }

    // 窓が閉じたら IME を background agent (.Accessory) に戻す (dock アイコンを消す)。
    unsafe impl NSWindowDelegate for DictTarget {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: Option<&AnyObject>) {
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm)
                    .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            }
        }
    }

    impl DictTarget {
        /// 「辞書に追加 / 更新」: 読み+表層を登録。編集モードなら元を置換 (state 側で判定)。
        #[unsafe(method(nukoDictAdd:))]
        fn add(&self, _s: Option<&AnyObject>) {
            crate::state::dict_add_from_window();
        }
        /// 「選択を編集」: 選択行を読み/表層欄に読み込み、編集モードにする。
        #[unsafe(method(nukoDictEdit:))]
        fn edit(&self, _s: Option<&AnyObject>) {
            crate::state::dict_edit_selected();
        }
        /// 「選択を削除」: 選択行を user_dict.json から削除して即反映。
        #[unsafe(method(nukoDictDelete:))]
        fn delete(&self, _s: Option<&AnyObject>) {
            crate::state::dict_delete_selected();
        }
    }
);

impl DictTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

/// 左寄せ 1 行の矩形。
fn row(y: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(PAD, y), NSSize::new(FIELD_W, h))
}

/// ぬこIME マイ辞書ウィンドウ (アプリ全体で 1 つ、[`crate::state`] が保持)。
pub struct DictWindow {
    window: Retained<NSWindow>,
    reading_field: Retained<NSTextField>,
    surface_field: Retained<NSTextField>,
    dict_table: Retained<NSTableView>,
    status: Retained<NSTextField>,
    /// ボタン/データソースの安定 target (保持し続ける)。
    _target: Retained<DictTarget>,
}

impl DictWindow {
    /// ウィンドウとフィールドを生成する (非表示)。
    pub fn new(mtm: MainThreadMarker) -> Self {
        let target = DictTarget::new(mtm);
        let btn_target: &AnyObject = &target;
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
        unsafe { window.setReleasedWhenClosed(false) };
        window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        window.setTitle(&NSString::from_str("ぬこIME マイ辞書"));

        let content: Retained<NSView> = NSView::initWithFrame(NSView::alloc(mtm), frame);

        let header = make_label(
            mtm,
            "【 マイ辞書（登録した単語の一覧）】行を選んで 編集 / 削除",
            NSRect::new(NSPoint::new(PAD, 448.0), NSSize::new(FIELD_W - 120.0, 18.0)),
        );

        // スクロール可能なテーブル (読み / 表層 の 2 列、cell-based、単一選択)。
        let dict_scroll = NSScrollView::new(mtm);
        dict_scroll.setFrame(NSRect::new(
            NSPoint::new(PAD, 232.0),
            NSSize::new(FIELD_W, 208.0),
        ));
        dict_scroll.setHasVerticalScroller(true);
        let dict_table = NSTableView::new(mtm);
        let col_reading = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("reading"),
        );
        col_reading.setWidth(220.0);
        col_reading
            .headerCell()
            .setStringValue(&NSString::from_str("読み"));
        let col_surface = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("surface"),
        );
        col_surface.setWidth(FIELD_W - 240.0);
        col_surface
            .headerCell()
            .setStringValue(&NSString::from_str("表層"));
        dict_table.addTableColumn(&col_reading);
        dict_table.addTableColumn(&col_surface);
        dict_table.setUsesAlternatingRowBackgroundColors(true);
        dict_table.setAllowsMultipleSelection(false);
        unsafe {
            dict_table.setDataSource(Some(ProtocolObject::from_ref(&*target)));
        }
        dict_scroll.setDocumentView(Some(&dict_table));

        // 追加/編集用の入力欄 (一覧のすぐ下＝同じ動線)。
        let reading_label = make_label(mtm, "読み（かな）:", row(206.0, 16.0));
        let reading_field = editable_field(mtm, row(180.0, 24.0));
        let surface_label = make_label(mtm, "表層（変換後の語）:", row(152.0, 16.0));
        let surface_field = editable_field(mtm, row(126.0, 24.0));

        let add = NSButton::new(mtm);
        add.setTitle(&NSString::from_str("辞書に追加 / 更新"));
        add.setFrame(NSRect::new(
            NSPoint::new(PAD, 82.0),
            NSSize::new(170.0, 30.0),
        ));
        unsafe {
            add.setTarget(Some(btn_target));
            add.setAction(Some(sel!(nukoDictAdd:)));
        }
        let edit = NSButton::new(mtm);
        edit.setTitle(&NSString::from_str("選択を編集"));
        edit.setFrame(NSRect::new(
            NSPoint::new(PAD + 180.0, 82.0),
            NSSize::new(130.0, 30.0),
        ));
        unsafe {
            edit.setTarget(Some(btn_target));
            edit.setAction(Some(sel!(nukoDictEdit:)));
        }
        let delete = NSButton::new(mtm);
        delete.setTitle(&NSString::from_str("選択を削除"));
        delete.setFrame(NSRect::new(
            NSPoint::new(PAD + 320.0, 82.0),
            NSSize::new(130.0, 30.0),
        ));
        unsafe {
            delete.setTarget(Some(btn_target));
            delete.setAction(Some(sel!(nukoDictDelete:)));
        }

        let close = NSButton::new(mtm);
        close.setTitle(&NSString::from_str("閉じる"));
        close.setFrame(NSRect::new(
            NSPoint::new(W - PAD - 100.0, 444.0),
            NSSize::new(100.0, 26.0),
        ));
        unsafe {
            // 設定窓と同じく window.performClose: で確実に閉じる (target 破棄に依存しない)。
            let win_any: &AnyObject = &window;
            close.setTarget(Some(win_any));
            close.setAction(Some(sel!(performClose:)));
        }

        let status = make_label(
            mtm,
            "",
            NSRect::new(NSPoint::new(PAD, 30.0), NSSize::new(FIELD_W, 40.0)),
        );
        status.setUsesSingleLineMode(false);

        for v in [&*header, &*reading_label, &*surface_label, &*status] {
            content.addSubview(v);
        }
        content.addSubview(&dict_scroll);
        content.addSubview(&reading_field);
        content.addSubview(&surface_field);
        content.addSubview(&add);
        content.addSubview(&edit);
        content.addSubview(&delete);
        content.addSubview(&close);
        window.setContentView(Some(&content));

        Self {
            window,
            reading_field,
            surface_field,
            dict_table,
            status,
            _target: target,
        }
    }

    /// 前面に出してキーボードフォーカスを取る。`reading_prefill` があれば読み欄に入れる。
    pub fn show(&self, mtm: MainThreadMarker, reading_prefill: Option<&str>) {
        self.status.setStringValue(&NSString::from_str(""));
        self.refresh();
        match reading_prefill {
            Some(r) if !r.is_empty() => {
                self.reading_field.setStringValue(&NSString::from_str(r));
                self.surface_field.setStringValue(&NSString::from_str(""));
            }
            _ => {}
        }
        self.window.center();
        // Cmd+V 等を有効化 (設定窓と共通の Edit メニュー)。
        SettingsWindow::install_edit_menu(mtm);

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
        self.window.makeKeyWindow();
        // 読み事前入力時は表層欄にフォーカス (単語だけ打てば済む)。
        if matches!(reading_prefill, Some(r) if !r.is_empty()) {
            self.window.makeFirstResponder(Some(&self.surface_field));
        }
    }

    /// 登録済み一覧を読み直してテーブルを再描画する。
    pub fn refresh(&self) {
        let rows = crate::state::user_dict_entries();
        DICT_ROWS.with(|r| *r.borrow_mut() = rows);
        self.dict_table.reloadData();
    }

    /// 選択中の行の (読み, 表層)。未選択なら None。
    pub fn selected_entry(&self) -> Option<(String, String)> {
        let row = self.dict_table.selectedRow();
        let idx = usize::try_from(row).ok()?;
        DICT_ROWS.with(|r| r.borrow().get(idx).cloned())
    }

    /// 読み欄・表層欄を設定する (「選択を編集」で使う)。
    pub fn set_fields(&self, reading: &str, surface: &str) {
        self.reading_field
            .setStringValue(&NSString::from_str(reading));
        self.surface_field
            .setStringValue(&NSString::from_str(surface));
        self.window.makeFirstResponder(Some(&self.surface_field));
    }

    pub fn reading_value(&self) -> String {
        self.reading_field.stringValue().to_string()
    }

    pub fn surface_value(&self) -> String {
        self.surface_field.stringValue().to_string()
    }

    pub fn clear_fields(&self) {
        self.reading_field.setStringValue(&NSString::from_str(""));
        self.surface_field.setStringValue(&NSString::from_str(""));
    }

    pub fn set_status(&self, s: &str) {
        self.status.setStringValue(&NSString::from_str(s));
    }
}
