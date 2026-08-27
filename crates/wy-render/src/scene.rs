/// Scene：平台无关的绘制命令记录器。
///
/// Widget 的 `draw()` 方法向 Scene 添加高层图元，
/// 最终由 Vello/wgpu 提交 GPU。
pub struct Scene {
    primitives: Vec<Primitive>,
}

/// 绘制图元。
pub enum Primitive {
    /// 填充矩形。
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: u32,
    },
    /// 圆角填充矩形。
    RoundRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: u32,
    },
    /// 文本。
    Text {
        x: f32,
        y: f32,
        text: String,
        font_size: f32,
        color: u32,
    },
    /// 裁剪入栈。
    ClipPush { x: f32, y: f32, w: f32, h: f32 },
    /// 裁剪出栈。
    ClipPop,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            primitives: Vec::new(),
        }
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        self.primitives.push(Primitive::Rect { x, y, w, h, color });
    }

    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: u32) {
        self.primitives.push(Primitive::RoundRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        });
    }

    pub fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: u32) {
        self.primitives.push(Primitive::Text {
            x,
            y,
            text: text.to_string(),
            font_size,
            color,
        });
    }

    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.primitives.push(Primitive::ClipPush { x, y, w, h });
    }

    pub fn pop_clip(&mut self) {
        self.primitives.push(Primitive::ClipPop);
    }
}
