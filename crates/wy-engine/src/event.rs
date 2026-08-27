/// UI 事件：鼠标、键盘、触摸等。
pub enum Event {
    /// 鼠标按下。
    MouseDown { x: f32, y: f32 },
    /// 鼠标移动。
    MouseMove { x: f32, y: f32 },
    /// 鼠标释放。
    MouseUp { x: f32, y: f32 },
    /// 鼠标滚轮。
    Wheel { delta_x: f32, delta_y: f32 },
    /// 键盘按下。
    KeyDown { key: Key },
    /// 键盘释放。
    KeyUp { key: Key },
}

/// 键码。
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Tab,
    Escape,
}
