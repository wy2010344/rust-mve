//! 核心事件类型：Widget 交互所需的基础事件。
//!
//! 这些类型定义在 `wy-render` 中（而非 `wy-engine`），以避免循环依赖。
//! `wy-engine` 会重新导出这些类型，用户通常不需要直接引用 `wy_render::event`。

/// 指针事件类型。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PointerType {
    /// 按下。
    Down,
    /// 移动。
    Move,
    /// 释放。
    Up,
    /// 点击（按下 + 释放）。
    Click,
    /// 取消（如触摸被系统中断）。
    Cancel,
    /// 滚轮。
    Wheel,
}

/// 输入设备类型。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PointerDevice {
    /// 鼠标。
    Mouse,
    /// 触摸。
    Touch,
    /// 触控笔。
    Pen,
}

/// 统一指针事件：鼠标 / 触摸 / 触控笔。
///
/// - `x`/`y`：相对当前传播节点的局部坐标（随命中链传播自动换算）
/// - `root_x`/`root_y`：相对渲染根的全局坐标
/// - `id`：指针编号，多点触控 / 多笔场景用于区分
#[derive(Debug)]
pub struct PointerEvent {
    /// 指针编号（多点触控）。
    pub id: i32,
    /// 事件类型。
    pub pointer_type: PointerType,
    /// 输入设备。
    pub device: PointerDevice,
    /// 局部 X 坐标。
    pub x: f32,
    /// 局部 Y 坐标。
    pub y: f32,
    /// 全局 X 坐标（相对渲染根）。
    pub root_x: f32,
    /// 全局 Y 坐标（相对渲染根）。
    pub root_y: f32,
    /// 鼠标按键状态（位掩码：左=1, 右=2, 中=4）。
    pub buttons: i32,
    /// 压力（0.0-1.0，触摸/笔）。
    pub pressure: f32,
    /// 滚轮增量（仅 Wheel 事件）。
    pub wheel_delta: f32,
    /// 是否已停止传播。
    stopped: bool,
}

impl PointerEvent {
    /// 创建指针事件。
    pub fn new(pointer_type: PointerType, x: f32, y: f32) -> Self {
        Self {
            id: 0,
            pointer_type,
            device: PointerDevice::Mouse,
            x,
            y,
            root_x: 0.0,
            root_y: 0.0,
            buttons: 0,
            pressure: 1.0,
            wheel_delta: 0.0,
            stopped: false,
        }
    }

    /// 设置全局坐标。
    pub fn with_root_position(mut self, root_x: f32, root_y: f32) -> Self {
        self.root_x = root_x;
        self.root_y = root_y;
        self
    }

    /// 设置设备类型。
    pub fn with_device(mut self, device: PointerDevice) -> Self {
        self.device = device;
        self
    }

    /// 设置指针 ID。
    pub fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
    }

    /// 停止事件传播（中断命中链）。
    pub fn stop_propagation(&mut self) {
        self.stopped = true;
    }

    /// 事件是否已被停止传播。
    pub fn is_propagation_stopped(&self) -> bool {
        self.stopped
    }
}

/// 键码。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// 字符键。
    Char(char),
    /// 回车。
    Enter,
    /// 退格。
    Backspace,
    /// 删除。
    Delete,
    /// 上箭头。
    ArrowUp,
    /// 下箭头。
    ArrowDown,
    /// 左箭头。
    ArrowLeft,
    /// 右箭头。
    ArrowRight,
    /// Tab。
    Tab,
    /// Escape。
    Escape,
    /// Home。
    Home,
    /// End。
    End,
    /// Page Up。
    PageUp,
    /// Page Down。
    PageDown,
}

/// 键盘事件。
#[derive(Debug)]
pub struct KeyEvent {
    /// 键码。
    pub key: Key,
    /// 是否按下（false = 释放）。
    pub pressed: bool,
    /// 是否按住 Ctrl。
    pub ctrl: bool,
    /// 是否按住 Shift。
    pub shift: bool,
    /// 是否按住 Alt。
    pub alt: bool,
    /// 是否按住 Meta（Win/Cmd）。
    pub meta: bool,
}

impl KeyEvent {
    /// 创建键盘事件。
    pub fn new(key: Key, pressed: bool) -> Self {
        Self {
            key,
            pressed,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    /// 设置修饰键状态。
    pub fn with_modifiers(mut self, ctrl: bool, shift: bool, alt: bool, meta: bool) -> Self {
        self.ctrl = ctrl;
        self.shift = shift;
        self.alt = alt;
        self.meta = meta;
        self
    }
}

/// UI 事件枚举：所有事件类型的统一包装。
#[derive(Debug)]
pub enum Event {
    /// 指针事件。
    Pointer(PointerEvent),
    /// 键盘事件。
    Key(KeyEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_event_basic() {
        let e = PointerEvent::new(PointerType::Down, 10.0, 20.0);
        assert_eq!(e.pointer_type, PointerType::Down);
        assert_eq!(e.x, 10.0);
        assert_eq!(e.y, 20.0);
        assert_eq!(e.id, 0);
        assert_eq!(e.device, PointerDevice::Mouse);
        assert!(!e.is_propagation_stopped());
    }

    #[test]
    fn pointer_event_stop_propagation() {
        let mut e = PointerEvent::new(PointerType::Click, 0.0, 0.0);
        assert!(!e.is_propagation_stopped());
        e.stop_propagation();
        assert!(e.is_propagation_stopped());
    }

    #[test]
    fn pointer_event_builder_chain() {
        let e = PointerEvent::new(PointerType::Move, 5.0, 5.0)
            .with_root_position(100.0, 200.0)
            .with_device(PointerDevice::Touch)
            .with_id(42);
        assert_eq!(e.root_x, 100.0);
        assert_eq!(e.root_y, 200.0);
        assert_eq!(e.device, PointerDevice::Touch);
        assert_eq!(e.id, 42);
    }

    #[test]
    fn key_event_basic() {
        let e = KeyEvent::new(Key::Enter, true);
        assert_eq!(e.key, Key::Enter);
        assert!(e.pressed);
        assert!(!e.ctrl);
    }

    #[test]
    fn key_event_with_modifiers() {
        let e = KeyEvent::new(Key::Char('c'), true).with_modifiers(true, false, false, false);
        assert!(e.ctrl);
        assert!(!e.shift);
    }

    #[test]
    fn event_enum_wraps_pointer_and_key() {
        let pe = Event::Pointer(PointerEvent::new(PointerType::Up, 0.0, 0.0));
        let ke = Event::Key(KeyEvent::new(Key::Escape, true));
        match pe {
            Event::Pointer(e) => assert_eq!(e.pointer_type, PointerType::Up),
            _ => panic!("expected Pointer"),
        }
        match ke {
            Event::Key(e) => assert_eq!(e.key, Key::Escape),
            _ => panic!("expected Key"),
        }
    }
}
