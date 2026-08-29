//! Vello GPU 执行器：将 `Scene`（高层图元）翻译为 Vello 绘制命令。
//!
//! 文本渲染通过 Parley 排版 + Vello `draw_glyphs` 实现。
//! 调用方需提供 Parley `FontContext` 和 `LayoutContext` 以支持文本图元。
//!
//! 使用方法：
//! ```ignore
//! let mut vello_scene = vello::Scene::new();
//! let mut font_cx = parley::FontContext::new();
//! let mut layout_cx = parley::LayoutContext::new();
//! execute_scene(&scene, &mut vello_scene, &mut font_cx, &mut layout_cx);
//! // 然后用 vello::Renderer 渲染 vello_scene 到 GPU surface
//! ```

use vello::kurbo::{Affine, RoundedRect};
use vello::peniko::{self, Fill};

use crate::{Color, Primitive, Scene};

/// 将 `Scene` 中的所有高层图元翻译到 Vello `Scene`。
///
/// `font_cx` 和 `layout_cx` 用于文本图元的 Parley 排版。
/// 如果 Scene 中没有文本图元，可以传入任意（但有效的）上下文。
pub fn execute_scene(
    src: &Scene,
    dst: &mut vello::Scene,
    font_cx: &mut parley::FontContext,
    layout_cx: &mut parley::LayoutContext,
) {
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
                render_text(dst, font_cx, layout_cx, point, text, *font_size, *color);
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

/// 使用 Parley 排版 + Vello draw_glyphs 渲染文本。
fn render_text(
    dst: &mut vello::Scene,
    font_cx: &mut parley::FontContext,
    layout_cx: &mut parley::LayoutContext,
    point: &crate::math::Point,
    text: &str,
    font_size: f32,
    color: Color,
) {
    if text.is_empty() {
        return;
    }

    // 使用系统默认 sans-serif 字体
    let brush = [color.red(), color.green(), color.blue(), color.alpha()];

    // 构建 Parley layout
    let display_scale = 1.0;
    let mut builder = layout_cx.ranged_builder(font_cx, text, display_scale, false);
    builder.push_default(parley::StyleProperty::FontSize(font_size));
    builder.push_default(parley::StyleProperty::Brush(brush));

    let mut layout: parley::Layout<[u8; 4]> = builder.build(text);

    // 不换行：整段文本作为一行
    layout.break_all_lines(None);
    layout.align(
        parley::Alignment::Start,
        parley::AlignmentOptions::default(),
    );

    let transform = Affine::translate((point.x as f64, point.y as f64));

    // 遍历排版结果，渲染每个 glyph run
    for line in layout.lines() {
        for item in line.items() {
            let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };

            let run = glyph_run.run();
            let font = run.font();
            let font_data = vello::peniko::FontData::new(font.data.clone(), font.index);
            let font_size = run.font_size();
            let synthesis = run.synthesis();
            let glyph_xform = synthesis
                .skew()
                .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

            let style = glyph_run.style();
            let text_color = to_peniko_color(Color::from_u32(u32::from_be_bytes(style.brush)));

            let x = glyph_run.offset();
            let y = glyph_run.baseline();

            dst.draw_glyphs(&font_data)
                .brush(text_color)
                .hint(true)
                .transform(transform)
                .glyph_transform(glyph_xform)
                .font_size(font_size)
                .normalized_coords(run.normalized_coords())
                .draw(Fill::NonZero, {
                    let mut cx = x;
                    glyph_run.glyphs().map(move |glyph| {
                        let gx = cx + glyph.x;
                        let gy = y + glyph.y;
                        cx += glyph.advance;
                        vello::Glyph {
                            id: glyph.id,
                            x: gx,
                            y: gy,
                        }
                    })
                });
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

    fn make_contexts() -> (parley::FontContext, parley::LayoutContext) {
        (parley::FontContext::new(), parley::LayoutContext::new())
    }

    #[test]
    fn execute_rect_produces_vello_scene() {
        let (mut font_cx, mut layout_cx) = make_contexts();
        let mut src = Scene::new();
        src.fill_rect(
            Rect::new(10.0, 20.0, 100.0, 50.0),
            Color::from_u32(0xFF_FF0000),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst, &mut font_cx, &mut layout_cx);
    }

    #[test]
    fn execute_round_rect_produces_vello_scene() {
        let (mut font_cx, mut layout_cx) = make_contexts();
        let mut src = Scene::new();
        src.fill_round_rect(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            8.0,
            Color::from_u32(0xFF_00FF00),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst, &mut font_cx, &mut layout_cx);
    }

    #[test]
    fn execute_text_produces_vello_scene() {
        let (mut font_cx, mut layout_cx) = make_contexts();
        let mut src = Scene::new();
        src.draw_text(
            crate::math::Point::new(0.0, 0.0),
            "Hello",
            16.0,
            Color::from_u32(0xFF_0000FF),
        );

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst, &mut font_cx, &mut layout_cx);
    }

    #[test]
    fn execute_empty_scene_is_noop() {
        let (mut font_cx, mut layout_cx) = make_contexts();
        let src = Scene::new();
        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst, &mut font_cx, &mut layout_cx);
    }

    #[test]
    fn execute_multiple_primitives() {
        let (mut font_cx, mut layout_cx) = make_contexts();
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
        execute_scene(&src, &mut dst, &mut font_cx, &mut layout_cx);
    }

    #[test]
    fn execute_empty_text_is_noop() {
        let (mut font_cx, mut layout_cx) = make_contexts();
        let mut src = Scene::new();
        src.draw_text(crate::math::Point::new(0.0, 0.0), "", 16.0, Color::BLACK);

        let mut dst = vello::Scene::new();
        execute_scene(&src, &mut dst, &mut font_cx, &mut layout_cx);
    }
}
