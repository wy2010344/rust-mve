//! 文本排版：基于 Parley 的富文本 shaping + 布局 + 缓存。
//!
//! 核心类型：
//! - [`FontContext`] — 全局字体上下文（封装 Parley FontContext + LayoutContext）
//! - [`TextStyle`] / [`TextSpan`] — 文本样式与片段（对应 Kotlin `RichTextStyle`/`RichTextSpan`）
//! - [`TextParagraph`] — 排版结果（对应 Kotlin `PlatformParagraph`）
//! - [`build_paragraph`] — 排版入口函数
//! - [`EditableParagraph`] — 可编辑段落（封装 Parley PlainEditor）
//!
//! # 编辑内核：`EditCore` 是唯一真相源
//!
//! | 层 | 归谁 |
//! |---|---|
//! | 文本 / 光标 / 选区 / undo / IME 组合态 / 掩码显示映射 | 本 crate 的 [`EditCore`] |
//! | 段落缓存与排版、光标矩形、垂直移动、命中、绘制、闪烁、跨组件选区 | `wy-mve::canvas_render2::editable_text_node` |
//!
//! 判据只有一条：**纯逻辑层给不出答案的才留在组件**。垂直移动是典型例子——
//! `move_to_line` 要行盒，所以行号解析留在组件，内核只负责"第几行第几列"。
//!
//! 历史：这套内核曾是 Kotlin `EditableTextNode` 的逐条复刻、**零调用**，而组件层
//! 自己又写了一份（`caret_pair` / `composing_text` / `undo_redo` / `preferred_x`
//! 六个字段各管一摊）。2026-11-01 已完成统一，组件层不再持有编辑状态；
//! 审计记录见 `docs/05-canvasRender2对标审计.md` §15。
//!
//! [`EditCore`] 内部用 [`TextBuffer`]（`editing` 模块）存文本与样式段边界表——
//! 它是内核的存储层，不是并列的第二实现。另一条线 [`EditableParagraph`] 是
//! Parley `PlainEditor` 那一派的封装，目前没有编辑框在走它；删它要先确认没有
//! 替代品，不能只看引用数。

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
