//! 排版入口：`build_paragraph` — 把文本 + 样式 → 排版结果。

use crate::font_cache::FontContext;
use crate::text_paragraph::TextParagraph;
use crate::text_style::{TextAlign, TextSpan};

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

/// 将 [`TextSpan`] 的样式作为全局默认样式推送到 Parley builder。
fn push_span_defaults(builder: &mut parley::RangedBuilder<'_, [u8; 4]>, span: &TextSpan) {
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

/// 将 [`TextSpan`] 的样式应用到指定字节区间（覆盖全局默认）。
fn push_span_range(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    range: core::ops::Range<usize>,
    span: &TextSpan,
) {
    use parley::StyleProperty;

    let s = &span.style;
    builder.push(StyleProperty::FontSize(s.font_size), range.clone());
    builder.push(
        StyleProperty::FontWeight(parley::FontWeight::new(s.font_weight as f32)),
        range.clone(),
    );
    if s.italic {
        builder.push(
            StyleProperty::FontStyle(parley::FontStyle::Italic),
            range.clone(),
        );
    }
    builder.push(StyleProperty::Brush(color_to_brush(s.color)), range.clone());
    if s.letter_spacing != 0.0 {
        builder.push(
            StyleProperty::LetterSpacing(s.letter_spacing),
            range.clone(),
        );
    }
    if s.word_spacing != 0.0 {
        builder.push(StyleProperty::WordSpacing(s.word_spacing), range.clone());
    }
    if let Some(lh) = s.line_height_multiplier {
        builder.push(
            StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(lh)),
            range.clone(),
        );
    }
    if s.decoration.underline {
        builder.push(StyleProperty::Underline(true), range.clone());
    }
    if s.decoration.line_through {
        builder.push(StyleProperty::Strikethrough(true), range);
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
/// - `max_lines` — 最大行数（`usize::MAX` / `0` 表示不限；超出时按 `ellipsis` 截断）。
/// - `ellipsis` — 省略号（截断时追加到可见文本末尾）。
/// - `text_align` — 对齐方式。
///
/// 对标 TS `buildParagraph(spans, width, maxLines, ellipsis, align)`：Parley 没有
/// `maxLines` 原语，因此截断在文本层做——先排出完整段落，若行数超限，就把
/// 「第 `max_lines` 行的起始偏移」作为上界，二分出**最长的可见前缀**，再拼上
/// 省略号重排。二分次数是 `log2(字节数)`，且结果被上层排版缓存持有，不会每帧重算。
pub fn build_paragraph(
    font_cx: &mut FontContext,
    spans: &[TextSpan],
    max_width: Option<f32>,
    max_lines: usize,
    ellipsis: &str,
    text_align: TextAlign,
) -> Result<TextParagraph, TextError> {
    if spans.is_empty() {
        let empty: parley::Layout<[u8; 4]> = parley::Layout::new();
        return Ok(TextParagraph::new(empty, String::new()));
    }

    let full = build_once(font_cx, spans, max_width, text_align)?;
    if max_lines == 0 || max_lines == usize::MAX || full.line_metrics().len() <= max_lines {
        return Ok(full);
    }

    // 第 `max_lines` 行的起点 = 第一个不可见字符（行索引 0-based）。
    let total: usize = spans.iter().map(|s| s.text.len()).sum();
    let cut = floor_char_boundary(spans, full.line_metrics()[max_lines].start_index.min(total));

    // 二分：最大的 `len` 使得 `spans[..len] + ellipsis` 仍然不超过 `max_lines` 行。
    let (mut lo, mut hi) = (0usize, cut);
    let mut best: Option<TextParagraph> = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let at = floor_char_boundary(spans, mid);
        let truncated = truncate_spans(spans, at, ellipsis);
        let p = build_once(font_cx, &truncated, max_width, text_align)?;
        if p.line_metrics().len() <= max_lines {
            best = Some(p);
            if at >= hi {
                break;
            }
            lo = at + 1;
        } else if at == 0 {
            break;
        } else {
            hi = at - 1;
        }
    }
    // 极端情况（连一个字符加省略号都超行）也至少返回省略号本身。
    Ok(best.unwrap_or_else(|| {
        let only = truncate_spans(spans, 0, ellipsis);
        build_once(font_cx, &only, max_width, text_align)
            .unwrap_or_else(|_| build_once(font_cx, &only, None, text_align).unwrap())
    }))
}

/// 单次排版（不截断）：`buildParagraph` 的实际排版步骤。
fn build_once(
    font_cx: &mut FontContext,
    spans: &[TextSpan],
    max_width: Option<f32>,
    text_align: TextAlign,
) -> Result<TextParagraph, TextError> {
    if spans.is_empty() {
        let empty: parley::Layout<[u8; 4]> = parley::Layout::new();
        return Ok(TextParagraph::new(empty, String::new()));
    }

    // 拼接全部文本
    let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();

    // 创建 builder（分别借用 layout_cx 和 font_cx，避免双重借用）
    let display_scale = 1.0;
    let mut builder =
        font_cx
            .layout_cx
            .ranged_builder(&mut font_cx.font_cx, &full_text, display_scale, false);

    // root 样式用第一个 span，各 span 再按区间精确覆盖
    push_span_defaults(&mut builder, &spans[0]);
    let mut offset = 0usize;
    for span in spans {
        let len = span.text.len();
        if len > 0 {
            push_span_range(&mut builder, offset..offset + len, span);
        }
        offset += len;
    }

    // 构建 layout
    let mut layout: parley::Layout<[u8; 4]> = builder.build(&full_text);

    // 换行
    layout.break_all_lines(max_width);

    // 对齐
    layout.align(
        to_parley_alignment(text_align),
        parley::AlignmentOptions::default(),
    );

    Ok(TextParagraph::new(layout, full_text))
}

/// 把字节偏移吸附到最近的字符边界（向下）。
fn floor_char_boundary(spans: &[TextSpan], offset: usize) -> usize {
    let total: usize = spans.iter().map(|s| s.text.len()).sum();
    let off = offset.min(total);
    // 逐 span 定位 offset 落在哪个片段，再在该片段内吸附。
    let mut base = 0usize;
    for span in spans {
        let end = base + span.text.len();
        if off < end {
            let local = off - base;
            let mut l = local;
            while l > 0 && !span.text.is_char_boundary(l) {
                l -= 1;
            }
            return base + l;
        }
        if off == end {
            return end;
        }
        base = end;
    }
    base
}

/// 取 `spans` 的前 `len` 字节，并在末尾追加 `ellipsis`。
fn truncate_spans(spans: &[TextSpan], len: usize, ellipsis: &str) -> Vec<TextSpan> {
    let mut out: Vec<TextSpan> = Vec::new();
    let mut rem = len;
    for span in spans {
        if rem == 0 {
            break;
        }
        let take = span.text.len().min(rem);
        if take == 0 {
            continue;
        }
        out.push(TextSpan {
            text: span.text[..take].to_string(),
            style: span.style.clone(),
        });
        rem -= take;
    }
    if let Some(last) = out.last_mut() {
        last.text.push_str(ellipsis);
    } else if !ellipsis.is_empty() {
        out.push(TextSpan {
            text: ellipsis.to_string(),
            style: spans[0].style.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_style::{TextSpan, TextStyle};

    #[test]
    fn build_empty_spans() {
        let mut fc = FontContext::new();
        let p = build_paragraph(&mut fc, &[], None, 100, "\u{2026}", TextAlign::Start).unwrap();
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
        let p = build_paragraph(
            &mut fc,
            &spans,
            Some(500.0),
            100,
            "\u{2026}",
            TextAlign::Start,
        )
        .unwrap();
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
        let p = build_paragraph(
            &mut fc,
            &spans,
            Some(100.0),
            100,
            "\u{2026}",
            TextAlign::Start,
        )
        .unwrap();
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
        let p_start = build_paragraph(
            &mut fc,
            &spans,
            Some(200.0),
            100,
            "\u{2026}",
            TextAlign::Start,
        )
        .unwrap();
        let p_center = build_paragraph(
            &mut fc,
            &spans,
            Some(200.0),
            100,
            "\u{2026}",
            TextAlign::Center,
        )
        .unwrap();
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
        let p = build_paragraph(
            &mut fc,
            &spans,
            Some(500.0),
            100,
            "\u{2026}",
            TextAlign::Start,
        )
        .unwrap();
        assert!(p.width() > 0.0);
        assert_eq!(p.text(), "Hello world");
    }
}
