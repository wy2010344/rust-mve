//! 无渲染上下文文本测量（复刻 Kotlin `RichTextNode.argWidth/argHeight` 用排版结果做 intrinsic 尺寸）。
//!
//! 布局阶段（`node_size`/`layout_offsets`）需要知道文本自然尺寸，但此时没有
//! `Scene` 或 runner 里的 `FontContext`。本模块用 `thread_local` 持有独立
//! Parley `FontContext`/`LayoutContext` 与结果缓存，供 MVE 布局期调用。
//!
//! 结果按 `(text, font_size)` 缓存；文本变化（信号驱动）时自动生成新 key。

use std::cell::RefCell;
use std::collections::HashMap;

use parley::{FontContext, LayoutContext};

/// 测量单行文本的自然尺寸（像素）。
///
/// 空文本返回 `(0.0, font_size * 1.2)`；中文等复杂脚本按实际排版结果。
pub fn measure_text(text: &str, font_size: f32) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, font_size * 1.2);
    }
    let key = (text.to_string(), font_size.to_bits());
    MEASURE.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(size) = m.cache.get(&key) {
            return *size;
        }
        let size = {
            let layout = m.build(text, font_size);
            (layout.width(), layout.height().max(font_size * 1.2))
        };
        m.cache.insert(key, size);
        size
    })
}

/// 某字节偏移处的光标盒（相对排版原点，与 `Scene::draw_text` 同一坐标系）。
///
/// x 为该处字素簇边界的行内坐标；`top`/`height` 为所在行的字形行盒
/// （`baseline - ascent` / `ascent + descent`），对齐 Kotlin
/// `getRectsForRange(.., TIGHT)` 的字形紧盒语义（以字体排版盒近似）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretBox {
    /// 行内 x 坐标（字素簇边界）。
    pub x: f32,
    /// 所在行字形行盒顶（相对排版原点）。
    pub top: f32,
    /// 所在行字形行盒高。
    pub height: f32,
}

/// 字节偏移归一化到最近的 UTF-8 字符边界且不越界。
fn clamp_boundary(text: &str, idx: usize) -> usize {
    let mut i = idx.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// [byte_idx] 处的光标盒（光标 / 选区 / IME 定位统一几何来源）。
///
/// 空文本回退为单行盒 `(0, 0, font_size * 1.2)`。
pub fn caret_box(text: &str, font_size: f32, byte_idx: usize) -> CaretBox {
    let idx = clamp_boundary(text, byte_idx);
    if text.is_empty() {
        return CaretBox {
            x: 0.0,
            top: 0.0,
            height: font_size * 1.2,
        };
    }
    MEASURE.with(|m| {
        let mut m = m.borrow_mut();
        let layout = m.layout_of(text, font_size);
        let fallback = CaretBox {
            x: 0.0,
            top: 0.0,
            height: font_size * 1.2,
        };
        // line_for_byte_index 是 parley 私有 API，按行字节范围自行定位；
        // 末尾偏移（idx == len）归入最后一行。
        let line = layout
            .lines()
            .find(|line| line.text_range().contains(&idx))
            .or_else(|| layout.lines().last());
        let Some(line) = line else {
            return fallback;
        };
        let m = line.metrics();
        let top = m.baseline - m.ascent;
        let height = (m.ascent + m.descent).max(1.0);
        // 行起始 x：对齐偏移 + 行内最小坐标（parley Cluster::from_point 同款公式）。
        let mut x = m.offset + m.inline_min_coord;
        for run in line.runs() {
            for cluster in run.clusters() {
                let r = cluster.text_range();
                if idx <= r.start {
                    return CaretBox { x, top, height };
                }
                x += cluster.advance();
                if idx <= r.end {
                    return CaretBox { x, top, height };
                }
            }
        }
        CaretBox { x, top, height }
    })
}

/// 点击坐标 → 最近的字素簇边界字节索引（对齐 Kotlin
/// `getGlyphPositionAtCoordinate`，parley 官方命中测试）。
///
/// 落在布局盒上方 → 文首；下方 → 文末。
pub fn hit_point(text: &str, font_size: f32, x: f32, y: f32) -> usize {
    if text.is_empty() {
        return 0;
    }
    MEASURE.with(|m| {
        let mut m = m.borrow_mut();
        let layout = m.layout_of(text, font_size);
        if let Some((cluster, side)) = parley::Cluster::from_point(layout, x, y) {
            let r = cluster.text_range();
            let idx = match side {
                parley::ClusterSide::Left => r.start,
                parley::ClusterSide::Right => r.end,
            };
            return idx.min(text.len());
        }
        if y < 0.0 {
            0
        } else {
            text.len()
        }
    })
}

/// 第 [line_idx] 行的字形行盒 `(top, height)`；行不存在返回 `None`。
pub fn line_box(text: &str, font_size: f32, line_idx: usize) -> Option<(f32, f32)> {
    if text.is_empty() {
        return None;
    }
    MEASURE.with(|m| {
        let mut m = m.borrow_mut();
        let layout = m.layout_of(text, font_size);
        let found = layout.lines().nth(line_idx).map(|line| {
            let m = line.metrics();
            (m.baseline - m.ascent, (m.ascent + m.descent).max(1.0))
        });
        found
    })
}

/// 指定行内的 X → 字节索引（上下键垂直导航的行内映射）。
pub fn hit_line(text: &str, font_size: f32, line_idx: usize, x: f32) -> usize {
    match line_box(text, font_size, line_idx) {
        Some((top, height)) => hit_point(text, font_size, x, top + height * 0.5),
        None => 0,
    }
}

/// 测量字体的实际行高（像素）。
///
/// 用 Parley 排版一个测试字符串，取首行度量返回。
/// 空文本或排版失败时回退到 `font_size * 1.2`。
pub fn line_height(font_size: f32) -> f32 {
    let key = ("\n", font_size.to_bits());
    MEASURE.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(size) = m.line_height_cache.get(&key) {
            return *size;
        }
        let lh = m.measure_line_height(font_size);
        m.line_height_cache.insert(key, lh);
        lh
    })
}

/// 光标在第 [text_idx] 个 UTF-8 字节处的 X 坐标（已排版文本前缀宽度）。
pub fn cursor_x(text: &str, font_size: f32, text_idx: usize) -> f32 {
    let mut idx = text_idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    measure_text(&text[..idx], font_size).0
}

/// 将文本内 X 坐标映射到最近的 UTF-8 字节索引（点击定位光标）。
///
/// 从左往右累加每个字素簇的排版宽度，第一个累计超过 [x] 的簇起点即返回。
/// 单行 LTR 文本下结果与「最接近光标的字素簇边界」一致。
pub fn index_at_x(text: &str, font_size: f32, x: f32) -> usize {
    if text.is_empty() || x <= 0.0 {
        return 0;
    }
    if x >= measure_text(text, font_size).0 {
        return text.len();
    }
    for (start, _) in unicode_segmentation::UnicodeSegmentation::grapheme_indices(text, true) {
        if measure_text(&text[..start], font_size).0 >= x {
            return start;
        }
    }
    text.len()
}

thread_local! {
    static MEASURE: RefCell<Measure> = RefCell::new(Measure {
        font_cx: FontContext::new(),
        layout_cx: LayoutContext::new(),
        cache: HashMap::new(),
        line_height_cache: HashMap::new(),
        layouts: HashMap::new(),
    });
}

/// 排版缓存容量上限：超出即整体清空（编辑器场景同帧连续查询同一文本，
/// 单文本反复重建的概率极低）。
const LAYOUT_CACHE_CAP: usize = 32;

struct Measure {
    font_cx: FontContext,
    layout_cx: LayoutContext,
    cache: HashMap<(String, u32), (f32, f32)>,
    line_height_cache: HashMap<(&'static str, u32), f32>,
    layouts: HashMap<(String, u32), parley::Layout<[u8; 4]>>,
}

impl Measure {
    fn build(&mut self, text: &str, font_size: f32) -> parley::Layout<[u8; 4]> {
        let brush = [0u8, 0, 0, 255];
        let display_scale = 1.0;
        let mut builder =
            self.layout_cx
                .ranged_builder(&mut self.font_cx, text, display_scale, false);
        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::Brush(brush));

        let mut layout: parley::Layout<[u8; 4]> = builder.build(text);
        layout.break_all_lines(None);
        layout.align(
            parley::Alignment::Start,
            parley::AlignmentOptions::default(),
        );
        layout
    }

    /// 取（或构建并缓存）整段文本的排版结果——几何查询的统一来源。
    fn layout_of(&mut self, text: &str, font_size: f32) -> &parley::Layout<[u8; 4]> {
        let key = (text.to_string(), font_size.to_bits());
        if !self.layouts.contains_key(&key) {
            if self.layouts.len() >= LAYOUT_CACHE_CAP {
                self.layouts.clear();
            }
            let layout = self.build(text, font_size);
            self.layouts.insert(key, layout);
        }
        self.layouts
            .get(&(text.to_string(), font_size.to_bits()))
            .expect("layout just inserted")
    }

    /// 用 Parley 排版一个测试行，取首行度量得到实际行高。
    fn measure_line_height(&mut self, font_size: f32) -> f32 {
        let layout = self.build("Xj", font_size);
        let lh = layout
            .lines()
            .next()
            .map(|line| {
                let m = line.metrics();
                (m.block_max_coord - m.block_min_coord).max(font_size * 1.2)
            })
            .unwrap_or(font_size * 1.2);
        lh
    }
}

#[cfg(test)]
mod tests {
    use super::{caret_box, cursor_x, hit_line, hit_point, index_at_x, line_box, measure_text};

    #[test]
    fn measures_nonempty_text() {
        let (w, h) = measure_text("hello", 14.0);
        assert!(w > 0.0);
        assert!(h >= 14.0 * 1.2);
    }

    #[test]
    fn measures_empty_text() {
        assert_eq!(measure_text("", 14.0), (0.0, 14.0 * 1.2));
    }

    #[test]
    fn longer_text_is_wider() {
        let (w1, _) = measure_text("a", 14.0);
        let (w2, _) = measure_text("abcdefghij", 14.0);
        assert!(w2 > w1);
    }

    #[test]
    fn cursor_x_is_prefix_width() {
        let (w, _) = measure_text("abc", 14.0);
        assert_eq!(cursor_x("abc", 14.0, 3), w);
        assert_eq!(cursor_x("abc", 14.0, 0), 0.0);
        assert!(cursor_x("abc", 14.0, 2) < w);
    }

    #[test]
    fn index_at_x_maps_coords() {
        let (w, _) = measure_text("hello", 14.0);
        assert_eq!(index_at_x("hello", 14.0, 0.0), 0);
        assert_eq!(index_at_x("hello", 14.0, w), 5);
        assert_eq!(index_at_x("hello", 14.0, w / 2.0), 2); // 中间落点
                                                           // 单字符
        let (cw, _) = measure_text("h", 14.0);
        let idx = index_at_x("h", 14.0, cw * 0.5);
        assert!(idx == 0 || idx == 1);
    }

    #[test]
    fn index_at_x_empty_or_negative() {
        assert_eq!(index_at_x("", 14.0, 5.0), 0);
        assert_eq!(index_at_x("abc", 14.0, -10.0), 0);
    }

    #[test]
    fn caret_box_single_line_monotonic() {
        let fs = 16.0;
        let b0 = caret_box("hello", fs, 0);
        let b5 = caret_box("hello", fs, 5);
        assert_eq!(b0.x, 0.0);
        // 末尾偏移 = 整行宽度（与 measure_text 同一排版来源）。
        let (w, _) = measure_text("hello", fs);
        assert!((b5.x - w).abs() < 0.5, "{} vs {}", b5.x, w);
        assert!(b0.x < b5.x);
        assert!(b0.height > 0.0 && b0.height >= fs);
    }

    #[test]
    fn caret_box_empty_fallback() {
        let b = caret_box("", 14.0, 0);
        assert_eq!(b.x, 0.0);
        assert_eq!(b.height, 14.0 * 1.2);
    }

    #[test]
    fn caret_box_multiline_lines_stacked() {
        let fs = 16.0;
        let text = "ab\ncd";
        // "ab" 末尾（\n 前）→ 第一行行尾；第二行行首（'c' 的偏移 3）→ x=0。
        let b_nl = caret_box(text, fs, 2); // '\n' 处
        let b_l2 = caret_box(text, fs, 3); // 第二行 'c'
        assert!(
            b_l2.top > b_nl.top,
            "第二行应更低：{} vs {}",
            b_l2.top,
            b_nl.top
        );
        assert!(b_nl.x > 0.0, "第一行行尾 x 应为行宽：{}", b_nl.x);
        assert_eq!(b_l2.x, 0.0, "第二行行首 x 应为 0");
    }

    #[test]
    fn hit_point_roundtrip() {
        let fs = 16.0;
        let text = "hello";
        let (w, _) = measure_text(text, fs);
        assert_eq!(hit_point(text, fs, -5.0, 8.0), 0);
        assert_eq!(hit_point(text, fs, w + 100.0, 8.0), text.len());
        let mid = hit_point(text, fs, w * 0.5, 8.0);
        assert!(mid > 0 && mid < text.len());
        // 中点落在簇边界附近
        let cb = caret_box(text, fs, mid);
        assert!((cb.x - w * 0.5).abs() < w * 0.3);
    }

    #[test]
    fn hit_point_multiline_row() {
        let fs = 16.0;
        let text = "ab\ncd";
        let (_, h) = measure_text(text, fs);
        // 点击下半区 → 第二行首
        let idx = hit_point(text, fs, 0.0, h - 1.0);
        assert_eq!(idx, 3, "应命中第二行行首，得到 {}", idx);
    }

    #[test]
    fn line_box_and_hit_line() {
        let fs = 16.0;
        let text = "ab\ncd";
        assert!(line_box(text, fs, 0).is_some());
        let (t0, _) = line_box(text, fs, 0).unwrap();
        let (t1, h1) = line_box(text, fs, 1).unwrap();
        assert!(t1 > t0 && h1 > 0.0);
        assert!(line_box(text, fs, 99).is_none());
        // 行内命中：第二行给行首 x → 该行起始
        let idx = hit_line(text, fs, 1, 0.0);
        assert_eq!(idx, 3);
        let text2 = "hello";
        let idx2 = hit_line(text2, fs, 0, 1000.0);
        assert_eq!(idx2, text2.len());
    }
}
