/// DrawContext：draw 调用的上下文，提供布局信息。
pub struct DrawContext {
    outer_width: f32,
    outer_height: f32,
    inner_x: f32,
    inner_y: f32,
}

impl DrawContext {
    /// 节点外框宽度。
    pub fn outer_width(&self) -> f32 { self.outer_width }

    /// 节点外框高度。
    pub fn outer_height(&self) -> f32 { self.outer_height }

    /// 内容区 X 偏移（含 padding）。
    pub fn inner_x(&self) -> f32 { self.inner_x }

    /// 内容区 Y 偏移（含 padding）。
    pub fn inner_y(&self) -> f32 { self.inner_y }
}
