//! Theme：主题配置（颜色、尺寸）。

use crate::Color;

/// 主题配置：统一管理颜色和尺寸。
///
/// ```ignore
/// use wy_render::theme::Theme;
///
/// let theme = Theme::light();
/// assert_eq!(theme.colors.background, Color::WHITE);
/// ```
pub struct Theme {
    /// 颜色配置。
    pub colors: Colors,
    /// 尺寸配置。
    pub sizes: Sizes,
}

/// 主题颜色。
pub struct Colors {
    /// 窗口/应用背景色。
    pub background: Color,
    /// 主文本颜色。
    pub text: Color,
    /// 次要文本颜色（占位符、禁用态）。
    pub text_secondary: Color,
    /// 主色调（按钮、链接、焦点边框）。
    pub primary: Color,
    /// 次要色调（悬停态）。
    pub primary_hover: Color,
    /// 边框颜色。
    pub border: Color,
    /// 输入框背景色。
    pub input_background: Color,
    /// 按钮背景色。
    pub button_background: Color,
    /// 按钮悬停背景色。
    pub button_hover_background: Color,
    /// 按钮文本颜色。
    pub button_text: Color,
}

/// 主题尺寸。
pub struct Sizes {
    /// 默认字体大小。
    pub font_size: f32,
    /// 小字体大小。
    pub font_size_small: f32,
    /// 内边距。
    pub padding: f32,
    /// 元素间距。
    pub spacing: f32,
    /// 圆角半径。
    pub border_radius: f32,
}

impl Theme {
    /// 浅色主题。
    pub fn light() -> Self {
        Self {
            colors: Colors {
                background: Color::WHITE,
                text: Color::BLACK,
                text_secondary: Color::rgba(160, 160, 160, 255),
                primary: Color::rgba(0, 120, 212, 255),
                primary_hover: Color::rgba(0, 100, 192, 255),
                border: Color::rgba(200, 200, 200, 255),
                input_background: Color::WHITE,
                button_background: Color::rgba(230, 230, 230, 255),
                button_hover_background: Color::rgba(210, 210, 210, 255),
                button_text: Color::BLACK,
            },
            sizes: Sizes {
                font_size: 14.0,
                font_size_small: 12.0,
                padding: 8.0,
                spacing: 8.0,
                border_radius: 4.0,
            },
        }
    }

    /// 深色主题。
    pub fn dark() -> Self {
        Self {
            colors: Colors {
                background: Color::rgba(30, 30, 30, 255),
                text: Color::rgba(240, 240, 240, 255),
                text_secondary: Color::rgba(120, 120, 120, 255),
                primary: Color::rgba(80, 160, 240, 255),
                primary_hover: Color::rgba(100, 180, 255, 255),
                border: Color::rgba(60, 60, 60, 255),
                input_background: Color::rgba(45, 45, 45, 255),
                button_background: Color::rgba(50, 50, 50, 255),
                button_hover_background: Color::rgba(65, 65, 65, 255),
                button_text: Color::rgba(240, 240, 240, 255),
            },
            sizes: Sizes {
                font_size: 14.0,
                font_size_small: 12.0,
                padding: 8.0,
                spacing: 8.0,
                border_radius: 4.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_theme_defaults() {
        let t = Theme::light();
        assert_eq!(t.colors.background, Color::WHITE);
        assert_eq!(t.colors.text, Color::BLACK);
        assert_eq!(t.sizes.font_size, 14.0);
        assert_eq!(t.sizes.border_radius, 4.0);
    }

    #[test]
    fn dark_theme_defaults() {
        let t = Theme::dark();
        assert_eq!(t.colors.background, Color::rgba(30, 30, 30, 255));
        assert_eq!(t.colors.text, Color::rgba(240, 240, 240, 255));
    }

    #[test]
    fn theme_colors_are_distinct() {
        let t = Theme::light();
        assert_ne!(t.colors.background, t.colors.text);
        assert_ne!(t.colors.button_background, t.colors.button_hover_background);
    }
}
