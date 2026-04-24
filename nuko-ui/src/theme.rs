//! UIテーマ定義

use iced::Color;

/// UIテーマ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// ライトテーマ
    Light,
    /// ダークテーマ
    Dark,
}

impl Theme {
    /// 背景色を取得
    #[must_use]
    pub fn background(&self) -> Color {
        match self {
            Theme::Light => Color::from_rgb(1.0, 1.0, 1.0),
            Theme::Dark => Color::from_rgb(0.15, 0.15, 0.15),
        }
    }

    /// テキスト色を取得
    #[must_use]
    pub fn text(&self) -> Color {
        match self {
            Theme::Light => Color::from_rgb(0.1, 0.1, 0.1),
            Theme::Dark => Color::from_rgb(0.9, 0.9, 0.9),
        }
    }

    /// 選択中のアイテムの背景色を取得
    #[must_use]
    pub fn selected_background(&self) -> Color {
        match self {
            Theme::Light => Color::from_rgb(0.2, 0.5, 0.8),
            Theme::Dark => Color::from_rgb(0.3, 0.5, 0.7),
        }
    }

    /// 選択中のアイテムのテキスト色を取得
    #[must_use]
    pub fn selected_text(&self) -> Color {
        Color::from_rgb(1.0, 1.0, 1.0)
    }

    /// ボーダー色を取得
    #[must_use]
    pub fn border(&self) -> Color {
        match self {
            Theme::Light => Color::from_rgb(0.8, 0.8, 0.8),
            Theme::Dark => Color::from_rgb(0.4, 0.4, 0.4),
        }
    }

    /// セカンダリテキスト色を取得
    #[must_use]
    pub fn secondary_text(&self) -> Color {
        match self {
            Theme::Light => Color::from_rgb(0.5, 0.5, 0.5),
            Theme::Dark => Color::from_rgb(0.6, 0.6, 0.6),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Light
    }
}
