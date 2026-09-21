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
        let size = m.layout(text, font_size);
        m.cache.insert(key, size);
        size
    })
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
    });
}

struct Measure {
    font_cx: FontContext,
    layout_cx: LayoutContext,
    cache: HashMap<(String, u32), (f32, f32)>,
    line_height_cache: HashMap<(&'static str, u32), f32>,
}

impl Measure {
    fn layout(&mut self, text: &str, font_size: f32) -> (f32, f32) {
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

        let width = layout.width();
        let height = layout.height().max(font_size * 1.2);
        (width, height)
    }

    /// 用 Parley 排版一个测试行，取首行度量得到实际行高。
    fn measure_line_height(&mut self, font_size: f32) -> f32 {
        let brush = [0u8, 0, 0, 255];
        let display_scale = 1.0;
        let test_text = "Xj";
        let mut builder =
            self.layout_cx
                .ranged_builder(&mut self.font_cx, test_text, display_scale, false);
        builder.push_default(parley::StyleProperty::FontSize(font_size));
        builder.push_default(parley::StyleProperty::Brush(brush));

        let mut layout: parley::Layout<[u8; 4]> = builder.build(test_text);
        layout.break_all_lines(None);

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
    use super::{cursor_x, index_at_x, measure_text};

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
}
