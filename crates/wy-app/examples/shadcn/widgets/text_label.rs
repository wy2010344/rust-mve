//! shadcn 文本：单行、垂直居中，宽度由文案测量撑开。

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
        auto_width: true,
    }
}
