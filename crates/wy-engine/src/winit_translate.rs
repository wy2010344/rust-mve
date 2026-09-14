//! winit 事件到 wy-engine 事件的翻译层。
//!
//! 将 winit 的 `WindowEvent` 翻译为统一的 [`Event`] 枚举，
//! 供应用层的命中测试和事件分发使用。

use crate::event::{Event, Key, KeyEvent, PointerEvent, PointerType};

/// 从 winit `WindowEvent` 翻译为 wy-engine `Event`。
///
/// 返回 `None` 表示该事件不产生可分发的 UI 事件
/// （如 `CloseRequested`、`Resized` 等，需要在调用方直接处理）。
pub fn translate_window_event(
    event: &winit::event::WindowEvent,
    modifiers: &winit::keyboard::ModifiersState,
    cursor_pos: (f32, f32),
) -> Option<Event> {
    match event {
        winit::event::WindowEvent::CursorMoved { position, .. } => Some(Event::Pointer(
            PointerEvent::new(PointerType::Move, position.x as f32, position.y as f32),
        )),
        winit::event::WindowEvent::MouseInput { state, button, .. } => {
            let pointer_type = match state {
                winit::event::ElementState::Pressed => PointerType::Down,
                winit::event::ElementState::Released => PointerType::Up,
            };
            let buttons = match button {
                winit::event::MouseButton::Left => 1,
                winit::event::MouseButton::Right => 2,
                winit::event::MouseButton::Middle => 4,
                _ => 0,
            };
            let mut e = PointerEvent::new(pointer_type, cursor_pos.0, cursor_pos.1);
            e.buttons = buttons;
            Some(Event::Pointer(e))
        }
        winit::event::WindowEvent::MouseWheel { delta, .. } => {
            let wheel_delta = match delta {
                winit::event::MouseScrollDelta::LineDelta(_, y) => *y,
                winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
            };
            let mut e = PointerEvent::new(PointerType::Wheel, cursor_pos.0, cursor_pos.1);
            e.wheel_delta = wheel_delta;
            Some(Event::Pointer(e))
        }
        winit::event::WindowEvent::KeyboardInput {
            event: key_event, ..
        } => {
            let key = translate_key(&key_event.logical_key);
            let pressed = matches!(key_event.state, winit::event::ElementState::Pressed);
            let m = translate_modifiers(modifiers);
            let e = KeyEvent::new(key, pressed).with_modifiers(
                m & 0x2 != 0,
                m & 0x1 != 0,
                m & 0x4 != 0,
                m & 0x8 != 0,
            );
            Some(Event::Key(e))
        }
        _ => None,
    }
}

/// 将 winit `ModifiersState` 翻译为统一 modifiers 位掩码。
///
/// 位掩码：shift=0x1, ctrl=0x2, alt=0x4, meta=0x8
pub fn translate_modifiers(state: &winit::keyboard::ModifiersState) -> u32 {
    let mut m = 0u32;
    if state.shift_key() {
        m |= 0x1;
    }
    if state.control_key() {
        m |= 0x2;
    }
    if state.alt_key() {
        m |= 0x4;
    }
    if state.super_key() {
        m |= 0x8;
    }
    m
}

/// 从 winit `KeyEvent` + `ModifiersState` 翻译为统一 [`KeyEvent`]。
pub fn translate_key_event(
    key_event: &winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<crate::event::KeyEvent> {
    let key = translate_key(&key_event.logical_key);
    let pressed = matches!(key_event.state, winit::event::ElementState::Pressed);
    let m = translate_modifiers(&modifiers);
    Some(crate::event::KeyEvent::new(key, pressed).with_modifiers(
        m & 0x2 != 0,
        m & 0x1 != 0,
        m & 0x4 != 0,
        m & 0x8 != 0,
    ))
}

/// 将 winit `Key` 翻译为统一 `Key` 枚举。
fn translate_key(key: &winit::keyboard::Key) -> Key {
    use winit::keyboard::{Key as WKey, NamedKey};
    match key {
        WKey::Named(NamedKey::Tab) => Key::Tab,
        WKey::Named(NamedKey::Enter) => Key::Enter,
        WKey::Named(NamedKey::Escape) => Key::Escape,
        WKey::Named(NamedKey::Space) => Key::Char(' '),
        WKey::Named(NamedKey::ArrowUp) => Key::ArrowUp,
        WKey::Named(NamedKey::ArrowDown) => Key::ArrowDown,
        WKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WKey::Named(NamedKey::Backspace) => Key::Backspace,
        WKey::Named(NamedKey::Delete) => Key::Delete,
        WKey::Named(NamedKey::Home) => Key::Home,
        WKey::Named(NamedKey::End) => Key::End,
        WKey::Named(NamedKey::PageUp) => Key::PageUp,
        WKey::Named(NamedKey::PageDown) => Key::PageDown,
        WKey::Character(c) => {
            let s = c.as_ref();
            if s.len() == 1 {
                Key::Char(s.chars().next().unwrap())
            } else {
                Key::Char('\0')
            }
        }
        _ => Key::Char('\0'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_modifiers_none() {
        let state = winit::keyboard::ModifiersState::empty();
        assert_eq!(translate_modifiers(&state), 0);
    }

    #[test]
    fn translate_modifiers_ctrl_shift() {
        let state =
            winit::keyboard::ModifiersState::CONTROL | winit::keyboard::ModifiersState::SHIFT;
        let m = translate_modifiers(&state);
        assert!(m & 0x1 != 0); // shift
        assert!(m & 0x2 != 0); // ctrl
        assert!(m & 0x4 == 0); // no alt
    }

    #[test]
    fn translate_key_enter() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter);
        assert_eq!(translate_key(&key), Key::Enter);
    }

    #[test]
    fn translate_key_char() {
        let key = winit::keyboard::Key::Character("a".into());
        assert_eq!(translate_key(&key), Key::Char('a'));
    }

    #[test]
    fn translate_cursor_moved() {
        let event = winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(100.0, 200.0),
        };
        let mods = winit::keyboard::ModifiersState::empty();
        let result = translate_window_event(&event, &mods, (0.0, 0.0));
        assert!(result.is_some());
        match result.unwrap() {
            Event::Pointer(e) => {
                assert_eq!(e.pointer_type, PointerType::Move);
                assert_eq!(e.x, 100.0);
                assert_eq!(e.y, 200.0);
            }
            _ => panic!("expected Pointer"),
        }
    }

    #[test]
    fn translate_mouse_input() {
        let event = winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Left,
        };
        let mods = winit::keyboard::ModifiersState::empty();
        let result = translate_window_event(&event, &mods, (50.0, 60.0));
        assert!(result.is_some());
        match result.unwrap() {
            Event::Pointer(e) => {
                assert_eq!(e.pointer_type, PointerType::Down);
                assert_eq!(e.buttons, 1);
                assert_eq!(e.x, 50.0);
                assert_eq!(e.y, 60.0);
            }
            _ => panic!("expected Pointer"),
        }
    }

    #[test]
    fn translate_key_backspace() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace);
        assert_eq!(translate_key(&key), Key::Backspace);
    }

    #[test]
    fn translate_key_escape() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape);
        assert_eq!(translate_key(&key), Key::Escape);
    }

    // ===== 对齐 Kotlin Engine 更多按键翻译 =====

    #[test]
    fn translate_key_tab() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab);
        assert_eq!(translate_key(&key), Key::Tab);
    }

    #[test]
    fn translate_key_delete() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Delete);
        assert_eq!(translate_key(&key), Key::Delete);
    }

    #[test]
    fn translate_key_arrow_up() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowUp);
        assert_eq!(translate_key(&key), Key::ArrowUp);
    }

    #[test]
    fn translate_key_arrow_down() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown);
        assert_eq!(translate_key(&key), Key::ArrowDown);
    }

    #[test]
    fn translate_key_arrow_left() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowLeft);
        assert_eq!(translate_key(&key), Key::ArrowLeft);
    }

    #[test]
    fn translate_key_arrow_right() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowRight);
        assert_eq!(translate_key(&key), Key::ArrowRight);
    }

    #[test]
    fn translate_key_home() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Home);
        assert_eq!(translate_key(&key), Key::Home);
    }

    #[test]
    fn translate_key_end() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::End);
        assert_eq!(translate_key(&key), Key::End);
    }

    #[test]
    fn translate_key_page_up() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::PageUp);
        assert_eq!(translate_key(&key), Key::PageUp);
    }

    #[test]
    fn translate_key_page_down() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::PageDown);
        assert_eq!(translate_key(&key), Key::PageDown);
    }

    #[test]
    fn translate_key_space() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space);
        assert_eq!(translate_key(&key), Key::Char(' '));
    }

    #[test]
    fn translate_modifiers_alt_meta() {
        let state = winit::keyboard::ModifiersState::ALT | winit::keyboard::ModifiersState::SUPER;
        let m = translate_modifiers(&state);
        assert!(m & 0x4 != 0); // alt
        assert!(m & 0x8 != 0); // meta
        assert!(m & 0x1 == 0); // no shift
        assert!(m & 0x2 == 0); // no ctrl
    }

    #[test]
    fn translate_mouse_input_right_button() {
        let event = winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Right,
        };
        let mods = winit::keyboard::ModifiersState::empty();
        let result = translate_window_event(&event, &mods, (10.0, 20.0));
        match result.unwrap() {
            Event::Pointer(e) => assert_eq!(e.buttons, 2),
            _ => panic!("expected Pointer"),
        }
    }

    #[test]
    fn translate_mouse_input_middle_button() {
        let event = winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Released,
            button: winit::event::MouseButton::Middle,
        };
        let mods = winit::keyboard::ModifiersState::empty();
        let result = translate_window_event(&event, &mods, (10.0, 20.0));
        match result.unwrap() {
            Event::Pointer(e) => {
                assert_eq!(e.pointer_type, PointerType::Up);
                assert_eq!(e.buttons, 4);
            }
            _ => panic!("expected Pointer"),
        }
    }

    #[test]
    fn translate_mouse_wheel() {
        let event = winit::event::WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: winit::event::MouseScrollDelta::LineDelta(0.0, 3.0),
            phase: winit::event::TouchPhase::Moved,
        };
        let mods = winit::keyboard::ModifiersState::empty();
        let result = translate_window_event(&event, &mods, (0.0, 0.0));
        match result.unwrap() {
            Event::Pointer(e) => {
                assert_eq!(e.pointer_type, PointerType::Wheel);
                assert_eq!(e.wheel_delta, 3.0);
            }
            _ => panic!("expected Pointer"),
        }
    }
}
