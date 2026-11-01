//! Scene：平台无关的绘制命令记录器。
//!
//! Widget 的 `draw()` 方法向 Scene 添加高层图元，最终由 Vello/wgpu 提交 GPU。
//! Scene 不依赖任何 GPU 资源，可在任意线程记录，作为渲染管线的中间表示。

use crate::color::Color;
use crate::math::Rect;

/// 已排版文本的共享句柄。
///
/// 只是 `Rc<parley::Layout<[u8; 4]>>` 的一层包装，存在的理由是
/// [`Primitive`] 需要 `Clone + Debug + PartialEq` 而 `parley::Layout` 都没有：
/// 相等按**同一份排版结果**（`Rc::ptr_eq`）判定，打印只给一句摘要。
#[derive(Clone)]
pub struct ParagraphHandle(std::rc::Rc<parley::Layout<[u8; 4]>>);

impl ParagraphHandle {
    /// 包装一份排版结果。
    #[must_use]
    pub fn new(layout: std::rc::Rc<parley::Layout<[u8; 4]>>) -> Self {
        Self(layout)
    }

    /// 取回底层排版结果。
    #[must_use]
    pub fn layout(&self) -> &parley::Layout<[u8; 4]> {
        &self.0
    }
}

impl From<std::rc::Rc<parley::Layout<[u8; 4]>>> for ParagraphHandle {
    fn from(v: std::rc::Rc<parley::Layout<[u8; 4]>>) -> Self {
        Self(v)
    }
}

impl PartialEq for ParagraphHandle {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ParagraphHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParagraphHandle(parley::Layout)")
    }
}

/// 平台图片（对标 TS `canvasRender/PlatformImage`）。
///
/// `source` 是 Vello 可直接消费的 RGBA/BGRA 像素资源；`width` / `height` 保留
/// TS 接口上的自然尺寸字段，图片节点据此计算宽高比。
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformImage {
    /// 原图宽度。
    pub width: f32,
    /// 原图高度。
    pub height: f32,
    /// 后端图片资源。
    pub source: vello::peniko::ImageData,
}

impl PlatformImage {
    /// 从后端图片资源构造。
    #[must_use]
    pub fn new(source: vello::peniko::ImageData) -> Self {
        Self {
            width: source.width as f32,
            height: source.height as f32,
            source,
        }
    }

    /// 从直通 RGBA8 像素构造；字节长度不等于 `width * height * 4` 时返回 `None`。
    #[must_use]
    pub fn from_rgba8(width: u32, height: u32, bytes: Vec<u8>) -> Option<Self> {
        let expected = 4usize
            .checked_mul(width as usize)?
            .checked_mul(height as usize)?;
        if bytes.len() != expected {
            return None;
        }
        Some(Self::new(vello::peniko::ImageData {
            data: bytes.into(),
            format: vello::peniko::ImageFormat::Rgba8,
            alpha_type: vello::peniko::ImageAlphaType::Alpha,
            width,
            height,
        }))
    }
}

/// 绘制图元。
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// 填充矩形。
    Rect { rect: Rect, color: Color },
    /// 圆角填充矩形。
    RoundRect {
        rect: Rect,
        radius: f32,
        color: Color,
    },
    /// 圆角描边矩形（边框）。
    StrokeRoundRect {
        rect: Rect,
        radius: f32,
        color: Color,
        stroke_width: f32,
    },
    /// 文本。
    Text {
        /// 文本锚点（左上角基线位置）。
        point: crate::math::Point,
        text: String,
        font_size: f32,
        color: Color,
    },
    /// 已排版好的整段文本（对标 CanvasKit `drawParagraph`）。
    ///
    /// `Text { text, font_size, color }` 只能表达"单一字号 + 单一颜色"，会迫使
    /// 声明层自己按行/按 span 切开再逐片 `drawText` —— 那样对齐偏移、混排基线、
    /// 跨片字距全部由声明层自己猜。这里直接把 Parley 排版结果整段交给后端，
    /// 后端按行/run/glyph 原样栅格化：对齐、基线、字距与测量用的是同一份数据。
    ///
    /// `layout` 里的每个 span 已带自己的 `Brush`（= 颜色），所以混色段落不需要
    /// 调用方再切分。
    Paragraph {
        /// 段落原点（左上角）。
        point: crate::math::Point,
        /// 排版结果（`wy-text::TextParagraph` 内部持有同一份）。
        layout: ParagraphHandle,
    },
    /// 图片，缩放到目标矩形。
    Image {
        /// 图片资源。
        image: PlatformImage,
        /// 目标矩形（局部坐标）。
        rect: Rect,
    },
    /// 路径：折线（`closed = false`）或闭合多边形（`closed = true`）。
    ///
    /// `stroke_width > 0` 时描边（折线），`0` 时填充（闭合多边形）。
    /// 有了它，图标可以自绘矢量路径（不依赖字体是否含该字形），
    /// 图表也能画面积/折线而不必退化成柱状矩形。
    Path {
        /// 路径顶点（局部坐标）。
        points: Vec<crate::math::Point>,
        /// 是否闭合（闭合后可填充）。
        closed: bool,
        /// 描边宽度（0 = 填充）。
        stroke_width: f32,
        color: Color,
    },
    /// 裁剪入栈。
    ClipPush { rect: Rect },
    /// 圆角裁剪入栈。
    RoundClipPush { rect: Rect, radius: f32 },
    /// 裁剪出栈。
    ClipPop,
    /// 坐标变换入栈：后续图元的坐标将加上偏移量。
    TransformPush { offset: crate::math::Point },
    /// 坐标变换出栈。
    TransformPop,
}

/// Scene：平台无关的绘制命令记录器。
#[derive(Default)]
pub struct Scene {
    primitives: Vec<Primitive>,
}

impl Scene {
    /// 创建空的 Scene。
    pub fn new() -> Self {
        Self {
            primitives: Vec::new(),
        }
    }

    /// 记录的图元数量。
    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    /// Scene 是否为空（没有记录任何图元）。
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    /// 逐图元只读访问，供测试与后续后端处理遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Primitive> {
        self.primitives.iter()
    }

    /// 清空已记录的全部图元（下一帧复用同一 Scene 时调用）。
    pub fn clear(&mut self) {
        self.primitives.clear();
    }

    /// 记录一个填充矩形。
    pub fn fill_rect(&mut self, rect: impl Into<Rect>, color: impl Into<Color>) {
        self.primitives.push(Primitive::Rect {
            rect: rect.into(),
            color: color.into(),
        });
    }

    /// 记录一个圆角填充矩形。
    pub fn fill_round_rect(&mut self, rect: impl Into<Rect>, radius: f32, color: impl Into<Color>) {
        self.primitives.push(Primitive::RoundRect {
            rect: rect.into(),
            radius,
            color: color.into(),
        });
    }

    /// 记录一个圆角描边矩形（边框）。
    pub fn stroke_round_rect(
        &mut self,
        rect: impl Into<Rect>,
        radius: f32,
        color: impl Into<Color>,
        stroke_width: f32,
    ) {
        self.primitives.push(Primitive::StrokeRoundRect {
            rect: rect.into(),
            radius,
            color: color.into(),
            stroke_width,
        });
    }

    /// 记录文本，锚点为其左上角位置。
    pub fn draw_text(
        &mut self,
        point: crate::math::Point,
        text: &str,
        font_size: f32,
        color: impl Into<Color>,
    ) {
        self.primitives.push(Primitive::Text {
            point,
            text: text.to_string(),
            font_size,
            color: color.into(),
        });
    }

    /// 记录一段已排版文本（对标 CanvasKit `drawParagraph`）。
    ///
    /// `point` 是段落原点（左上角），不是基线：行的基线、行内对齐偏移都由
    /// `layout` 自己给出，调用方不再参与。
    pub fn draw_paragraph(
        &mut self,
        point: crate::math::Point,
        layout: impl Into<ParagraphHandle>,
    ) {
        self.primitives.push(Primitive::Paragraph {
            point,
            layout: layout.into(),
        });
    }

    /// 绘制图片并缩放到目标矩形（对标 Canvas 2D `drawImage(source, x, y, w, h)`）。
    pub fn draw_image(&mut self, image: PlatformImage, rect: impl Into<Rect>) {
        self.primitives.push(Primitive::Image {
            image,
            rect: rect.into(),
        });
    }

    /// 记录一条折线（`stroke_width > 0` 描边）。
    ///
    /// 顶点少于 2 个时忽略。坐标为局部坐标，会被 `push_transform` 平移。
    pub fn stroke_path(
        &mut self,
        points: impl IntoIterator<Item = crate::math::Point>,
        stroke_width: f32,
        color: impl Into<Color>,
    ) {
        let points: Vec<crate::math::Point> = points.into_iter().collect();
        if points.len() < 2 || stroke_width <= 0.0 {
            return;
        }
        self.primitives.push(Primitive::Path {
            points,
            closed: false,
            stroke_width,
            color: color.into(),
        });
    }

    /// 记录一个闭合多边形（填充）。
    ///
    /// 顶点少于 3 个时忽略。
    pub fn fill_path(
        &mut self,
        points: impl IntoIterator<Item = crate::math::Point>,
        color: impl Into<Color>,
    ) {
        let points: Vec<crate::math::Point> = points.into_iter().collect();
        if points.len() < 3 {
            return;
        }
        self.primitives.push(Primitive::Path {
            points,
            closed: true,
            stroke_width: 0.0,
            color: color.into(),
        });
    }

    /// 裁剪入栈：后续图元将被限制在该矩形内。
    pub fn push_clip(&mut self, rect: impl Into<Rect>) {
        self.primitives
            .push(Primitive::ClipPush { rect: rect.into() });
    }

    /// 圆角裁剪入栈：后续图元将被限制在该圆角矩形内。
    pub fn push_round_clip(&mut self, rect: impl Into<Rect>, radius: f32) {
        self.primitives.push(Primitive::RoundClipPush {
            rect: rect.into(),
            radius,
        });
    }

    /// 裁剪出栈：恢复裁剪前的状态。
    pub fn pop_clip(&mut self) {
        self.primitives.push(Primitive::ClipPop);
    }

    /// 坐标变换入栈：后续图元的坐标将加上偏移量。
    pub fn push_transform(&mut self, offset: crate::math::Point) {
        self.primitives.push(Primitive::TransformPush { offset });
    }

    /// 坐标变换出栈：恢复变换前的状态。
    pub fn pop_transform(&mut self) {
        self.primitives.push(Primitive::TransformPop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Point, Rect};

    #[test]
    fn scene_records_primitives_in_order() {
        let mut s = Scene::new();
        assert!(s.is_empty());

        s.fill_rect(Rect::new(1.0, 2.0, 3.0, 4.0), Color::RED);
        s.fill_round_rect(Rect::new(5.0, 6.0, 7.0, 8.0), 2.0, Color::BLUE);
        s.draw_text(Point::new(0.0, 1.0), "hi", 14.0, Color::BLACK);
        s.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
        s.pop_clip();
        s.push_transform(Point::new(5.0, 10.0));
        s.pop_transform();

        assert_eq!(s.len(), 7);
        assert!(!s.is_empty());
        let items: Vec<_> = s.iter().collect();
        assert!(matches!(items[0], Primitive::Rect { .. }));
        assert!(matches!(items[1], Primitive::RoundRect { .. }));
        assert!(matches!(items[2], Primitive::Text { .. }));
        assert!(matches!(items[3], Primitive::ClipPush { .. }));
        assert!(matches!(items[4], Primitive::ClipPop));
        assert!(matches!(items[5], Primitive::TransformPush { .. }));
        assert!(matches!(items[6], Primitive::TransformPop));
    }

    #[test]
    fn scene_clear_resets() {
        let mut s = Scene::new();
        s.fill_rect(Rect::zero(), Color::WHITE);
        assert_eq!(s.len(), 1);
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn scene_rect_stores_geometry_and_color() {
        let mut s = Scene::new();
        s.fill_rect(Rect::new(1.0, 2.0, 3.0, 4.0), 0x11223344);
        let first = s.iter().next().cloned().expect("one primitive");
        match first {
            Primitive::Rect { rect, color } => {
                assert_eq!(rect, Rect::new(1.0, 2.0, 3.0, 4.0));
                assert_eq!(color.to_u32(), 0x11223344);
            }
            other => panic!("unexpected primitive: {other:?}"),
        }
    }

    #[test]
    fn stroke_path_records_points() {
        let mut s = Scene::new();
        s.stroke_path(
            [Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
            2.0,
            Color::RED,
        );
        let items: Vec<_> = s.iter().cloned().collect();
        match &items[0] {
            Primitive::Path {
                points,
                closed,
                stroke_width,
                ..
            } => {
                assert_eq!(points.len(), 2);
                assert!(!closed);
                assert_eq!(*stroke_width, 2.0);
            }
            other => panic!("unexpected primitive: {other:?}"),
        }
    }

    #[test]
    fn path_ignores_degenerate_input() {
        let mut s = Scene::new();
        // 少于 2 个点的折线、少于 3 个点的多边形都不应产生图元
        s.stroke_path([Point::new(0.0, 0.0)], 2.0, Color::RED);
        s.fill_path([Point::new(0.0, 0.0), Point::new(1.0, 1.0)], Color::RED);
        assert!(s.is_empty());
    }

    #[test]
    fn fill_path_is_closed() {
        let mut s = Scene::new();
        s.fill_path(
            [
                Point::new(0.0, 0.0),
                Point::new(4.0, 0.0),
                Point::new(4.0, 4.0),
            ],
            Color::BLUE,
        );
        let items: Vec<_> = s.iter().cloned().collect();
        match &items[0] {
            Primitive::Path { points, closed, .. } => {
                assert_eq!(points.len(), 3);
                assert!(*closed);
            }
            other => panic!("unexpected primitive: {other:?}"),
        }
    }

    #[test]
    fn image_and_round_clip_are_recorded_in_order() {
        let image = PlatformImage::from_rgba8(1, 1, vec![255, 0, 0, 255]).unwrap();
        let mut s = Scene::new();
        let rect = Rect::new(2.0, 3.0, 20.0, 10.0);
        s.push_round_clip(rect, 4.0);
        s.draw_image(image.clone(), rect);
        s.pop_clip();

        let items: Vec<_> = s.iter().collect();
        assert!(matches!(items[0], Primitive::RoundClipPush { .. }));
        match items[1] {
            Primitive::Image {
                image: actual,
                rect: actual_rect,
            } => {
                assert_eq!(actual, &image);
                assert_eq!(*actual_rect, rect);
            }
            other => panic!("unexpected primitive: {other:?}"),
        }
        assert!(matches!(items[2], Primitive::ClipPop));
    }

    #[test]
    fn invalid_rgba_length_is_rejected() {
        assert!(PlatformImage::from_rgba8(2, 2, vec![0; 15]).is_none());
    }
}
