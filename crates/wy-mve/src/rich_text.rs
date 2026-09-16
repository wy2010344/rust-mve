//! `RichEditableTextNode`：多行富文本编辑组件（复刻 Kotlin `RichEditableTextNode`）。
//!
//! 薄壳设计：编辑内核（光标 / 选区 / IME / 撤销 / 导航 / 样式段）全部由
//! [`wy_text::EditCore`] 承担，本组件只负责：
//! - **绘制**：背景 / 边框（聚焦高亮）/ 多色文本片段 / 选区高亮 / 光标 / IME 下划线 / 占位符
//! - **事件转发**：点击定位、键盘导航与编辑、IME 组合态
//! - **值同步**：编辑后写回外部 `on_change`；外部 `value` 变化时重灌编辑器
//!
//! 与 Kotlin 相同，编辑内核对外暴露：`rich_editable_opts` 返回共享 `Rc<RefCell<EditCore>>`，
//! 可在组件外直接调用 `core.borrow_mut().buffer_mut().style_range(start, end, style)` 设置样式。

use std::cell::RefCell;
use std::rc::Rc;

use wy_render::{Color, Point, Rect, Scene};
use wy_text::EditCore;

use crate::node::{ImeEvent, Key, KeyEvent, Node, PointerEvent};

/// 文本绘制左内边距。
const PAD_X: f32 = 6.0;
/// 文本垂直居中（绘制为基线顶部位置）。
fn text_top(opts: &RichTextOpts) -> f32 {
    (opts.height - opts.font_size) / 2.0
}

/// 掩码显示文本（真实文本 → 等长 `•`，字符数而非字节数）。
fn mask(text: &str) -> String {
    "•".repeat(text.chars().count())
}

/// 多行富文本编辑组件（默认配置）。
pub fn rich_text(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
) -> Node {
    rich_text_opts(value, on_change, RichTextOpts::default())
}

/// 富文本编辑配置。
#[derive(Clone)]
pub struct RichTextOpts {
    /// 输入框宽度。
    pub width: f32,
    /// 输入框高度。
    pub height: f32,
    /// 字号。
    pub font_size: f32,
    /// 占位符。
    pub placeholder: String,
    /// 文本颜色（占位符/默认片段颜色）。
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

impl Default for RichTextOpts {
    fn default() -> Self {
        Self {
            width: 320.0,
            height: 80.0,
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
    /// 是否聚焦。
    focused: bool,
    /// 上次已同步的外部文本（防回环与重复重灌）。
    last_synced: String,
}

type Shared = Rc<RefCell<SharedState>>;

/// 创建带完整配置的富文本编辑组件（不暴露共享内核）。
pub fn rich_text_opts(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
    opts: RichTextOpts,
) -> Node {
    rich_editable_opts(value, on_change, opts).0
}

/// 创建带完整配置的富文本编辑组件，返回 `(Node, 共享编辑内核)`。
///
/// 共享内核可在组件外直接操作样式段（复刻 Kotlin `RichEditableTextNode` 的
/// `styleRange` 等实例方法）：
///
/// ```ignore
/// let (node, core) = rich_editable_opts(move || doc.get(), move |t| doc.set(t), opts);
/// core.borrow_mut().buffer_mut().style_range(0, 5, Some(bold_style));
/// ```
pub fn rich_editable_opts(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
    opts: RichTextOpts,
) -> (Node, Rc<RefCell<EditCore>>) {
    // 编辑器预灌当前外部文本（构建期一次）。
    let initial = value();
    let core: Rc<RefCell<EditCore>> = Rc::new(RefCell::new(EditCore::with_text(&initial)));
    let shared: Shared = Rc::new(RefCell::new(SharedState {
        focused: false,
        last_synced: initial.clone(),
    }));
    {
        let mut c = core.borrow_mut();
        c.set_placeholder(&opts.placeholder);
        c.set_obscure_text(opts.password);
    }

    let value = Rc::new(value);
    let on_change = Rc::new(on_change);
    let opts = Rc::new(opts);

    // --- 外部文本 → 编辑器（draw 前同步；外部变化重灌，光标复位） ---
    let sync_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        Rc::new(move || {
            let mut s = shared.borrow_mut();
            let ext = value();
            if ext != s.last_synced {
                let mut c = core.borrow_mut();
                c.buffer_mut().write_text(ext.clone());
                let new_len = c.text_len();
                c.set_cursor(new_len);
                s.last_synced = ext;
            }
        })
    };

    // --- 绘制 ---
    let draw_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let sync = Rc::clone(&sync_fn);
        let opts = Rc::clone(&opts);
        Rc::new(move |scene: &mut dyn std::any::Any| {
            let Some(scene) = scene.downcast_mut::<Scene>() else {
                return;
            };
            sync();
            let focused = shared.borrow().focused;
            let core = core.borrow();
            let border = if focused {
                opts.focus_border_color
            } else {
                opts.border_color
            };
            let top = text_top(&opts);

            // 背景 + 边框
            scene.fill_rect(Rect::new(0.0, 0.0, opts.width, opts.height), opts.bg_color);
            scene.stroke_round_rect(
                Rect::new(0.0, 0.0, opts.width, opts.height),
                4.0,
                border,
                1.0,
            );

            // 密码掩码文本显示
            let (shown, is_mask) = if opts.password && !core.showing_placeholder() {
                (mask(core.text()), true)
            } else {
                (core.display_text(), false)
            };

            // 显示片段（每片段独立颜色）
            if !shown.is_empty() {
                let spans = if is_mask {
                    vec![wy_text::TextSpan::styled(
                        shown.clone(),
                        wy_text::TextStyle::normal().with_color(opts.text_color.to_u32()),
                    )]
                } else {
                    core.display_spans(
                        wy_text::TextStyle::normal().with_color(opts.text_color.to_u32()),
                    )
                };
                let mut dx = PAD_X;
                for span in &spans {
                    if span.text.is_empty() {
                        continue;
                    }
                    let color = Color::from_u32(span.style.color);
                    scene.draw_text(Point::new(dx, top), &span.text, opts.font_size, color);
                    let (w, _) = wy_render::text_measure::measure_text(&span.text, opts.font_size);
                    dx += w;
                }
            }

            // 选区高亮
            if core.has_selection() && focused {
                let sel_s = core.logic_to_display_index(core.sel_start());
                let sel_e = core.logic_to_display_index(core.sel_end());
                let text_for_x = if is_mask {
                    shown.as_str()
                } else {
                    &core.display_text()
                };
                let ax = PAD_X + cursor_x_from_display(text_for_x, opts.font_size, sel_s);
                let bx = PAD_X + cursor_x_from_display(text_for_x, opts.font_size, sel_e);
                if bx > ax {
                    scene.fill_rect(
                        Rect::new(ax, top, bx - ax, opts.font_size),
                        Color::rgba(0, 120, 212, 70),
                    );
                }
            }

            // 光标（聚焦 + 未组合时显示）
            if focused && !core.in_composing() {
                let text_for_x = if is_mask {
                    shown.as_str()
                } else {
                    &core.display_text()
                };
                let disp_idx = core.logic_to_display_index(core.cursor());
                let cx = PAD_X + cursor_x_from_display(text_for_x, opts.font_size, disp_idx);
                scene.fill_rect(Rect::new(cx, top, 1.0, opts.font_size), opts.text_color);
            }

            // IME 组合带下划线
            if let Some((cs, ce)) = core.composing_display_range() {
                let text_for_x = if is_mask {
                    shown.as_str()
                } else {
                    &core.display_text()
                };
                let ax = PAD_X + cursor_x_from_display(text_for_x, opts.font_size, cs);
                let bx = PAD_X + cursor_x_from_display(text_for_x, opts.font_size, ce);
                let y = top + opts.font_size + 1.0;
                scene.fill_rect(Rect::new(ax, y, bx - ax, 1.0), opts.text_color);
            }
        })
    };

    let hit_test_fn = {
        let opts = Rc::clone(&opts);
        Rc::new(move |x: f32, y: f32| {
            (0.0..opts.width).contains(&x) && (0.0..opts.height).contains(&y)
        })
    };

    // --- 点击：聚焦 + 定位光标 ---
    let on_down_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut PointerEvent| {
            shared.borrow_mut().focused = true;
            let mut core = core.borrow_mut();
            let display_text = core.display_text();
            let display_idx = cursor_pos_from_x(&display_text, opts.font_size, event.x - PAD_X);
            let logic_idx = core.display_to_logic_index(display_idx);
            core.set_cursor(logic_idx);
        })
    };

    // --- 键盘导航/编辑 ---
    let key_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        Rc::new(move |event: &mut KeyEvent| -> bool {
            let mut core = core.borrow_mut();
            let handled = editable_handle_key(&mut core, event);
            if handled {
                let mut s = shared.borrow_mut();
                push_change(&core, &mut s.last_synced, &*value, &*on_change);
            }
            handled
        })
    };

    // --- IME 组合态 ---
    let ime_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        Rc::new(move |event: &mut ImeEvent| -> bool {
            let mut core = core.borrow_mut();
            let handled = editable_handle_ime(&mut core, event);
            if handled {
                let mut s = shared.borrow_mut();
                push_change(&core, &mut s.last_synced, &*value, &*on_change);
            }
            handled
        })
    };

    let node = Node {
        draw_fn,
        hit_test_fn,
        on_down_fn: Some(on_down_fn),
        key_fn: Some(key_fn),
        ime_fn: Some(ime_fn),
        focusable: true,
        width: opts.width,
        height: opts.height,
        ..Node::default()
    };

    (node, core)
}

/// 编辑后把快照写回外部（仅在文本变化时调用 `on_change`）。
fn push_change(
    core: &EditCore,
    last_synced: &mut String,
    value: &dyn Fn() -> String,
    on_change: &dyn Fn(String),
) {
    let text = core.text().to_string();
    *last_synced = text.clone();
    if text != value() {
        on_change(text);
    }
}

/// 键盘事件 → EditCore 操作；返回是否消费。
///
/// 多行行为：`Enter` 插入 `\n`（`single_line = false`）。
/// `Ctrl+Z/Y`（撤销/重做）由 EditCore 内置支持。
fn editable_handle_key(core: &mut EditCore, event: &KeyEvent) -> bool {
    let ctrl_or_meta = event.ctrl || event.meta;
    let shift = event.shift;

    if ctrl_or_meta {
        return handle_ctrl_shortcut(core, event);
    }

    match event.key {
        Key::Char(ch) => {
            core.insert_text(&ch.to_string());
            true
        }
        Key::Enter => {
            core.insert_text("\n");
            true
        }
        Key::Backspace => {
            if ctrl_or_meta {
                core.delete_word_backward();
            } else {
                core.backspace();
            }
            true
        }
        Key::Delete => {
            if ctrl_or_meta {
                core.delete_word_forward();
            } else {
                core.delete();
            }
            true
        }
        Key::ArrowLeft => {
            if ctrl_or_meta {
                if shift {
                    core.select_prev_word();
                } else {
                    core.move_prev_word();
                }
            } else if shift {
                core.select_left();
            } else {
                core.move_left();
            }
            true
        }
        Key::ArrowRight => {
            if ctrl_or_meta {
                if shift {
                    core.select_next_word();
                } else {
                    core.move_next_word();
                }
            } else if shift {
                core.select_right();
            } else {
                core.move_right();
            }
            true
        }
        Key::Home => {
            if shift {
                core.select_home(None);
            } else {
                core.move_home(None);
            }
            true
        }
        Key::End => {
            if shift {
                core.select_end(None);
            } else {
                core.move_end(None);
            }
            true
        }
        _ => false,
    }
}

/// Ctrl 快捷键：Z 撤销 / Y 重做 / A 全选。
fn handle_ctrl_shortcut(core: &mut EditCore, event: &KeyEvent) -> bool {
    match event.key {
        Key::Char('z') if !event.alt => {
            core.undo();
            true
        }
        Key::Char('y') | Key::Char('z') if event.alt => {
            core.redo();
            true
        }
        Key::Char('a') => {
            core.select_all();
            true
        }
        _ => false,
    }
}

/// IME 组合事件 → EditCore。
fn editable_handle_ime(core: &mut EditCore, event: &ImeEvent) -> bool {
    match event {
        ImeEvent::Enabled => true,
        ImeEvent::Disabled => {
            core.end_composition(false);
            true
        }
        ImeEvent::Preedit { text, cursor } => {
            if text.is_empty() {
                core.on_composing("", 0); // 空 preedit = 取消组合，还原原文
            } else {
                let char_cursor = cursor.map(|(start, _)| start);
                core.on_composing(text, char_cursor.unwrap_or(0));
            }
            true
        }
        ImeEvent::Commit(_text) => {
            core.end_composition(false);
            true
        }
    }
}

/// 从显示文本的显示索引（字符数）计算 X 坐标。
fn cursor_x_from_display(display_text: &str, font_size: f32, display_idx: usize) -> f32 {
    let prefix: String = display_text.chars().take(display_idx).collect();
    wy_render::text_measure::measure_text(&prefix, font_size).0
}

/// 从本地 X 坐标映射到显示索引（点击定位）。
fn cursor_pos_from_x(display_text: &str, font_size: f32, x: f32) -> usize {
    wy_render::text_measure::index_at_x(display_text, font_size, x.max(0.0))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn core(text: &str) -> EditCore {
        let mut c = EditCore::with_text(text);
        if !text.is_empty() {
            c.set_cursor(text.chars().count());
        }
        c
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
        let mut c = core("hi");
        c.set_cursor(1);
        let ev = key(Key::Char('x'), false, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "hxi");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn backspace_deletes_grapheme() {
        let mut c = core("a😀b");
        c.set_cursor(2);
        let ev = key(Key::Backspace, false, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "ab");
    }

    #[test]
    fn left_arrow_moves_and_collapses() {
        let mut c = core("hello");
        c.select_range(2, 5);
        let ev = key(Key::ArrowLeft, false, false);
        assert!(editable_handle_key(&mut c, &ev));
        // 有选区时按左箭头：塌缩到 sel_start (2)
        assert_eq!(c.cursor(), 2);
        assert!(!c.has_selection());
        let ev = key(Key::ArrowLeft, false, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.cursor(), 1);
    }

    #[test]
    fn shift_arrow_extends_selection() {
        let mut c = core("hello");
        c.set_cursor(0); // 从文档首开始
        let ev = key(Key::ArrowRight, false, true);
        assert!(editable_handle_key(&mut c, &ev));
        assert!(c.has_selection());
        assert_eq!(c.sel_start(), 0);
        assert_eq!(c.sel_end(), 1);
    }

    #[test]
    fn home_end_navigation() {
        let mut c = core("hello");
        c.set_cursor(3);
        let ev = key(Key::Home, false, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.cursor(), 0);
        let ev = key(Key::End, false, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.cursor(), 5);
    }

    #[test]
    fn ctrl_z_undo() {
        let mut c = core("");
        c.insert_text("hi");
        assert_eq!(c.text(), "hi");
        let ev = key(Key::Char('z'), true, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "");
    }

    #[test]
    fn ctrl_a_select_all() {
        let mut c = core("hello");
        let ev = key(Key::Char('a'), true, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.sel_start(), 0);
        assert_eq!(c.sel_end(), 5);
    }

    #[test]
    fn enter_inserts_newline() {
        let mut c = core("ab");
        c.set_cursor(1);
        let ev = key(Key::Enter, false, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "a\nb");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn tab_escape_not_consumed() {
        let mut c = core("hi");
        for k in [Key::Tab, Key::Escape, Key::PageUp, Key::PageDown] {
            let ev = key(k, false, false);
            assert!(
                !editable_handle_key(&mut c, &ev),
                "{k:?} should not consume"
            );
        }
    }

    #[test]
    fn ime_preedit_and_commit() {
        let mut c = core("a");
        c.set_cursor(1);
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: Some((3, 3)),
        };
        assert!(editable_handle_ime(&mut c, &ev));
        assert!(c.in_composing());
        assert_eq!(c.text(), "a中");
        let ev = ImeEvent::Commit("中".into());
        assert!(editable_handle_ime(&mut c, &ev));
        assert!(!c.in_composing());
        assert_eq!(c.text(), "a中");
    }

    #[test]
    fn ime_clear_removes_preedit() {
        let mut c = core("a");
        c.set_cursor(1);
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut c, &ev));
        assert!(c.in_composing());
        let ev = ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut c, &ev));
        assert!(!c.in_composing());
        assert_eq!(c.text(), "a");
    }

    #[test]
    fn push_change_writes_only_on_diff() {
        let cell = Rc::new(RefCell::new(String::from("hi")));
        let mut c = core("hi");
        let cell_for_value = Rc::clone(&cell);
        let value = move || cell_for_value.borrow().clone();
        let count = Rc::new(std::cell::Cell::new(0));
        let count_for_cb = Rc::clone(&count);
        let cell_for_cb = Rc::clone(&cell);
        let on_change: Rc<dyn Fn(String)> = Rc::new(move |t: String| {
            count_for_cb.set(count_for_cb.get() + 1);
            *cell_for_cb.borrow_mut() = t;
        });
        let mut last_synced = "hi".to_string();
        // 文本一致：不触发 on_change
        push_change(&c, &mut last_synced, &value, &*on_change);
        assert_eq!(count.get(), 0);
        // 编辑后触发
        c.set_cursor(2);
        c.insert_text("!");
        push_change(&c, &mut last_synced, &value, &*on_change);
        assert_eq!(count.get(), 1);
        assert_eq!(cell.borrow().as_str(), "hi!");
    }

    #[test]
    fn mask_preserves_char_count() {
        assert_eq!(mask("héllo"), "•••••");
        assert_eq!(mask(""), "");
        assert_eq!(mask("a😀b"), "•••");
    }

    #[test]
    fn style_range_through_core() {
        let mut c = core("hello");
        c.set_cursor(5);
        let bold = wy_text::TextStyle::normal().with_font_weight(700);
        c.buffer_mut().style_range(0, 5, Some(bold));
        assert_eq!(c.buffer().style_at(2).unwrap().font_weight, 700);
        // Kotlin coerceIn(0, len-1)：style_at(5) → index 4，仍是粗体
        assert_eq!(c.buffer().style_at(5).unwrap().font_weight, 700);
    }

    #[test]
    fn richeditable_exposes_shared_core() {
        let doc = Rc::new(RefCell::new(String::from("hello")));
        let (node, core) = rich_editable_opts(
            {
                let doc = Rc::clone(&doc);
                move || doc.borrow().clone()
            },
            {
                let doc = Rc::clone(&doc);
                move |t| *doc.borrow_mut() = t
            },
            RichTextOpts::default(),
        );
        // 组件外直接操作样式段
        core.borrow_mut().buffer_mut().style_range(
            0,
            5,
            Some(wy_text::TextStyle::normal().with_color(0xFFFF0000)),
        );
        assert_eq!(
            core.borrow().buffer().style_at(0).unwrap().color,
            0xFFFF0000
        );
        assert!(node.focusable);
        assert!(node.ime_fn.is_some());
        assert!(node.on_down_fn.is_some());
    }
}
