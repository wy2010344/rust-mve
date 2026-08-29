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
    /// 滚轮。
    Wheel,
}

/// 统一指针事件：鼠标 / 触摸 / 触控笔。
#[derive(Debug)]
pub struct PointerEvent {
    /// 事件类型。
    pub pointer_type: PointerType,
    /// 局部 X 坐标。
    pub x: f32,
    /// 局部 Y 坐标。
    pub y: f32,
    /// 鼠标按键状态（位掩码：左=1, 右=2, 中=4）。
    pub buttons: i32,
    /// 滚轮增量（仅 Wheel 事件）。
    pub wheel_delta: f32,
    /// 是否已停止传播。
    stopped: bool,
}

impl PointerEvent {
    /// 创建指针事件。
    pub fn new(pointer_type: PointerType, x: f32, y: f32) -> Self {
        Self {
            pointer_type,
            x,
            y,
            buttons: 0,
            wheel_delta: 0.0,
            stopped: false,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_event_basic() {
        let e = PointerEvent::new(PointerType::Down, 10.0, 20.0);
        assert_eq!(e.pointer_type, PointerType::Down);
        assert_eq!(e.x, 10.0);
        assert_eq!(e.y, 20.0);
        assert!(!e.is_propagation_stopped());
    }

    #[test]
    fn pointer_event_stop_propagation() {
        let mut e = PointerEvent::new(PointerType::Click, 0.0, 0.0);
        e.stop_propagation();
        assert!(e.is_propagation_stopped());
    }
}
