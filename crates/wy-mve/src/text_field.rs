//! `TextField`：单行文本输入组件（复刻 Kotlin `EditableTextNode` + `TextField`）。
//!
//! 薄壳设计：编辑内核（光标 / 选区 / IME / 导航 / 删除 / 几何）全部由
//! [`wy_text::editable::EditableParagraph`]（Parley `PlainEditor`）承担，
//! 本组件只负责：
//! - **绘制**：背景 / 边框（聚焦高亮）/ 文本 / 占位符 / 选区高亮 / 光标 / IME 下划线
//! - **事件转发**：点击定位（`move_to_point`）、键盘导航与编辑、IME 组合态
//! - **值同步**：编辑后写回外部 `on_change`；外部 `value` 变化时重灌编辑器
//!
//! 声明式构造：内容由外部信号/闭包驱动。绘制在组件**本地坐标**。

use std::cell::RefCell;
use std::rc::Rc;

use wy_render::{Color, Point, Rect, Scene};
use wy_text::EditableParagraph;

use crate::node::{ImeEvent, Key, KeyEvent, Node, PointerEvent};

/// 创建文本输入组件（默认配置）。
///
/// ```ignore
/// let text = Signal::new(String::new());
/// cx.child(text_field(
///     move || text.get(),
///     move |t| text.set(t),
/// ));
/// ```
pub fn text_field(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
) -> Node {
    text_field_opts(value, on_change, TextFieldOpts::default())
}

/// 文本输入配置。
#[derive(Clone)]
pub struct TextFieldOpts {
    /// 输入框宽度。
    pub width: f32,
    /// 输入框高度。
    pub height: f32,
    /// 字号。
    pub font_size: f32,
    /// 占位符。
    pub placeholder: String,
    /// 文本颜色。
    pub text_color: Color,
    /// 占位符颜色。
    pub placeholder_color: Color,
    /// 背景色。
    pub bg_color: Color,
    /// 常规边框色。
    pub border_color: Color,
    /// 聚焦边框色。
    pub focus_border_color: Color,
    /// 密码模式（掩码显示）。
    pub password: bool,
}

impl Default for TextFieldOpts {
    fn default() -> Self {
        Self {
            width: 240.0,
            height: 34.0,
            font_size: 14.0,
            placeholder: String::new(),
            text_color: Color::BLACK,
            placeholder_color: Color::rgba(160, 160, 160, 255),
            bg_color: Color::WHITE,
            border_color: Color::rgba(180, 180, 180, 255),
            focus_border_color: Color::rgba(0, 120, 212, 255),
            password: false,
        }
    }
}

/// 组件共享运行时状态。
struct SharedState {
    /// 编辑内核（真源）。
    editable: EditableParagraph,
    /// 是否聚焦。
    focused: bool,
    /// 上次已同步的外部文本（防回环与重复重灌）。
    last_synced: String,
}

type Shared = Rc<RefCell<SharedState>>;

/// 文本绘制左内边距。
const PAD_X: f32 = 6.0;
/// 文本垂直居中（绘制为基线顶部位置）。
fn text_top(opts: &TextFieldOpts) -> f32 {
    (opts.height - opts.font_size) / 2.0
}

/// 掩码显示文本（真实文本 → 等长 `*`）。
fn mask(text: &str) -> String {
    "*".repeat(text.chars().count())
}

/// 创建带完整配置的文本输入组件。
pub fn text_field_opts(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
    opts: TextFieldOpts,
) -> Node {
    // 编辑器预灌当前外部文本（构建期一次）。
    let initial = value();
    let font_size = opts.font_size;
    let shared: Shared = Rc::new(RefCell::new(SharedState {
        editable: EditableParagraph::new(font_size),
        focused: false,
        last_synced: initial.clone(),
    }));
    {
        let mut s = shared.borrow_mut();
        s.editable.replace_with(&initial);
    }

    let value = Rc::new(value);
    let on_change = Rc::new(on_change);
    let opts = Rc::new(opts);

    // --- 外部文本 → 编辑器（draw 前同步；外部变化重灌，光标复位） ---
    let sync_fn = {
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        Rc::new(move || {
            let mut s = shared.borrow_mut();
            let ext = value();
            if ext != s.last_synced {
                s.editable.replace_with(&ext);
                s.last_synced = ext;
            }
        })
    };

    // --- 绘制 ---
    let draw_fn = {
        let shared = Rc::clone(&shared);
        let sync = Rc::clone(&sync_fn);
        let opts = Rc::clone(&opts);
        Rc::new(move |scene: &mut dyn std::any::Any| {
            let Some(scene) = scene.downcast_mut::<Scene>() else {
                return;
            };
            sync();
            let s = shared.borrow();
            let border = if s.focused {
                opts.focus_border_color
            } else {
                opts.border_color
            };
            scene.fill_rect(Rect::new(0.0, 0.0, opts.width, opts.height), opts.bg_color);
            scene.stroke_round_rect(
                Rect::new(0.0, 0.0, opts.width, opts.height),
                4.0,
                border,
                1.0,
            );

            let top = text_top(&opts);
            let text = s.editable.text();

            // 密码掩码：显示文本与几何都走掩码（同字素 count）。
            let (display, disp_len) = if opts.password {
                let m = mask(&text);
                let n = m.len();
                (m, n)
            } else {
                let n = text.len();
                (text, n)
            };

            // 文本或占位符
            let (shown, color) = if display.is_empty() && !opts.placeholder.is_empty() {
                (opts.placeholder.as_str(), opts.placeholder_color)
            } else {
                (display.as_str(), opts.text_color)
            };
            if !shown.is_empty() {
                scene.draw_text(Point::new(PAD_X, top), shown, opts.font_size, color);
            }

            // 选区高亮（Enter 到编辑器的几何，掩码时按掩码重算）
            let anchor = s.editable.anchor().min(disp_len);
            let cursor = s.editable.cursor().min(disp_len);
            let (lo, hi) = (anchor.min(cursor), anchor.max(cursor));
            if lo < hi {
                let ax = PAD_X + cursor_x(&display, opts.font_size, lo);
                let bx = PAD_X + cursor_x(&display, opts.font_size, hi);
                scene.fill_rect(
                    Rect::new(ax, top, bx - ax, opts.font_size),
                    Color::rgba(0, 120, 212, 70),
                );
            }

            // 光标（聚焦 + 未组合时显示）
            if s.focused && !s.editable.is_composing() {
                let cx = PAD_X + cursor_x(&display, opts.font_size, cursor);
                scene.fill_rect(Rect::new(cx, top, 1.0, opts.font_size), opts.text_color);
            }

            // IME 组合带下划线
            if let Some(range) = s.editable.composing_range() {
                let a = PAD_X + cursor_x(&display, opts.font_size, range.start.min(disp_len));
                let b = PAD_X + cursor_x(&display, opts.font_size, range.end.min(disp_len));
                let y = top + opts.font_size + 1.0;
                scene.fill_rect(Rect::new(a, y, b - a, 1.0), opts.text_color);
            }
        })
    };

    let hit_test_fn = Rc::new(move |_x: f32, _y: f32| true);

    // --- 点击：聚焦 + 定位光标 ---
    let on_down_fn = {
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut PointerEvent| {
            let mut s = shared.borrow_mut();
            s.focused = true;
            s.editable
                .move_to_point(event.x - PAD_X, text_top(&opts), false);
        })
    };

    // --- 键盘导航/编辑 ---
    let key_fn = {
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut KeyEvent| -> bool {
            let mut s = shared.borrow_mut();
            let handled = editable_handle_key(&mut s, &opts, event);
            if handled {
                push_change(&mut s, &*value, &*on_change);
            }
            handled
        })
    };

    // --- IME 组合态 ---
    let ime_fn = {
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        Rc::new(move |event: &mut ImeEvent| -> bool {
            let mut s = shared.borrow_mut();
            let handled = editable_handle_ime(&mut s, event);
            if handled {
                push_change(&mut s, &*value, &*on_change);
            }
            handled
        })
    };

    Node {
        draw_fn,
        hit_test_fn,
        on_down_fn: Some(on_down_fn),
        key_fn: Some(key_fn),
        ime_fn: Some(ime_fn),
        focusable: true,
        width: opts.width,
        height: opts.height,
        ..Node::default()
    }
}

/// 编辑后把快照写回外部（仅在文本变化时调用 `on_change`）。
fn push_change(s: &mut SharedState, value: &dyn Fn() -> String, on_change: &dyn Fn(String)) {
    let snap = s.editable.snapshot();
    s.last_synced = snap.text.clone();
    if snap.text != value() {
        on_change(snap.text);
    }
}

/// 键盘事件 → 编辑内核操作；返回是否消费。
///
/// 纯逻辑，可单测。`Ctrl+Z`（撤销）留待二期（Kotlin 用 undoManager）。
fn editable_handle_key(s: &mut SharedState, opts: &TextFieldOpts, event: &KeyEvent) -> bool {
    let ctrl_or_meta = event.ctrl || event.meta;
    let shift = event.shift;

    // Ctrl/Meta 组合
    if ctrl_or_meta {
        // 需要能引用 editable 的 driver
        return handle_ctrl_shortcut(s, event);
    }

    let _ = opts;
    match event.key {
        Key::Char(ch) if !ctrl_or_meta && !shift => {
            s.editable.with_driver(|mut drv| {
                drv.insert_or_replace_selection(&ch.to_string());
            });
            true
        }
        Key::Backspace => {
            if ctrl_or_meta {
                s.editable.with_driver(|mut drv| {
                    if shift {
                        // Cmd+Backspace 删行（macOS）；Rust 侧先作普通词删除
                        drv.backdelete_word();
                    } else {
                        drv.backdelete_word();
                    }
                });
            } else {
                s.editable.with_driver(|mut drv| drv.backdelete());
            }
            true
        }
        Key::Delete => {
            if ctrl_or_meta {
                s.editable.with_driver(|mut drv| drv.delete_word());
            } else {
                s.editable.with_driver(|mut drv| drv.delete());
            }
            true
        }
        Key::ArrowLeft => {
            if ctrl_or_meta {
                if shift {
                    s.editable.with_driver(|mut drv| drv.select_word_left());
                } else {
                    s.editable.with_driver(|mut drv| drv.move_word_left());
                }
            } else if shift {
                s.editable.with_driver(|mut drv| drv.select_left());
            } else {
                s.editable.with_driver(|mut drv| drv.move_left());
            }
            true
        }
        Key::ArrowRight => {
            if ctrl_or_meta {
                if shift {
                    s.editable.with_driver(|mut drv| drv.select_word_right());
                } else {
                    s.editable.with_driver(|mut drv| drv.move_word_right());
                }
            } else if shift {
                s.editable.with_driver(|mut drv| drv.select_right());
            } else {
                s.editable.with_driver(|mut drv| drv.move_right());
            }
            true
        }
        Key::Home => {
            if shift {
                s.editable.with_driver(|mut drv| drv.select_to_line_start());
            } else {
                s.editable.with_driver(|mut drv| drv.move_to_line_start());
            }
            true
        }
        Key::End => {
            if shift {
                s.editable.with_driver(|mut drv| drv.select_to_line_end());
            } else {
                s.editable.with_driver(|mut drv| drv.move_to_line_end());
            }
            true
        }
        // 未处理的控制键（Enter / Tab / Escape / PageUp/Down）交给上层
        _ => false,
    }
}

/// Ctrl 快捷键：A 全选 / C 复制 / X 剪切 / V 粘贴。
fn handle_ctrl_shortcut(_s: &mut SharedState, _event: &KeyEvent) -> bool {
    // TODO(二期)：剪贴板经 wy-mve::clipboard 桥 + 全选/复制/剪切/粘贴。
    // 目前编辑内核接管已知操作返回 false，其余打印日志避免误消费。
    false
}

/// IME 组合事件 → 编辑内核。
fn editable_handle_ime(s: &mut SharedState, event: &ImeEvent) -> bool {
    match event {
        ImeEvent::Enabled => true,
        ImeEvent::Disabled => {
            s.editable.with_driver(|mut drv| drv.finish_compose());
            true
        }
        ImeEvent::Preedit { text, cursor } => {
            if text.is_empty() {
                s.editable.with_driver(|mut drv| drv.clear_compose());
            } else {
                // cursor 即组合带内区间 (start,end)，透传给 parley set_compose
                let cur = cursor.filter(|(_, e)| *e <= text.len());
                s.editable.with_driver(|mut drv| drv.set_compose(text, cur));
            }
            true
        }
        ImeEvent::Commit(_text) => {
            // parley 组合已把文本并入 buffer；finish_compose 标记提交（选区不动）。
            s.editable.with_driver(|mut drv| drv.finish_compose());
            true
        }
    }
}

/// 光标 X 坐标（本地前缀宽度）——复用 `wy_render::text_measure`。
fn cursor_x(text: &str, font_size: f32, text_idx: usize) -> f32 {
    wy_render::text_measure::cursor_x(text, font_size, text_idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // --- 纯逻辑：editable_handle_key / editable_handle_ime ---

    fn shared(text: &str, font_size: f32) -> SharedState {
        let mut e = EditableParagraph::new(font_size);
        e.replace_with(text);
        SharedState {
            editable: e,
            focused: true,
            last_synced: text.to_string(),
        }
    }

    fn key(k: Key, ctrl: bool, shift: bool) -> KeyEvent {
        KeyEvent {
            key: k,
            ctrl,
            shift,
            alt: false,
            meta: false,
        }
    }

    #[test]
    fn char_insert_in_the_middle() {
        let mut s = shared("hi", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = key(Key::Char('x'), false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "hxi");
        assert_eq!(s.editable.cursor(), 2);
    }

    #[test]
    fn backspace_deletes_grapheme() {
        let mut s = shared("a😀b", 14.0);
        // a(1) + 😀(4) = 5 字节处是 b 前
        s.editable.with_driver(|mut drv| drv.move_to_byte(5));
        let ev = key(Key::Backspace, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "ab");
    }

    #[test]
    fn left_arrow_moves_and_collapses() {
        let mut s = shared("hello", 14.0);
        s.editable
            .with_driver(|mut drv| drv.select_byte_range(2, 5));
        let ev = key(Key::ArrowLeft, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        // move_left 在选区非塌缩时塌缩到锚点
        assert!(s.editable.selection_range() == (2, 2));
        let ev = key(Key::ArrowLeft, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.cursor(), 1);
    }

    #[test]
    fn shift_arrow_extends_selection() {
        let mut s = shared("hello", 14.0);
        let ev = key(Key::ArrowRight, false, true);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.selection_range(), (0, 1));
    }

    #[test]
    fn home_end_navigation() {
        let mut s = shared("hello", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(3));
        let ev = key(Key::Home, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.cursor(), 0);
        let ev = key(Key::End, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.cursor(), 5);
    }

    #[test]
    fn enter_tab_escape_not_consumed() {
        let mut s = shared("hi", 14.0);
        for k in [
            Key::Enter,
            Key::Tab,
            Key::Escape,
            Key::PageUp,
            Key::PageDown,
        ] {
            let ev = key(k, false, false);
            assert!(
                !editable_handle_key(&mut s, &default_opts(), &ev),
                "{k:?} 不应消费"
            );
        }
    }

    #[test]
    fn ime_preedit_and_commit() {
        let mut s = shared("a", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: Some((3, 3)),
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(s.editable.is_composing());
        assert_eq!(s.editable.raw_text(), "a中"); // 组合文本含在 raw_text
        let ev = ImeEvent::Commit("中".into());
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(!s.editable.is_composing());
        assert_eq!(s.editable.text(), "a中");
        // a(1) + 中(3) = 4 字节，光标在组合带末尾
        assert_eq!(s.editable.cursor(), 4);
    }

    #[test]
    fn ime_clear_removes_preedit() {
        let mut s = shared("a", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(s.editable.is_composing());
        let ev = ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(!s.editable.is_composing());
        assert_eq!(s.editable.text(), "a");
    }

    #[test]
    fn push_change_writes_only_on_diff() {
        let cell = Rc::new(RefCell::new(String::from("hi")));
        let mut s = shared("hi", 14.0);
        let cell_for_value = Rc::clone(&cell);
        let value = move || cell_for_value.borrow().clone();
        let count = Rc::new(std::cell::Cell::new(0));
        let count_for_cb = Rc::clone(&count);
        let cell_for_cb = Rc::clone(&cell);
        let on_change: Rc<dyn Fn(String)> = Rc::new(move |t: String| {
            count_for_cb.set(count_for_cb.get() + 1);
            *cell_for_cb.borrow_mut() = t;
        });
        // 文本一致：不触发 on_change
        push_change(&mut s, &value, &*on_change);
        assert_eq!(count.get(), 0);
        // 编辑后触发
        s.editable.with_driver(|mut drv| drv.move_to_byte(2));
        s.editable
            .with_driver(|mut drv| drv.insert_or_replace_selection("!"));
        push_change(&mut s, &value, &*on_change);
        assert_eq!(count.get(), 1);
        assert_eq!(cell.borrow().as_str(), "hi!");
    }

    #[test]
    fn mask_preserves_char_count() {
        assert_eq!(mask("héllo"), "*****");
        assert_eq!(mask(""), "");
        assert_eq!(mask("a😀b"), "***");
    }

    fn default_opts() -> TextFieldOpts {
        TextFieldOpts::default()
    }
}
