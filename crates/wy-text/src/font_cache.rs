//! 字体上下文：封装 Parley 的 `FontContext` + `LayoutContext`。
//!
//! `FontContext` 管理系统字体数据库（加载代价高），`LayoutContext` 是排版计算的
//! scratch space（可复用分配）。两者都应全局共享（至少在粗粒度边界共享）。

use parley::{FontContext as ParleyFontContext, LayoutContext};

/// 全局字体上下文：包含字体数据库 + 排版 scratch space。
///
/// 创建代价高（需要枚举系统字体），应在应用生命周期中共享复用。
/// 两个字段公开以便 `build_paragraph` 分别借用。
pub struct FontContext {
    /// Parley 字体数据库。
    pub font_cx: ParleyFontContext,
    /// Parley 排版 scratch space。
    pub layout_cx: LayoutContext,
}

impl FontContext {
    /// 创建新的字体上下文（枚举系统字体，代价较高）。
    pub fn new() -> Self {
        Self {
            font_cx: ParleyFontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// 测量文本尺寸，返回 `(width, height)` 像素值。
    ///
    /// 封装了 Parley 的 font_cx + layout_cx 分别借用，
    /// 避免 RefCell split borrow 问题。
    pub fn measure_text(&mut self, text: &str, font_size: f32) -> (f32, f32) {
        if text.is_empty() {
            return (0.0, font_size * 1.2);
        }
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
}

impl Default for FontContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_context_creates_successfully() {
        let fc = FontContext::new();
        let _ = fc;
    }

    #[test]
    fn font_context_default_is_same_as_new() {
        let a = FontContext::new();
        let b = FontContext::default();
        let _ = a;
        let _ = b;
    }
}
