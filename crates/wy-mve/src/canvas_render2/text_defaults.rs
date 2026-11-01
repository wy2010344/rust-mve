//! 核心中性文本默认色（对标 TS `canvasRender2/TextDefaults.ts`）。
//!
//! [`text_color`] / [`placeholder_color`] 是 `WrappedTextNode`/`RichTextNode` 默认色的取值
//! 来源。核心包不感知具体组件主题（Theme 属组件包）：默认值取浅色主题数值，
//! 与迁移前逐像素等价；组件包切换主题时经 setter 回写同步，昼/夜切换自动换肤。

use std::cell::RefCell;
use wy_render::Color;
use wy_signal::{GetValue, SetValue, Signal};

thread_local! {
    static TEXT: RefCell<Signal<Color>> = RefCell::new(Signal::new(Color::rgb(30, 41, 59)));
    static PLACEHOLDER: RefCell<Signal<Color>> =
        RefCell::new(Signal::new(Color::rgb(148, 163, 184)));
}

/// 正文默认色（组件主题 `text`）。
#[must_use]
pub fn text_color() -> Color {
    TEXT.with(|s| s.borrow().get())
}

/// 回写正文默认色（组件主题 `text`；信号驱动，读取方随重绘更新）。
pub fn set_text_color(value: Color) {
    TEXT.with(|s| s.borrow().set(value));
}

/// 占位符默认色（组件主题 `placeholder`）。
#[must_use]
pub fn placeholder_color() -> Color {
    PLACEHOLDER.with(|s| s.borrow().get())
}

/// 回写占位符默认色（组件主题 `placeholder`）。
pub fn set_placeholder_color(value: Color) {
    PLACEHOLDER.with(|s| s.borrow().set(value));
}
