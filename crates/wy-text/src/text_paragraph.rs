//! 文本排版结果：Parley `Layout` 的包装，提供 Kotlin `PlatformParagraph` 对应的查询接口。

use core::ops::Range;

use parley::layout::{Cluster, ClusterSide};

use crate::text_style::{LineMetric, RectStyle, TextRect};

/// 文本排版结果：包含段落尺寸、行度量、原始文本与命中几何。
///
/// 对应 Kotlin `PlatformParagraph`。通过 [`crate::build_paragraph`] 创建。
/// 内部持有 Parley `Layout`，支持：
/// - [`Self::glyph_position_at_coordinate`]：坐标 → 字符字节偏移（对应 `getGlyphPositionAtCoordinate`）
/// - [`Self::rects_for_range`]：选区 → 矩形列表（对应 `getRectsForRange`）
/// - [`Self::line_metrics`]：行度量（对应 `getLineMetrics`）
///
/// **索引单位**：与 Parley 一致用**字节偏移**（字符起点的字节位），
/// 对应 Kotlin 侧的 UTF-16 偏移（对 BMP 字符二者数值一致）。
pub struct TextParagraph {
    /// 排版内部表示（换行 + 对齐后）。
    layout: parley::Layout<[u8; 4]>,
    /// 原始文本（`Layout` 不保存文本内容，需随段落携带）。
    text: String,
}

impl TextParagraph {
    /// 构造排版结果。
    pub(crate) fn new(layout: parley::Layout<[u8; 4]>, text: String) -> Self {
        Self { layout, text }
    }

    /// 段落排版后的宽度。
    pub fn width(&self) -> f32 {
        self.layout.width()
    }

    /// 段落排版后的高度。
    pub fn height(&self) -> f32 {
        self.layout.height()
    }

    /// 全部软行度量（按行序）。
    ///
    /// `start_index`/`end_index` 为**字节偏移**。
    pub fn line_metrics(&self) -> Vec<LineMetric> {
        self.layout
            .lines()
            .map(|line| {
                let r = line.text_range();
                LineMetric {
                    start_index: r.start,
                    end_index: r.end,
                }
            })
            .collect()
    }

    /// 原始文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 原始文本的字符数（Unicode 标量数）。
    pub fn text_len(&self) -> usize {
        self.text.chars().count()
    }

    /// 原始文本的字数（按空白切分）。
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    /// 坐标 → 字符字节偏移（对应 Kotlin `getGlyphPositionAtCoordinate`）。
    ///
    /// 命中字符左侧一半返回该字节起点，右侧一半返回下一个字节起点；
    /// 命中行尾部空白返回行尾字节。点位于文本范围外时：
    /// - `dy` 在首行上方 → 返回 `0`；
    /// - 其他情况（下方/右侧） → 返回文本末尾字节。
    pub fn glyph_position_at_coordinate(&self, dx: f32, dy: f32) -> usize {
        if self.text.is_empty() {
            return 0;
        }
        if let Some((cluster, side)) = Cluster::from_point(&self.layout, dx, dy) {
            let range = cluster.text_range();
            return match side {
                ClusterSide::Left => range.start,
                ClusterSide::Right => range.end,
            };
        }
        // 未命中（点在布局外）：dy 上方 → 0，否则文末
        if dy < 0.0 {
            0
        } else {
            self.text.len()
        }
    }

    /// 选区 `[start, end)`（**字节偏移**）对应的矩形列表（Kotlin `getRectsForRange`）。
    ///
    /// `Tight`：每条相交行返回一个紧贴字形宽度的矩形；
    /// `Full`：每条相交行返回整行全宽矩形。
    pub fn rects_for_range(&self, start: usize, end: usize, style: RectStyle) -> Vec<TextRect> {
        if start >= end {
            return Vec::new();
        }
        let mut out = Vec::new();
        for line in self.layout.lines() {
            let m = line.metrics();
            let line_range = line.text_range();
            let ls = start.max(line_range.start);
            let le = end.min(line_range.end);
            if ls >= le {
                continue;
            }
            if style == RectStyle::Tight {
                // 求该行内与选区相交簇的最小/最大 x
                let (mut xlo_opt, mut xhi_opt) = (None, None);
                for run in line.runs() {
                    for cluster in run.clusters() {
                        let c = cluster.text_range();
                        if c.end <= ls || c.start >= le {
                            continue;
                        }
                        if let Some(x) = cluster.visual_offset() {
                            let x0 = x;
                            let x1 = x + cluster.advance();
                            xlo_opt = Some(xlo_opt.map_or(x0, |v: f32| v.min(x0)));
                            xhi_opt = Some(xhi_opt.map_or(x1, |v: f32| v.max(x1)));
                        }
                    }
                }
                if let (Some(xlo), Some(xhi)) = (xlo_opt, xhi_opt) {
                    out.push(TextRect::new(
                        xlo,
                        m.block_min_coord,
                        xhi,
                        m.block_max_coord,
                    ));
                }
            } else {
                out.push(TextRect::new(
                    m.inline_min_coord,
                    m.block_min_coord,
                    m.inline_max_coord,
                    m.block_max_coord,
                ));
            }
        }
        out
    }

    /// 第 `idx` 行占用的字符区间（供外部行导航）。
    pub fn line_range(&self, idx: usize) -> Option<Range<usize>> {
        self.layout.get(idx).map(|line| line.text_range())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_style::{TextAlign, TextSpan, TextStyle};

    const TIGHT: RectStyle = RectStyle::Tight;
    const FULL: RectStyle = RectStyle::Full;

    /// 构造一段简短文本排版，供不依赖具体缩放值的几何测试。
    fn paragraph(text: &str) -> TextParagraph {
        use crate::build_paragraph;
        let mut fc = crate::font_cache::FontContext::new();
        let spans = vec![TextSpan::styled(text, TextStyle::normal().with_font_size(16.0))];
        build_paragraph(&mut fc, &spans, Some(200.0), 100, TextAlign::Start).unwrap()
    }

    #[test]
    fn paragraph_exposes_dimensions() {
        let p = paragraph("hello");
        assert!(p.width() > 0.0);
        assert!(p.height() > 0.0);
    }

    #[test]
    fn paragraph_exposes_line_metrics() {
        let p = paragraph("hello world");
        let m = p.line_metrics();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].start_index, 0);
        assert_eq!(m[0].end_index, 11);
    }

    #[test]
    fn paragraph_exposes_text() {
        let p = paragraph("hello");
        assert_eq!(p.text(), "hello");
        assert_eq!(p.text_len(), 5);
    }

    #[test]
    fn glyph_position_maps_point_into_text() {
        let p = paragraph("hello");
        // 点在第一行内：结果应落在 [0, len]
        let pos = p.glyph_position_at_coordinate(0.0, 2.0);
        assert!(pos <= 11, "pos={pos}");
        // 点在上方 → 0
        assert_eq!(p.glyph_position_at_coordinate(0.0, -50.0), 0);
        // 点在下方 → 文末
        let pos = p.glyph_position_at_coordinate(0.0, 200.0);
        assert!(pos <= 11);
    }

    #[test]
    fn rects_for_range_tight_single_line() {
        let p = paragraph("hello");
        let r = p.rects_for_range(0, 5, TIGHT);
        assert_eq!(r.len(), 1);
        let rect = r[0];
        assert!(rect.left < rect.right);
        assert!(rect.top < rect.bottom);
    }

    #[test]
    fn rects_for_range_full_uses_line() {
        let p = paragraph("hello");
        let r = p.rects_for_range(0, 5, FULL);
        assert_eq!(r.len(), 1);
        assert!(r[0].left <= r[0].right);
        assert!(r[0].top <= r[0].bottom);
    }

    #[test]
    fn rects_for_range_empty_range() {
        let p = paragraph("hello");
        assert!(p.rects_for_range(2, 2, TIGHT).is_empty());
        assert!(p.rects_for_range(5, 3, FULL).is_empty());
    }

    #[test]
    fn multi_line_rects_per_line() {
        let p = paragraph("This is a longer paragraph that will wrap given the modest max width used here.");
        let r = p.rects_for_range(0, 100, FULL);
        assert_eq!(r.len(), p.line_metrics().len(), "每行一个 Full 矩形");
        for rect in &r {
            assert!(rect.left <= rect.right);
            assert!(rect.top <= rect.bottom);
        }
    }

    #[test]
    fn word_boundary_finds_words() {
        let p = paragraph("hello world");
        let (s, e) = p.word_boundary(0).unwrap();
        assert_eq!(&p.text()[s..e], "hello");
        let (s, e) = p.word_boundary(6).unwrap();
        assert_eq!(&p.text()[s..e], "world");
    }

    #[test]
    fn word_boundary_out_of_range() {
        let p = paragraph("hello");
        assert!(p.word_boundary(100).is_none());
    }
}