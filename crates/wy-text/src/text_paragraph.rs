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
    ///
    /// 用 `Rc` 持有：整段绘制时（`Scene::draw_paragraph` / CanvasKit
    /// `drawParagraph`）把同一份 layout 直接交给后端，不重新排版、不切段。
    layout: std::rc::Rc<parley::Layout<[u8; 4]>>,
    /// 原始文本（`Layout` 不保存文本内容，需随段落携带）。
    text: String,
}

impl TextParagraph {
    /// 构造排版结果。
    pub(crate) fn new(layout: parley::Layout<[u8; 4]>, text: String) -> Self {
        Self {
            layout: std::rc::Rc::new(layout),
            text,
        }
    }

    /// 共享排版结果（供渲染后端整段栅格化，见 `Scene::draw_paragraph`）。
    #[must_use]
    pub fn shared_layout(&self) -> std::rc::Rc<parley::Layout<[u8; 4]>> {
        std::rc::Rc::clone(&self.layout)
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
                let m = line.metrics();
                LineMetric {
                    start_index: r.start,
                    end_index: r.end,
                    top: m.block_min_coord,
                    bottom: m.block_max_coord,
                }
            })
            .collect()
    }

    /// 原始文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 字节偏移落在第几条软行。
    ///
    /// 与"用光标矩形去碰行盒"不同，这里**只看偏移**：空行（没有字形簇、量不出
    /// 矩形）同样能定位。规则与 Parley 的行区间一致：`start <= byte < end`；
    /// `byte` 到达文末（或落在任何行的右边界上）归最后一个匹配行。
    ///
    /// 上下键换行的行号必须走这里——用几何反推会在空行上退回上一行，
    /// 表现为"按上键跳不过空行"。
    pub fn line_index_of_byte(&self, byte: usize) -> Option<usize> {
        let mut last = None;
        for (i, line) in self.layout.lines().enumerate() {
            let r = line.text_range();
            if byte >= r.start && byte < r.end {
                return Some(i);
            }
            last = Some(i);
        }
        // 文末 / 越界：归最后一行（Parley 的行区间是左闭右开，末尾没有归属）
        last
    }

    /// 第 `line_index` 行内，横向 `dx` 对应的字节偏移。
    ///
    /// 与 [`Self::glyph_position_at_coordinate`] 的差别：坐标查找是"全段落找最近
    /// 簇"，空行没有簇可命中，会落到"文末"兜底——上下键于是**跳过空行**。
    /// 这里把**行范围当硬边界**：命中簇也钳到本行，空行 / 行内无字形则返回行首。
    ///
    /// 钳制上界是**可见行末**（行尾换行符之前），不是 `range.end`：Parley 的行区间
    /// 左闭右开且把 `\n` 归上一行，`dx` 超出短行宽度时 `Cluster::from_point` 会命中
    /// `\n` 簇的**右侧**（= 下一行行首）——上下键于是"回到行首且永远卡在本行"
    /// （`line_index_of_byte` 又把该位置归回本行，再按上键原地打转）。Skia 的
    /// `getGlyphPositionAtCoordinate`（Kotlin 同名原语）对越界 x 返回的是换行符
    /// **之前**的行末位置，这里对齐它。
    pub fn position_in_line(&self, line_index: usize, dx: f32) -> usize {
        let Some(line) = self.layout.lines().nth(line_index) else {
            return self.text.len();
        };
        let range = line.text_range();
        // 空行没有任何簇：`Cluster::from_point` 会去别的行找最近的，命中的
        // 是**相邻行**的字形（clamp 之后仍会跨过去）。所以先短路。
        if self.is_blank_line(line_index) {
            return range.start;
        }
        let m = line.metrics();
        let y = (m.block_min_coord + m.block_max_coord) / 2.0;
        // 可见行末：剥掉行尾的 `\r`/`\n`（软行没有，硬行末尾必有）。
        let bytes = self.text.as_bytes();
        let mut visible_end = range.end.min(bytes.len());
        while visible_end > range.start {
            let b = bytes[visible_end - 1];
            if b == b'\n' || b == b'\r' {
                visible_end -= 1;
            } else {
                break;
            }
        }
        match Cluster::from_point(&self.layout, dx, y) {
            Some((cluster, side)) => {
                let r = cluster.text_range();
                let pos = match side {
                    ClusterSide::Left => r.start,
                    ClusterSide::Right => r.end,
                };
                pos.clamp(range.start, visible_end)
            }
            // 点在行外：落在行首（空行已在上面短路，这里是非空行的左侧空白）。
            None => range.start,
        }
    }

    /// 第 `line_index` 行是否**没有字形**（空行，或只由 `\r`/`\n` 组成）。
    ///
    /// 光标落在这种行上时没有任何簇可量，绘制与几何都得改走行盒。
    pub fn is_blank_line(&self, line_index: usize) -> bool {
        let Some(line) = self.layout.lines().nth(line_index) else {
            return false;
        };
        let r = line.text_range();
        r.start >= r.end
            || self
                .text
                .as_bytes()
                .get(r.start..r.end)
                .is_none_or(|b| b.iter().all(|c| *c == b'\r' || *c == b'\n'))
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
        let spans = vec![TextSpan::styled(
            text,
            TextStyle::normal().with_font_size(16.0),
        )];
        build_paragraph(
            &mut fc,
            &spans,
            Some(200.0),
            100,
            "\u{2026}",
            TextAlign::Start,
        )
        .unwrap()
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
        let p = paragraph(
            "This is a longer paragraph that will wrap given the modest max width used here.",
        );
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

    /// 空行是**真行**：有独立行盒、有自己的字节区间。
    ///
    /// 这是"上下键能停在空行上"的前提；若排版层把空行合并掉，
    /// 上层无论怎么算都跳不过去。
    #[test]
    fn blank_lines_are_real_lines() {
        let p = paragraph("a\n\nb");
        let m = p.line_metrics();
        assert_eq!(m.len(), 3, "a\\n\\nb 必须排成 3 行");
        assert!(m[1].bottom > m[1].top, "空行也要有行高，光标才画得出来");
        assert_eq!(
            (m[1].start_index, m[1].end_index),
            (2, 3),
            "空行区间就是那一格换行符"
        );
    }

    /// 按偏移查行：空行有归属，文末归最后一行。
    #[test]
    fn line_index_of_byte_lands_on_blank_lines() {
        let p = paragraph("a\n\nb");
        assert_eq!(p.line_index_of_byte(0), Some(0));
        assert_eq!(p.line_index_of_byte(1), Some(0), "换行符归上一行");
        assert_eq!(p.line_index_of_byte(2), Some(1), "空行不能被跳过");
        assert_eq!(p.line_index_of_byte(3), Some(2));
        assert_eq!(p.line_index_of_byte(4), Some(2), "文末归最后一行");
    }

    /// 行内取点：空行只返回行首，绝不越到别的行（尤其不能兜底到文末）。
    #[test]
    fn position_in_line_never_leaves_the_line() {
        let p = paragraph("a\n\nb");
        assert_eq!(p.position_in_line(1, 0.0), 2, "空行走行首");
        assert_eq!(p.position_in_line(1, 500.0), 2, "空行不能兜底到文末");
        assert_eq!(p.position_in_line(2, 0.0), 3, "非空行仍按横向位置取");
        assert_eq!(p.position_in_line(99, 0.0), 4, "越界行归文末");
    }

    /// 回归（上下键卡在本行）：`dx` 超出**短行**宽度时，落点必须钳到**可见行末**
    /// （换行符之前），不能钳到含 `\n` 的 `range.end`。
    ///
    /// `"ab\n中文测试"`：行 0 = `ab`（约 2 字宽）。末行光标在第 4 个中文字之后，
    /// `dx` 远超行 0 宽度——旧行为命中 `\n` 簇右侧并钳到 `range.end`（= 3，`\n`
    /// 之后 = 下一行行首），上下键视觉上"回到行首且永远卡在本行"。Skia 的
    /// `getGlyphPositionAtCoordinate`（Kotlin 同名原语）对越界 x 返回换行符之前。
    #[test]
    fn position_in_line_clamps_before_trailing_newline() {
        let p = paragraph("ab\n中文测试");
        // dx 超出行 0 宽度：落行 0 可见末（= 2，`\n` 之前），绝不是 3。
        assert_eq!(p.position_in_line(0, 500.0), 2, "越界 dx 钳到可见行末");
        assert_eq!(p.position_in_line(0, 0.0), 0, "dx=0 落行首");
        // 末行没有换行符：越界仍归文末（字节偏移："ab\n" 3 字节 + 4 中文字 × 3 = 15）。
        assert_eq!(p.position_in_line(1, 500.0), 15, "无换行行不受影响");
    }

    /// 末尾空行（`"a\n"` 排成两行）同样要能被识别，否则文末光标会画在上一行。
    #[test]
    fn blank_line_detection_covers_trailing_newline() {
        let p = paragraph("a\n");
        assert_eq!(p.line_metrics().len(), 2);
        assert!(p.is_blank_line(1), "末尾空行");
        assert!(!p.is_blank_line(0));
    }
}
