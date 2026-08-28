//! 排版入口：`build_paragraph` — 把文本 + 样式 → 排版结果。

use crate::font_cache::FontContext;
use crate::text_paragraph::TextParagraph;
use crate::text_style::{LineMetric, TextAlign, TextSpan};

/// 排版错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError {
    message: String,
}

impl TextError {
    /// 构造错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 返回错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TextError {}

/// 将 [`TextAlign`] 转换为 Parley 的 [`parley::Alignment`]。
fn to_parley_alignment(align: TextAlign) -> parley::Alignment {
    match align {
        TextAlign::Start => parley::Alignment::Start,
        TextAlign::Center => parley::Alignment::Center,
        TextAlign::End => parley::Alignment::End,
        TextAlign::Justify => parley::Alignment::Justify,
    }
}

/// 将 RGBA u32 颜色转换为 `[u8; 4]`（Parley 0.11 的 Brush 类型）。
fn color_to_brush(rgba: u32) -> [u8; 4] {
    [
        ((rgba >> 24) & 0xFF) as u8,
        ((rgba >> 16) & 0xFF) as u8,
        ((rgba >> 8) & 0xFF) as u8,
        (rgba & 0xFF) as u8,
    ]
}

/// 将 [`TextSpan`] 的样式推送到 Parley builder。
fn push_span_styles(builder: &mut parley::RangedBuilder<'_, [u8; 4]>, span: &TextSpan) {
    use parley::StyleProperty;

    let s = &span.style;
    builder.push_default(StyleProperty::FontSize(s.font_size));
    builder.push_default(StyleProperty::FontWeight(parley::FontWeight::new(
        s.font_weight as f32,
    )));
    if s.italic {
        builder.push_default(StyleProperty::FontStyle(parley::FontStyle::Italic));
    }
    builder.push_default(StyleProperty::Brush(color_to_brush(s.color)));
    if s.letter_spacing != 0.0 {
        builder.push_default(StyleProperty::LetterSpacing(s.letter_spacing));
    }
    if s.word_spacing != 0.0 {
        builder.push_default(StyleProperty::WordSpacing(s.word_spacing));
    }
    if let Some(lh) = s.line_height_multiplier {
        builder.push_default(StyleProperty::LineHeight(
            parley::LineHeight::FontSizeRelative(lh),
        ));
    }
    if s.decoration.underline {
        builder.push_default(StyleProperty::Underline(true));
    }
    if s.decoration.line_through {
        builder.push_default(StyleProperty::Strikethrough(true));
    }
}

/// 构建文本段落：排版 + 换行 + 对齐。
///
/// 对应 Kotlin `buildParagraph()`。输入文本片段列表、最大宽度、最大行数、
/// 省略号、对齐方式，输出 [`TextParagraph`]。
///
/// # 参数
/// - `font_cx` — 全局字体上下文（可变引用，Parley 需要）。
/// - `spans` — 文本片段列表（按顺序拼接）。
/// - `max_width` — 最大宽度（`None` 表示不换行）。
/// - `max_lines` — 最大行数（超出部分截断）。
/// - `text_align` — 对齐方式。
pub fn build_paragraph(
    font_cx: &mut FontContext,
    spans: &[TextSpan],
    max_width: Option<f32>,
    max_lines: usize,
    text_align: TextAlign,
) -> Result<TextParagraph, TextError> {
    if spans.is_empty() {
        return Ok(TextParagraph::new(0.0, 0.0, vec![], String::new()));
    }

    // 拼接全部文本
    let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();

    // 创建 builder（分别借用 layout_cx 和 font_cx，避免双重借用）
    let display_scale = 1.0;
    let mut builder =
        font_cx
            .layout_cx
            .ranged_builder(&mut font_cx.font_cx, &full_text, display_scale, false);

    // 设置全局默认样式（用第一个 span 的样式）
    push_span_styles(&mut builder, &spans[0]);

    // 构建 layout
    let mut layout: parley::Layout<[u8; 4]> = builder.build(&full_text);

    // 换行
    layout.break_all_lines(max_width);

    // 对齐
    layout.align(
        to_parley_alignment(text_align),
        parley::AlignmentOptions::default(),
    );

    // 提取尺寸
    let width = layout.width();
    let height = layout.height();

    // 提取行度量
    let mut line_metrics = Vec::new();
    for line in layout.lines() {
        let run_range = line.text_range();
        line_metrics.push(LineMetric {
            start_index: run_range.start,
            end_index: run_range.end,
        });
    }

    // 截断到 max_lines
    if line_metrics.len() > max_lines {
        line_metrics.truncate(max_lines);
    }

    Ok(TextParagraph::new(width, height, line_metrics, full_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_style::{TextSpan, TextStyle};

    #[test]
    fn build_empty_spans() {
        let mut fc = FontContext::new();
        let p = build_paragraph(&mut fc, &[], None, 100, TextAlign::Start).unwrap();
        assert_eq!(p.width(), 0.0);
        assert_eq!(p.height(), 0.0);
        assert!(p.line_metrics().is_empty());
    }

    #[test]
    fn build_simple_text() {
        let mut fc = FontContext::new();
        let spans = vec![TextSpan::styled(
            "Hello, world!",
            TextStyle::normal().with_font_size(16.0),
        )];
        let p = build_paragraph(&mut fc, &spans, Some(500.0), 100, TextAlign::Start).unwrap();
        assert!(p.width() > 0.0);
        assert!(p.height() > 0.0);
        assert!(!p.line_metrics().is_empty());
        assert_eq!(p.text(), "Hello, world!");
    }

    #[test]
    fn build_wraps_long_text() {
        let mut fc = FontContext::new();
        let text = "This is a long text that should wrap at some point when the max width is small enough to force line breaking.";
        let spans = vec![TextSpan::styled(
            text,
            TextStyle::normal().with_font_size(16.0),
        )];
        let p = build_paragraph(&mut fc, &spans, Some(100.0), 100, TextAlign::Start).unwrap();
        assert!(
            p.line_metrics().len() > 1,
            "长文本应该换行，实际行数: {}",
            p.line_metrics().len()
        );
    }

    #[test]
    fn build_with_different_alignments() {
        let mut fc = FontContext::new();
        let spans = vec![TextSpan::styled("test", TextStyle::normal())];
        let p_start = build_paragraph(&mut fc, &spans, Some(200.0), 100, TextAlign::Start).unwrap();
        let p_center =
            build_paragraph(&mut fc, &spans, Some(200.0), 100, TextAlign::Center).unwrap();
        assert!(p_start.width() > 0.0);
        assert!(p_center.width() > 0.0);
    }

    #[test]
    fn build_with_multiple_spans() {
        let mut fc = FontContext::new();
        let spans = vec![
            TextSpan::styled("Hello ", TextStyle::normal().with_font_size(16.0)),
            TextSpan::styled(
                "world",
                TextStyle::normal()
                    .with_font_size(24.0)
                    .with_font_weight(700),
            ),
        ];
        let p = build_paragraph(&mut fc, &spans, Some(500.0), 100, TextAlign::Start).unwrap();
        assert!(p.width() > 0.0);
        assert_eq!(p.text(), "Hello world");
    }
}
