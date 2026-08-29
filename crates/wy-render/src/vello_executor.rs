//! Vello GPU 执行器：将 `Scene`（高层图元）翻译为 Vello 绘制命令。
//!
//! 使用方法：
//! ```ignore
//! let mut vello_scene = vello::Scene::new();
//! execute_scene(&scene, &mut vello_scene);
//! // 然后用 vello::Renderer 渲染 vello_scene 到 GPU surface
//! ```

use vello::kurbo::{Affine, RoundedRect};
use vello::peniko::{self, Fill};

use crate::{Color, Primitive, Scene};

/// 将 `Scene` 中的所有高层图元翻译到 Vello `Scene`。
pub fn execute_scene(src: &Scene, dst: &mut vello::Scene) {
    for prim in src.iter() {
        match prim {
            Primitive::Rect { rect, color } => {
                let k_rect = kurbo_rect(rect.x, rect.y, rect.width, rect.height);
                let c = to_peniko_color(*color);
                dst.fill(Fill::NonZero, Affine::IDENTITY, c, None, &k_rect);
            }
            Primitive::RoundRect {
                rect,
                radius,
                color,
            } => {
                let k_rect = kurbo_rect(rect.x, rect.y, rect.width, rect.height);
                let rr = RoundedRect::from_rect(k_rect, *radius as f64);
                let c = to_peniko_color(*color);
                dst.fill(Fill::NonZero, Affine::IDENTITY, c, None, &rr);
            }
            Primitive::Text {
                point,
                text,
                font_size,
                color,
            } => {
                // 文本渲染：用占位矩形代替（需要 Parley 集成才能正确渲染）
                // TODO: 集成 Parley glyph shaping + vello draw_glyphs
                let c = to_peniko_color(*color);
                let placeholder_width = text.len() as f32 * *font_size * 0.6;
                let placeholder_height = *font_size;
                let k_rect = kurbo_rect(point.x, point.y, placeholder_width, placeholder_height);
                dst.fill(Fill::NonZero, Affine::IDENTITY, c, None, &k_rect);
            }
            Primitive::ClipPush { rect } => {
                let k_rect = kurbo_rect(rect.x, rect.y, rect.width, rect.height);
                dst.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &k_rect);
            }
            Primitive::ClipPop => {
                dst.pop_layer();
            }
        }
    }
}

/// 将我们的 `Color` 转换为 peniko `Color`。
fn to_peniko_color(color: Color) -> peniko::Color {
    peniko::Color::from_rgba8(color.red(), color.green(), color.blue(), color.alpha())
}

/// 构造 kurbo `Rect`（左上角 + 宽高 → 左上角 + 右下角）。
fn kurbo_rect(x: f32, y: f32, w: f32, h: f32) -> vello::kurbo::Rect {
    vello::kurbo::Rect::new(x as f64, y as f64, (x + w) as f64, (y + h) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn execute_rect_produces_vello_scene() {
        let mut src = Scene::new();
        src.fill_rect(
            Rect::new(10.0, 20.0, 100.0, 50.0),
            Color::from_u32(0xFF_FF0000),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst);
    }

    #[test]
    fn execute_round_rect_produces_vello_scene() {
        let mut src = Scene::new();
        src.fill_round_rect(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            8.0,
            Color::from_u32(0xFF_00FF00),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst);
    }

    #[test]
    fn execute_text_produces_vello_scene() {
        let mut src = Scene::new();
        src.draw_text(
            crate::math::Point::new(0.0, 0.0),
            "Hello",
            16.0,
            Color::from_u32(0xFF_0000FF),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst);
    }

    #[test]
    fn execute_empty_scene_is_noop() {
        let src = Scene::new();
        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst);
    }

    #[test]
    fn execute_multiple_primitives() {
        let mut src = Scene::new();
        src.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Color::WHITE);
        src.fill_round_rect(Rect::new(10.0, 10.0, 80.0, 80.0), 4.0, Color::BLACK);
        src.draw_text(
            crate::math::Point::new(20.0, 40.0),
            "Test",
            20.0,
            Color::from_u32(0xFF_FF0000),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst);
    }
}
