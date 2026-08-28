//! 文本排版：基于 Parley 的富文本 shaping + 布局 + 缓存。
//!
//! 核心类型：
//! - [`FontContext`] — 全局字体上下文（封装 Parley FontContext + LayoutContext）
//! - [`TextStyle`] / [`TextSpan`] — 文本样式与片段（对应 Kotlin `RichTextStyle`/`RichTextSpan`）
//! - [`TextParagraph`] — 排版结果（对应 Kotlin `PlatformParagraph`）
//! - [`build_paragraph`] — 排版入口函数

mod build_paragraph;
mod font_cache;
mod text_paragraph;
mod text_style;

pub use build_paragraph::{build_paragraph, TextError};
pub use font_cache::FontContext;
pub use text_paragraph::TextParagraph;
pub use text_style::{LineMetric, TextAlign, TextDecoration, TextRect, TextSpan, TextStyle};
