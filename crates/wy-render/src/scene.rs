//! Scene：平台无关的绘制命令记录器。
//!
//! Widget 的 `draw()` 方法向 Scene 添加高层图元，最终由 Vello/wgpu 提交 GPU。
//! Scene 不依赖任何 GPU 资源，可在任意线程记录，作为渲染管线的中间表示。

use crate::color::Color;
use crate::math::Rect;

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
    /// 文本。
    Text {
        /// 文本锚点（左上角基线位置）。
        point: crate::math::Point,
        text: String,
        font_size: f32,
        color: Color,
    },
    /// 裁剪入栈。
    ClipPush { rect: Rect },
    /// 裁剪出栈。
    ClipPop,
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

    /// 裁剪入栈：后续图元将被限制在该矩形内。
    pub fn push_clip(&mut self, rect: impl Into<Rect>) {
        self.primitives
            .push(Primitive::ClipPush { rect: rect.into() });
    }

    /// 裁剪出栈：恢复裁剪前的状态。
    pub fn pop_clip(&mut self) {
        self.primitives.push(Primitive::ClipPop);
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

        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
        let items: Vec<_> = s.iter().collect();
        assert!(matches!(items[0], Primitive::Rect { .. }));
        assert!(matches!(items[1], Primitive::RoundRect { .. }));
        assert!(matches!(items[2], Primitive::Text { .. }));
        assert!(matches!(items[3], Primitive::ClipPush { .. }));
        assert!(matches!(items[4], Primitive::ClipPop));
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
}
