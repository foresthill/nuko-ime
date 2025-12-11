//! IME設定

use crate::error::{PlatformError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// IME設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 一般設定
    pub general: GeneralConfig,
    /// 辞書設定
    pub dictionary: DictionaryConfig,
    /// UI設定
    pub ui: UiConfig,
    /// プライバシー設定
    pub privacy: PrivacyConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            dictionary: DictionaryConfig::default(),
            ui: UiConfig::default(),
            privacy: PrivacyConfig::default(),
        }
    }
}

impl Config {
    /// 設定ファイルから読み込み
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content)
            .map_err(|e| PlatformError::Config(format!("設定ファイルのパースに失敗: {e}")))
    }

    /// 設定ファイルに保存
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| PlatformError::Config(format!("シリアライズに失敗: {e}")))?;

        std::fs::write(path, content)?;
        Ok(())
    }

    /// デフォルトの設定ファイルパスを取得
    #[must_use]
    pub fn default_path() -> PathBuf {
        dirs_config_path().join("config.toml")
    }
}

/// 一般設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// 予測変換を有効にする
    pub enable_prediction: bool,
    /// 候補の表示数
    pub candidate_count: usize,
    /// 学習機能を有効にする
    pub learning_enabled: bool,
    /// 入力モード（ローマ字/かな）
    pub input_mode: InputMode,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            enable_prediction: true,
            candidate_count: 9,
            learning_enabled: true,
            input_mode: InputMode::Romaji,
        }
    }
}

/// 入力モード
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    /// ローマ字入力
    Romaji,
    /// かな入力
    Kana,
}

/// 辞書設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryConfig {
    /// システム辞書の種類
    pub system_dict: String,
    /// ユーザー辞書のパス
    pub user_dict_path: PathBuf,
    /// 学習データのパス
    pub learning_data_path: PathBuf,
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        let data_dir = dirs_data_path();
        Self {
            system_dict: "ipadic".to_string(),
            user_dict_path: data_dir.join("user.dict"),
            learning_data_path: data_dir.join("learning.json"),
        }
    }
}

/// UI設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// テーマ
    pub theme: Theme,
    /// フォントサイズ
    pub font_size: u32,
    /// 透明度（0.0〜1.0）
    pub transparency: f32,
    /// 候補ウィンドウの位置
    pub candidate_position: CandidatePosition,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            font_size: 14,
            transparency: 0.95,
            candidate_position: CandidatePosition::Cursor,
        }
    }
}

/// テーマ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    /// システム設定に従う
    System,
    /// ライト
    Light,
    /// ダーク
    Dark,
}

/// 候補ウィンドウの位置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidatePosition {
    /// カーソル位置
    Cursor,
    /// 画面下部
    Bottom,
    /// カスタム位置
    Custom { x: i32, y: i32 },
}

/// プライバシー設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// 匿名統計データを送信する
    pub send_statistics: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            send_statistics: false,
        }
    }
}

/// 設定ディレクトリのパスを取得
fn dirs_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("nuko-ime")
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("nuko-ime")
    }

    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".config")
            })
            .join("nuko-ime")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        PathBuf::from(".").join("nuko-ime")
    }
}

/// データディレクトリのパスを取得
fn dirs_data_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("nuko-ime")
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("nuko-ime")
    }

    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".local")
                    .join("share")
            })
            .join("nuko-ime")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        PathBuf::from(".").join("nuko-ime")
    }
}
