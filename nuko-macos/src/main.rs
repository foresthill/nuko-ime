#![allow(non_snake_case)]

mod controller;
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

fn main() {
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
