//! Vello GPU 执行器：将 `Scene`（高层图元）翻译为 Vello 绘制命令。
//!
//! 文本渲染通过 Parley 排版 + Vello `draw_glyphs` 实现。
//! 调用方需提供 Parley `FontContext`、`LayoutContext` 和 [`TextLayoutCache`]。
//!
//! 使用方法：
//! ```ignore
//! let mut vello_scene = vello::Scene::new();
//! let mut font_cx = parley::FontContext::new();
//! let mut layout_cx = parley::LayoutContext::new();
//! let mut text_cache = TextLayoutCache::new();
//! execute_scene(&scene, &mut vello_scene, &mut font_cx, &mut layout_cx, &mut text_cache);
//! // 然后用 vello::Renderer 渲染 vello_scene 到 GPU surface
//! ```

use std::collections::HashMap;

use vello::kurbo::{Affine, RoundedRect, Stroke};
use vello::peniko::{self, Fill};

use crate::{Color, Primitive, Scene};

/// 文本布局缓存：避免每帧重建 Parley Layout。
///
/// 按 `(text, font_size, color)` 缓存排版结果。
/// Scene 内容不变时命中缓存，跳过 Parley shaping + layout。
pub struct TextLayoutCache {
    cache: HashMap<u64, parley::Layout<[u8; 4]>>,
}

impl TextLayoutCache {
    /// 创建空缓存。
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// 清空缓存（窗口大小改变或字体变化时调用）。
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// 获取或构建文本布局。
    fn get_or_build(
        &mut self,
        font_cx: &mut parley::FontContext,
        layout_cx: &mut parley::LayoutContext,
        text: &str,
        font_size: f32,
        color: Color,
    ) -> &parley::Layout<[u8; 4]> {
        let key = Self::hash_key(text, font_size, color);

        self.cache.entry(key).or_insert_with(|| {
            let brush = [color.red(), color.green(), color.blue(), color.alpha()];
            let display_scale = 1.0;
            let mut builder = layout_cx.ranged_builder(font_cx, text, display_scale, false);
            builder.push_default(parley::StyleProperty::FontSize(font_size));
            builder.push_default(parley::StyleProperty::Brush(brush));

            let mut layout: parley::Layout<[u8; 4]> = builder.build(text);
            layout.break_all_lines(None);
            layout.align(
                parley::Alignment::Start,
                parley::AlignmentOptions::default(),
            );
            layout
        })
    }

    fn hash_key(text: &str, font_size: f32, color: Color) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        font_size.to_bits().hash(&mut hasher);
        color.to_u32().hash(&mut hasher);
        hasher.finish()
    }
}

impl Default for TextLayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 `Scene` 中的所有高层图元翻译到 Vello `Scene`。
///
/// `font_cx` 和 `layout_cx` 用于文本图元的 Parley 排版。
/// `text_cache` 缓存文本布局结果，避免每帧重建。
pub fn execute_scene(
    src: &Scene,
    dst: &mut vello::Scene,
    font_cx: &mut parley::FontContext,
    layout_cx: &mut parley::LayoutContext,
    text_cache: &mut TextLayoutCache,
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
            Primitive::StrokeRoundRect {
                rect,
                radius,
                color,
                stroke_width,
            } => {
                let k_rect = kurbo_rect(rect.x, rect.y, rect.width, rect.height);
                let rr = RoundedRect::from_rect(k_rect, *radius as f64);
                let c = to_peniko_color(*color);
                let stroke = Stroke::new(*stroke_width as f64);
                dst.stroke(&stroke, Affine::IDENTITY, c, None, &rr);
            }
            Primitive::Text {
                point,
                text,
                font_size,
                color,
            } => {
                render_text(&mut TextRenderParams {
                    dst,
                    font_cx,
                    layout_cx,
                    text_cache,
                    point: *point,
                    text,
                    font_size: *font_size,
                    color: *color,
                });
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
/// 文本渲染参数。
struct TextRenderParams<'a> {
    dst: &'a mut vello::Scene,
    font_cx: &'a mut parley::FontContext,
    layout_cx: &'a mut parley::LayoutContext,
    text_cache: &'a mut TextLayoutCache,
    point: crate::math::Point,
    text: &'a str,
    font_size: f32,
    color: Color,
}

/// 使用缓存的 Parley layout + Vello draw_glyphs 渲染文本。
fn render_text(params: &mut TextRenderParams<'_>) {
    if params.text.is_empty() {
        return;
    }

    // 从缓存获取或构建 layout
    let layout = params.text_cache.get_or_build(
        params.font_cx,
        params.layout_cx,
        params.text,
        params.font_size,
        params.color,
    );

    let transform = Affine::translate((params.point.x as f64, params.point.y as f64));

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

            params
                .dst
                .draw_glyphs(&font_data)
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

    fn make_contexts() -> (parley::FontContext, parley::LayoutContext, TextLayoutCache) {
        (
            parley::FontContext::new(),
            parley::LayoutContext::new(),
            TextLayoutCache::new(),
        )
    }

    #[test]
    fn execute_rect_produces_vello_scene() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
        let mut src = Scene::new();
        src.fill_rect(
            Rect::new(10.0, 20.0, 100.0, 50.0),
            Color::from_u32(0xFF_FF0000),
        );

        let mut dst = vello::Scene::new();
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
    }

    #[test]
    fn execute_round_rect_produces_vello_scene() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
        let mut src = Scene::new();
        src.fill_round_rect(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            8.0,
            Color::from_u32(0xFF_00FF00),
        );

        let mut dst = vello::Scene::new();
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
    }

    #[test]
    fn execute_text_produces_vello_scene() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
        let mut src = Scene::new();
        src.draw_text(
            crate::math::Point::new(0.0, 0.0),
            "Hello",
            16.0,
            Color::from_u32(0xFF_0000FF),
        );

        let mut dst = vello::Scene::new();
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
    }

    #[test]
    fn execute_empty_scene_is_noop() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
        let src = Scene::new();
        let mut dst = vello::Scene::new();
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
    }

    #[test]
    fn execute_multiple_primitives() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
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
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
    }

    #[test]
    fn execute_empty_text_is_noop() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
        let mut src = Scene::new();
        src.draw_text(crate::math::Point::new(0.0, 0.0), "", 16.0, Color::BLACK);

        let mut dst = vello::Scene::new();
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
    }

    #[test]
    fn text_cache_reuses_layout() {
        let (mut font_cx, mut layout_cx, mut text_cache) = make_contexts();
        let mut src = Scene::new();
        src.draw_text(
            crate::math::Point::new(0.0, 0.0),
            "Cached",
            16.0,
            Color::BLACK,
        );

        let mut dst = vello::Scene::new();
        // 第一次：构建 layout 并缓存
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
        assert_eq!(text_cache.cache.len(), 1);
        // 第二次：命中缓存，不再构建
        execute_scene(
            &src,
            &mut dst,
            &mut font_cx,
            &mut layout_cx,
            &mut text_cache,
        );
        assert_eq!(text_cache.cache.len(), 1);
    }
}
