//! shadcn zinc 设计 token（浅/深两套）。

use wy_render::Color;
use wy_signal::{GetValue, Signal};

/// 响应式主题句柄：持有浅/深切换信号，`tokens()` 在**绘制期**求值。
///
/// 节点树只构建一次，颜色必须在 draw 期读信号才能随主题切换重绘；
/// 若在构建期读取（`render_root` 的 callback 只执行一次）会永久烤死。
#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: Signal<bool>,
}

impl Theme {
    pub fn new(dark: Signal<bool>) -> Self {
        Self { dark }
    }

    /// 当前主题的 token（读 `dark` 信号 → 被绘制追踪 → 切换自动重绘）。
    pub fn tokens(&self) -> Tokens {
        if self.dark.get() {
            Tokens::DARK
        } else {
            Tokens::LIGHT
        }
    }
}

#[derive(Clone, Copy)]
pub struct Tokens {
    pub background: Color,
    pub foreground: Color,
    pub card: Color,
    pub muted: Color,
    pub muted_fg: Color,
    pub border: Color,
    pub input_bg: Color,
    pub primary: Color,
    pub primary_fg: Color,
    pub destructive: Color,
    pub destructive_fg: Color,
    pub accent: Color,
    pub ring: Color,
}

impl Tokens {
    pub const LIGHT: Self = Self {
        background: Color::rgb(250, 250, 250),
        foreground: Color::rgb(9, 9, 11),
        card: Color::WHITE,
        muted: Color::rgb(244, 244, 245),
        muted_fg: Color::rgb(113, 113, 122),
        border: Color::rgb(228, 228, 231),
        input_bg: Color::WHITE,
        primary: Color::rgb(24, 24, 27),
        primary_fg: Color::WHITE,
        destructive: Color::rgb(220, 38, 38),
        destructive_fg: Color::WHITE,
        accent: Color::rgb(244, 244, 245),
        ring: Color::rgb(24, 24, 27),
    };

    pub const DARK: Self = Self {
        background: Color::rgb(9, 9, 11),
        foreground: Color::rgb(250, 250, 250),
        card: Color::rgb(24, 24, 27),
        muted: Color::rgb(39, 39, 42),
        muted_fg: Color::rgb(161, 161, 170),
        border: Color::rgb(39, 39, 42),
        input_bg: Color::rgb(24, 24, 27),
        primary: Color::rgb(250, 250, 250),
        primary_fg: Color::rgb(24, 24, 27),
        destructive: Color::rgb(220, 38, 38),
        destructive_fg: Color::WHITE,
        accent: Color::rgb(39, 39, 42),
        ring: Color::rgb(250, 250, 250),
    };
}
