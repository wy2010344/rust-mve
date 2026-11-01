//! 文本度量的杂项工具（对标 TS `canvasCore/TextMetric.ts`）。
//!
//! 目前只有一条：[`ink_shift_y_of`]。

use wy_text::{RectStyle, TextParagraph};

/// 单行文本的**光学垂直居中补偿**（对标 TS `inkShiftYOf` / Kotlin `RichTextNode.inkShiftY`）。
///
/// Skia 对小字号（≤16px）的段落行高有约 19px 的下限，且多出来的留白全部堆在
/// 字形上方：行盒垂直居中时墨迹会整体下沉 1~2px——这就是"按钮 / 标签文字
/// 看着没上下居中"的根因。这里按**紧致墨迹盒**把绘制原点平移到内容盒中心。
///
/// 只对"单行语义"生效（`optical` 为真）；多行保持自然行高（段落排版语义）。
/// 绘制、选区、命中必须共用同一个原点，否则点击位置会和看到的字错开。
///
/// - `para`：已排好的段落（空文本 / 还没排版时传 `None`，返回 0）；
/// - `text_bytes`：段落文本的**字节**长度（`rects_for_range` 要字节偏移）；
/// - `inner_height`：内容盒高度（`inner_size('y')`，不含 padding）；
/// - `optical`：是否按单行语义做补偿。
#[must_use]
pub fn ink_shift_y_of(
    para: Option<&TextParagraph>,
    text_bytes: usize,
    inner_height: f32,
    optical: bool,
) -> f32 {
    if !optical || text_bytes == 0 {
        return 0.0;
    }
    let Some(p) = para else {
        return 0.0;
    };
    let rects = p.rects_for_range(0, text_bytes, RectStyle::Tight);
    let Some(first) = rects.first() else {
        return 0.0;
    };
    let mut top = first.top;
    let mut bottom = first.bottom;
    for r in &rects {
        if r.top < top {
            top = r.top;
        }
        if r.bottom > bottom {
            bottom = r.bottom;
        }
    }
    (inner_height - (bottom - top)) / 2.0 - top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_para_or_empty_text_is_zero() {
        assert_eq!(ink_shift_y_of(None, 5, 40.0, true), 0.0);
        let p = build_sample_para("x");
        assert_eq!(ink_shift_y_of(Some(&p), 0, 40.0, true), 0.0);
    }

    #[test]
    fn multi_line_semantics_never_shifts() {
        let p = build_sample_para("x");
        assert_eq!(ink_shift_y_of(Some(&p), 1, 40.0, false), 0.0);
    }

    /// 单行语义下：内容盒比墨迹高时，原点下移，使墨迹在盒里居中。
    #[test]
    fn single_line_centers_ink_in_taller_box() {
        let p = build_sample_para("测试文本");
        let ink = p.rects_for_range(0, "测试文本".len(), RectStyle::Tight);
        let (top, bottom) = ink
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(t, b), r| {
                (t.min(r.top), b.max(r.bottom))
            });
        let inner_h = 60.0;
        let shift = ink_shift_y_of(Some(&p), "测试文本".len(), inner_h, true);
        let ink_center = top + shift + (bottom - top) / 2.0;
        assert!(
            (ink_center - inner_h / 2.0).abs() < 0.01,
            "墨迹中心 {ink_center} 应落在内容盒中心 {}",
            inner_h / 2.0
        );
    }

    fn build_sample_para(text: &str) -> TextParagraph {
        use wy_text::{TextAlign, TextSpan, TextStyle};
        crate::canvas_render2::rich_text::build_para(
            &[TextSpan {
                text: text.to_string(),
                style: TextStyle::default(),
            }],
            Some(200.0),
            1,
            "…",
            TextAlign::Start,
        )
    }
}
