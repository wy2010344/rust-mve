//! 文本节点：单行、尺寸由文案测量撑开，交给父级 flex 定位。
//!
//! 与 shadcn `widgets/text_label.rs` 同一份契约（示例之间不共享 crate，
//! 所以各留一份）。存在的理由只有一个：**文本是节点，不是画上去的像素**。
//! 调用方不再 `measure_text` 算坐标，节点自己答出"我有多宽多高"。

use wy_mve::{rec, wrapped_text_node, Fc};
use wy_render::Color;

/// 文本节点。`content` / `color` 是 getter：绘制期读信号，改动自动重绘。
#[rec]
pub fn text_label(
    #[getter]
    #[props(required)]
    content: String,
    #[props(default = 13.0f32)] font_size: f32,
    #[getter]
    #[props(required)]
    color: Color,
) -> Fc {
    wrapped_text_node! {
        text: content(),
        font_size: font_size,
        color: color(),
        // 宽度由内容决定（不向父级要份额），这就是"文本撑起来的尺寸"。
        auto_width: true,
    }
}
