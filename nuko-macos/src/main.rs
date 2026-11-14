#![allow(non_snake_case)]
// unknown_lints を先に許可 → 下行の新 lint 名を旧 clippy が未知扱いしても壊れない (forward-compat)。
#![allow(unknown_lints)]
#![allow(clippy::assert_is_empty)] // clippy 1.99+ の新 lint (テスト assert! の可読性優先)

mod candidate_panel;
mod commit;
mod controller;
mod dict_window;
mod learning_panel;
mod settings_window;
mod state;

use objc2::AnyThread;
use objc2::ClassType;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;
use objc2_input_method_kit::IMKServer;
use tracing::info;

// controller.rs で定義される NukoInputController をインポート
use controller::NukoInputController;

/// panic を `~/Library/Application Support/nuko-ime/panic.log` に自己記録する hook。
///
/// IME の stderr はどこにも出ず、release は `strip` + `panic = "abort"` なので、
/// クラッシュしても従来は原因 (file:line) が一切残らなかった。だが
/// - `set_hook` は `panic = "abort"` でも **abort の直前に必ず走る**、
/// - `Location`(file:line) は `#[track_caller]` 由来で **strip 後も残る**、
/// - panic メッセージも payload から取れる、
///
/// ため、release のまま全 panic を追記記録できる。これで「毎日1回落ちるが原因
/// 不明」を「次の1回で正確な発生箇所が出る」に変える。クラッシュ自体は止めない
/// (それは原因特定後の別対応)。
fn install_panic_logger() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = state::nuko_app_support_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let loc = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown location>".to_string());
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let thread = std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_string();
            let line = format!("[unix:{ts}] thread '{thread}' panicked at {loc}:\n  {msg}\n");
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("panic.log"))
            {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
        // 既定 hook (stderr 出力) も残す。
        prev(info);
    }));
}

fn main() {
    // 何よりも先に panic logger を入れる (以降の初期化で落ちても記録する)。
    install_panic_logger();

    // ログ初期化
    tracing_subscriber::fmt()
        .with_env_filter("nuko=debug,info")
        .init();

    info!("ぬこIME macOS starting...");

    // メインスレッドで実行されていることを確認
    let mtm = MainThreadMarker::new().expect("Must be on main thread");

    // NSApplication 取得
    let app = NSApplication::sharedApplication(mtm);

    // NukoInputController クラスをObjCランタイムに強制登録
    // IMKServer が Info.plist の InputMethodServerControllerClass からクラスを探す前に
    // 登録されている必要がある
    let _cls = NukoInputController::class();
    info!("NukoInputController class registered");

    // IMKServer を作成
    // connection_name は Info.plist の InputMethodConnectionName と一致必須
    let connection_name = NSString::from_str("com.nuko.inputmethod.Nuko_Connection");
    let bundle_id = NSString::from_str("com.nuko.inputmethod.Nuko");

    let _server = unsafe {
        IMKServer::initWithName_bundleIdentifier(
            IMKServer::alloc(),
            Some(&connection_name),
            Some(&bundle_id),
        )
    };

    info!("IMKServer created, entering run loop...");

    // イベントループ開始（ブロッキング、永久に返らない）
    app.run();
}
