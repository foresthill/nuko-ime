//! 設定画面

use crate::theme::Theme;
use nuko_platform::config::Config;

/// 設定アプリケーションの状態
#[derive(Debug, Clone)]
pub struct SettingsApp {
    /// 現在の設定
    config: Config,
    /// 変更があるかどうか
    dirty: bool,
    /// 現在のタブ
    current_tab: SettingsTab,
    /// テーマ
    theme: Theme,
}

/// 設定タブ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// 一般設定
    General,
    /// 辞書設定
    Dictionary,
    /// 外観設定
    Appearance,
    /// プライバシー設定
    Privacy,
    /// バージョン情報
    About,
}

impl SettingsApp {
    /// 新しい設定アプリを作成
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            dirty: false,
            current_tab: SettingsTab::General,
            theme: Theme::Light,
        }
    }

    /// デフォルト設定でアプリを作成
    #[must_use]
    pub fn with_default_config() -> Self {
        Self::new(Config::default())
    }

    /// 現在の設定を取得
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 設定を更新
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
        self.dirty = true;
    }

    /// 変更があるかどうか
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 変更をクリア
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// 現在のタブを取得
    #[must_use]
    pub fn current_tab(&self) -> SettingsTab {
        self.current_tab
    }

    /// タブを切り替え
    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.current_tab = tab;
    }

    /// テーマを設定
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// テーマを取得
    #[must_use]
    pub fn theme(&self) -> Theme {
        self.theme
    }

    // 設定変更メソッド

    /// 予測変換を切り替え
    pub fn toggle_prediction(&mut self) {
        self.config.general.enable_prediction = !self.config.general.enable_prediction;
        self.dirty = true;
    }

    /// 学習機能を切り替え
    pub fn toggle_learning(&mut self) {
        self.config.general.learning_enabled = !self.config.general.learning_enabled;
        self.dirty = true;
    }

    /// 候補数を設定
    pub fn set_candidate_count(&mut self, count: usize) {
        self.config.general.candidate_count = count.clamp(1, 20);
        self.dirty = true;
    }

    /// フォントサイズを設定
    pub fn set_font_size(&mut self, size: u32) {
        self.config.ui.font_size = size.clamp(8, 32);
        self.dirty = true;
    }

    /// 統計送信を切り替え
    pub fn toggle_statistics(&mut self) {
        self.config.privacy.send_statistics = !self.config.privacy.send_statistics;
        self.dirty = true;
    }
}

impl Default for SettingsApp {
    fn default() -> Self {
        Self::with_default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_app() {
        let mut app = SettingsApp::with_default_config();
        assert!(!app.is_dirty());

        app.toggle_prediction();
        assert!(app.is_dirty());
        assert!(!app.config().general.enable_prediction);

        app.clear_dirty();
        assert!(!app.is_dirty());
    }

    #[test]
    fn test_tabs() {
        let mut app = SettingsApp::with_default_config();
        assert_eq!(app.current_tab(), SettingsTab::General);

        app.set_tab(SettingsTab::Dictionary);
        assert_eq!(app.current_tab(), SettingsTab::Dictionary);
    }
}
