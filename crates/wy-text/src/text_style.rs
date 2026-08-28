//! 文本样式：对应 Kotlin `RichTextStyle` / `RichTextSpan` / `TextAlign` / `TextDecoration`。

/// 文本装饰线（可组合）。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TextDecoration {
    /// 下划线。
    pub underline: bool,
    /// 删除线。
    pub line_through: bool,
}

impl TextDecoration {
    /// 无装饰。
    pub const NONE: Self = Self {
        underline: false,
        line_through: false,
    };

    /// 下划线。
    pub const UNDERLINE: Self = Self {
        underline: true,
        line_through: false,
    };

    /// 删除线。
    pub const LINE_THROUGH: Self = Self {
        underline: false,
        line_through: true,
    };

    /// 是否有任何装饰。
    pub const fn has_any(self) -> bool {
        self.underline || self.line_through
    }
}

/// 文本对齐方式。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// 靠起点（LTR 左对齐，RTL 右对齐）。
    #[default]
    Start,
    /// 居中。
    Center,
    /// 靠终点。
    End,
    /// 两端对齐。
    Justify,
}

/// 文本样式：字体/字号/粗细/斜体/颜色/间距/行高/装饰。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextStyle {
    /// 字体族名称；`None` 使用系统默认。
    pub font_family: Option<String>,
    /// 字号（像素）。
    pub font_size: f32,
    /// 字重（100-900，常用 400=Regular, 700=Bold）。
    pub font_weight: u16,
    /// 是否斜体。
    pub italic: bool,
    /// 文字颜色（RGBA u32，如 `0xFF0000FF`）。
    pub color: u32,
    /// 字间距（像素）。
    pub letter_spacing: f32,
    /// 词间距（像素）。
    pub word_spacing: f32,
    /// 行高倍数；`None` 使用 Parley 默认（约 1.2）。
    pub line_height_multiplier: Option<f32>,
    /// 装饰线。
    pub decoration: TextDecoration,
}

impl TextStyle {
    /// 默认样式（16px，Regular，黑色）。
    pub fn normal() -> Self {
        Self {
            font_size: 16.0,
            font_weight: 400,
            color: 0xFF000000,
            ..Default::default()
        }
    }

    /// 设置字号。
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// 设置字重。
    pub fn with_font_weight(mut self, weight: u16) -> Self {
        self.font_weight = weight;
        self
    }

    /// 设置颜色（RGBA u32）。
    pub fn with_color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    /// 设置斜体。
    pub fn with_italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// 设置行高倍数。
    pub fn with_line_height(mut self, multiplier: f32) -> Self {
        self.line_height_multiplier = Some(multiplier);
        self
    }
}

/// 文本片段：一段文本 + 样式。
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    /// 文本内容。
    pub text: String,
    /// 样式。
    pub style: TextStyle,
}

impl TextSpan {
    /// 构造纯文本片段（使用默认样式）。
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            text: s.into(),
            style: TextStyle::default(),
        }
    }

    /// 构造带样式的文本片段。
    pub fn styled(s: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: s.into(),
            style,
        }
    }
}

/// 软行度量。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct LineMetric {
    /// 该行在原始文本中的起始字符索引（含）。
    pub start_index: usize,
    /// 该行在原始文本中的结束字符索引（不含）。
    pub end_index: usize,
}

/// 文本矩形区域。
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct TextRect {
    /// 左边界。
    pub left: f32,
    /// 上边界。
    pub top: f32,
    /// 右边界。
    pub right: f32,
    /// 下边界。
    pub bottom: f32,
}

impl TextRect {
    /// 宽度。
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    /// 高度。
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_decoration_has_any() {
        assert!(!TextDecoration::NONE.has_any());
        assert!(TextDecoration::UNDERLINE.has_any());
        assert!(TextDecoration::LINE_THROUGH.has_any());
        assert!(TextDecoration {
            underline: true,
            line_through: true,
        }
        .has_any());
    }

    #[test]
    fn text_align_default_is_start() {
        assert_eq!(TextAlign::default(), TextAlign::Start);
    }

    #[test]
    fn text_style_normal_defaults() {
        let s = TextStyle::normal();
        assert_eq!(s.font_size, 16.0);
        assert_eq!(s.color, 0xFF000000);
        assert!(!s.italic);
        assert_eq!(s.font_weight, 400);
    }

    #[test]
    fn text_style_builder_chain() {
        let s = TextStyle::normal()
            .with_font_size(24.0)
            .with_font_weight(700)
            .with_color(0xFF0000FF)
            .with_italic()
            .with_line_height(1.5);
        assert_eq!(s.font_size, 24.0);
        assert_eq!(s.font_weight, 700);
        assert_eq!(s.color, 0xFF0000FF);
        assert!(s.italic);
        assert_eq!(s.line_height_multiplier, Some(1.5));
    }

    #[test]
    fn text_span_constructors() {
        let t = TextSpan::text("hello");
        assert_eq!(t.text, "hello");
        assert_eq!(t.style, TextStyle::default());

        let s = TextSpan::styled("world", TextStyle::normal().with_font_size(20.0));
        assert_eq!(s.text, "world");
        assert_eq!(s.style.font_size, 20.0);
    }

    #[test]
    fn line_metric_exposes_indices() {
        let m = LineMetric {
            start_index: 0,
            end_index: 5,
        };
        assert_eq!(m.start_index, 0);
        assert_eq!(m.end_index, 5);
    }

    #[test]
    fn text_rect_dimensions() {
        let r = TextRect {
            left: 10.0,
            top: 20.0,
            right: 50.0,
            bottom: 80.0,
        };
        assert_eq!(r.width(), 40.0);
        assert_eq!(r.height(), 60.0);
    }
}
