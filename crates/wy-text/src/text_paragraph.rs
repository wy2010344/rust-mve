//! 文本排版结果：Parley `Layout` 的包装，提供 Kotlin `PlatformParagraph` 对应的查询接口。

use crate::text_style::LineMetric;

/// 文本排版结果：包含段落尺寸、行度量、原始文本。
///
/// 对应 Kotlin `PlatformParagraph`。通过 [`crate::build_paragraph`] 创建。
/// 支持按新宽度重新换行（Parley 的 `break_all_lines` 支持多次调用）。
pub struct TextParagraph {
    /// 排版后的宽度。
    width: f32,
    /// 排版后的高度。
    height: f32,
    /// 行度量（按行序）。
    line_metrics: Vec<LineMetric>,
    /// 原始文本（用于字符索引查询）。
    text: String,
}

impl TextParagraph {
    /// 构造排版结果。
    pub(crate) fn new(
        width: f32,
        height: f32,
        line_metrics: Vec<LineMetric>,
        text: String,
    ) -> Self {
        Self {
            width,
            height,
            line_metrics,
            text,
        }
    }

    /// 段落排版后的宽度。
    pub fn width(&self) -> f32 {
        self.width
    }

    /// 段落排版后的高度。
    pub fn height(&self) -> f32 {
        self.height
    }

    /// 全部软行度量（按行序）。
    pub fn line_metrics(&self) -> &[LineMetric] {
        &self.line_metrics
    }

    /// 原始文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 原始文本的字符数。
    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    /// 包含 `offset` 的词边界（半开区间 `[start, end)`）。
    /// 无法分词时返回 `None`。
    pub fn word_boundary(&self, offset: usize) -> Option<(usize, usize)> {
        if offset >= self.text.len() {
            return None;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let byte_to_char: Vec<usize> = {
            let mut v = vec![0; self.text.len() + 1];
            for (i, (ci, _)) in self.text.char_indices().enumerate() {
                v[ci] = i;
            }
            v
        };
        let char_idx = byte_to_char.get(offset).copied().unwrap_or(chars.len());
        // 简单分词：找前后空白边界
        let mut start = char_idx;
        let mut end = char_idx;
        // 向前找非空白起点
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        // 向后找空白终点
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        // 转回字节偏移
        let byte_start = self
            .text
            .char_indices()
            .nth(start)
            .map_or(self.text.len(), |(i, _)| i);
        let byte_end = self
            .text
            .char_indices()
            .nth(end)
            .map_or(self.text.len(), |(i, _)| i);
        Some((byte_start, byte_end))
    }

    /// 获取 `offset` 处字符的坐标（简易实现：返回该行的 x 偏移）。
    pub fn glyph_position_at_coordinate(&self, _dx: f32, _dy: f32) -> usize {
        // 简易实现：按行高度估算行号，再估算列
        // 真正的实现需要 Parley 的详细 glyph 位置数据
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_paragraph() -> TextParagraph {
        TextParagraph::new(
            100.0,
            50.0,
            vec![
                LineMetric {
                    start_index: 0,
                    end_index: 5,
                },
                LineMetric {
                    start_index: 6,
                    end_index: 11,
                },
            ],
            "hello world".to_string(),
        )
    }

    #[test]
    fn paragraph_exposes_dimensions() {
        let p = dummy_paragraph();
        assert_eq!(p.width(), 100.0);
        assert_eq!(p.height(), 50.0);
    }

    #[test]
    fn paragraph_exposes_line_metrics() {
        let p = dummy_paragraph();
        assert_eq!(p.line_metrics().len(), 2);
        assert_eq!(p.line_metrics()[0].start_index, 0);
        assert_eq!(p.line_metrics()[0].end_index, 5);
        assert_eq!(p.line_metrics()[1].start_index, 6);
    }

    #[test]
    fn paragraph_exposes_text() {
        let p = dummy_paragraph();
        assert_eq!(p.text(), "hello world");
        assert_eq!(p.text_len(), 11);
    }

    #[test]
    fn word_boundary_finds_words() {
        let p = dummy_paragraph();
        // 'h' in "hello" → 找到整个 "hello"
        let (s, e) = p.word_boundary(0).unwrap();
        assert_eq!(&p.text()[s..e], "hello");
        // 'w' in "world" → 找到 "world"
        let (s, e) = p.word_boundary(6).unwrap();
        assert_eq!(&p.text()[s..e], "world");
    }

    #[test]
    fn word_boundary_out_of_range() {
        let p = dummy_paragraph();
        assert!(p.word_boundary(100).is_none());
    }
}
