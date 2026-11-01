//! 文本排版：基于 Parley 的富文本 shaping + 布局 + 缓存。
//!
//! 核心类型：
//! - [`FontContext`] — 全局字体上下文（封装 Parley FontContext + LayoutContext）
//! - [`TextStyle`] / [`TextSpan`] — 文本样式与片段（对应 Kotlin `RichTextStyle`/`RichTextSpan`）
//! - [`TextParagraph`] — 排版结果（对应 Kotlin `PlatformParagraph`）
//! - [`build_paragraph`] — 排版入口函数
//! - [`EditableParagraph`] — 可编辑段落（封装 Parley PlainEditor）
//!
//! # 两套编辑内核（现状说明，清理代码前必读）
//!
//! | 实现 | 状态 |
//! |---|---|
//! | `wy-mve::canvas_render2::editable_text_node` | **在用**：节点自带光标/选区/IME/剪贴板，编辑框组件就是它 |
//! | 本 crate 的 [`EditCore`] / [`EditableParagraph`] / [`TextBuffer`] | **未接线**：Kotlin `EditableTextNode` 的逐条复刻，74 条测试（字素/词边界、IME 组合、obscure 掩码、样式段边界表） |
//!
//! 后者没有任何调用方，但它**不是死代码**：它是移植进度的记录，也是把编辑器
//! 切到统一内核时的落点。判断"能否删"要看它有没有替代品，不能只看引用数。

mod build_paragraph;
mod edit_core;
pub mod editable;
mod editing;
mod font_cache;
mod text_paragraph;
mod text_style;

pub use build_paragraph::{build_paragraph, TextError};
pub use edit_core::{grapheme_next, grapheme_prev, word_next, word_prev, EditCore};
pub use editable::{EditSnapshot, EditableParagraph, Rect4};
pub use editing::{TextBuffer, TextSegment};
pub use font_cache::FontContext;
pub use text_paragraph::TextParagraph;
pub use text_style::{
    LineMetric, RectStyle, TextAlign, TextDecoration, TextRect, TextSpan, TextStyle,
};
