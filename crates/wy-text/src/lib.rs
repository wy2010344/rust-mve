//! 文本排版：基于 Parley 的富文本 shaping + 布局 + 缓存。
//!
//! 核心类型：
//! - [`FontContext`] — 全局字体上下文（封装 Parley FontContext + LayoutContext）
//! - [`TextStyle`] / [`TextSpan`] — 文本样式与片段（对应 Kotlin `RichTextStyle`/`RichTextSpan`）
//! - [`TextParagraph`] — 排版结果（对应 Kotlin `PlatformParagraph`）
//! - [`build_paragraph`] — 排版入口函数
//! - [`EditableParagraph`] — 可编辑段落（封装 Parley PlainEditor）

mod build_paragraph;
mod edit_core;
pub mod editable;
mod editing;
mod font_cache;
mod text_paragraph;
mod text_style;

pub use build_paragraph::{build_paragraph, TextError};
pub use edit_core::EditCore;
pub use editable::{EditSnapshot, EditableParagraph, Rect4};
pub use editing::{TextBuffer, TextSegment};
pub use font_cache::FontContext;
pub use text_paragraph::TextParagraph;
pub use text_style::{
    LineMetric, RectStyle, TextAlign, TextDecoration, TextRect, TextSpan, TextStyle,
};
