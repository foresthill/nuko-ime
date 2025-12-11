//! エラー型の定義

use thiserror::Error;

/// ぬこIMEのエラー型
#[derive(Error, Debug)]
pub enum NukoError {
    /// 辞書関連のエラー
    #[error("辞書エラー: {0}")]
    Dictionary(String),

    /// 変換エラー
    #[error("変換エラー: {0}")]
    Conversion(String),

    /// 学習データエラー
    #[error("学習データエラー: {0}")]
    Learning(String),

    /// IOエラー
    #[error("IOエラー: {0}")]
    Io(#[from] std::io::Error),

    /// データベースエラー
    #[error("データベースエラー: {0}")]
    Database(String),

    /// 設定エラー
    #[error("設定エラー: {0}")]
    Config(String),

    /// 形態素解析エラー
    #[error("形態素解析エラー: {0}")]
    Morphological(String),

    /// 不正な入力
    #[error("不正な入力: {0}")]
    InvalidInput(String),
}

/// 結果型のエイリアス
pub type Result<T> = std::result::Result<T, NukoError>;
