//! プラットフォーム固有のエラー型

use thiserror::Error;

/// プラットフォームエラー
#[derive(Error, Debug)]
pub enum PlatformError {
    /// 初期化エラー
    #[error("初期化エラー: {0}")]
    Initialization(String),

    /// 登録エラー
    #[error("IME登録エラー: {0}")]
    Registration(String),

    /// 設定エラー
    #[error("設定エラー: {0}")]
    Config(String),

    /// コアエンジンエラー
    #[error("コアエンジンエラー: {0}")]
    Core(#[from] nuko_core::error::NukoError),

    /// IOエラー
    #[error("IOエラー: {0}")]
    Io(#[from] std::io::Error),

    /// プラットフォーム固有エラー
    #[error("プラットフォームエラー: {0}")]
    Platform(String),

    /// 未サポートの機能
    #[error("未サポートの機能: {0}")]
    Unsupported(String),
}

/// 結果型のエイリアス
pub type Result<T> = std::result::Result<T, PlatformError>;
