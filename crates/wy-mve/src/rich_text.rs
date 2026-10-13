//! `RichEditableTextNode`：多行富文本编辑组件（复刻 Kotlin `RichEditableTextNode`）。
//!
//! 薄壳设计：编辑内核（光标 / 选区 / IME / 撤销 / 导航 / 样式段）全部由
//! [`wy_text::EditCore`] 承担，本组件只负责：
//! - **绘制**：背景 / 边框（聚焦高亮）/ 多色文本片段 / 选区高亮 / 光标 / IME 下划线 / 占位符
//! - **事件转发**：点击定位、键盘导航与编辑、IME 组合态
//! - **值同步**：编辑后写回外部 `on_change`；外部 `value` 变化时重灌编辑器
//!
//! 与 Kotlin 相同，编辑内核对外暴露：`rich_editable_opts` 返回共享 `Rc<RefCell<EditCore>>`，
//! 可在组件外直接调用 `core.borrow_mut().style_range(start, end, style)` 设置样式
//! （样式、光标、选区、组合态变化都会触发局部重绘）。

use std::cell::RefCell;
use std::rc::Rc;

use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};
use wy_text::EditCore;

use crate::node::{ImeEvent, Key, KeyEvent, Node, PointerEvent};
use crate::text_util::{byte_to_char as _byte_to_char, char_to_byte as _char_to_byte};

/// display char 索引 → 字节索引（`caret_box` 等 parley 几何函数按字节寻址）。
fn char_to_byte(text: &str, char_idx: usize) -> usize {
    _char_to_byte(text, char_idx)
}

/// 字节索引 → display char 索引（`hit_point`/`hit_line` 返回字节）。
fn byte_to_char(text: &str, byte_idx: usize) -> usize {
    _byte_to_char(text, byte_idx)
}

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
    /// 是否聚焦（信号承载：绘制依赖它，点击聚焦才会触发重绘）。
    focused: Signal<bool>,
    /// 上次已同步的外部文本（防回环与重复重灌）。
    last_synced: String,
    /// 多击计数（400ms / 5px 内同位置连击，Kotlin ClickTracker）。
    clicks: crate::text_util::ClickTracker,
    /// 双击拖选锚词（display 字节区间）：双击建立，on_move 词级扩展，on_up 清空。
    word_anchor: Option<(usize, usize)>,
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
/// core.borrow_mut().style_range(0, 5, Some(bold_style));
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
        focused: Signal::new(false),
        last_synced: initial.clone(),
        clicks: crate::text_util::ClickTracker::new(),
        word_anchor: None,
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
                c.replace_text(ext.clone());
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
            let focused = shared.borrow().focused.get();
            let core = core.borrow();
            // 追踪缓冲内容变化（文本/样式段直接写 buffer 时通过 revision 感知）
            let _revision = core.revision();
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

            // 选区高亮（逐行绘制，字形行盒几何对齐 Kotlin TIGHT rect）
            if core.has_selection() && focused {
                let sel_s = core.logic_to_display_index(core.sel_start());
                let sel_e = core.logic_to_display_index(core.sel_end());
                let text_for_x = if is_mask {
                    shown.as_str()
                } else {
                    &core.display_text()
                };
                for &(ls, le) in &display_line_ranges(text_for_x) {
                    let vis_s = sel_s.max(ls);
                    let vis_e = sel_e.min(le);
                    if vis_s >= vis_e {
                        continue;
                    }
                    let sb = wy_render::text_measure::caret_box(
                        text_for_x,
                        opts.font_size,
                        char_to_byte(text_for_x, vis_s),
                    );
                    let eb = wy_render::text_measure::caret_box(
                        text_for_x,
                        opts.font_size,
                        char_to_byte(text_for_x, vis_e),
                    );
                    scene.fill_rect(
                        Rect::new(PAD_X + sb.x, top + sb.top, eb.x - sb.x, sb.height),
                        Color::rgba(100, 100, 200, 60),
                    );
                }
            }

            // 光标（聚焦 + 无选区 + 未组合时显示，对齐 Kotlin !hasSel）
            if focused && !core.has_selection() && !core.in_composing() {
                let text_for_x = if is_mask {
                    shown.as_str()
                } else {
                    &core.display_text()
                };
                let disp_idx = core.logic_to_display_index(core.cursor());
                let cb = wy_render::text_measure::caret_box(
                    text_for_x,
                    opts.font_size,
                    char_to_byte(text_for_x, disp_idx),
                );
                scene.fill_rect(
                    Rect::new(PAD_X + cb.x, top + cb.top, 2.0, cb.height),
                    opts.text_color,
                );
            }

            // IME 组合带：背景 + 底部 2px 下划线（对齐 Kotlin drawComposing）
            if let Some((cs, ce)) = core.composing_display_range() {
                let text_for_x = if is_mask {
                    shown.as_str()
                } else {
                    &core.display_text()
                };
                let sb = wy_render::text_measure::caret_box(
                    text_for_x,
                    opts.font_size,
                    char_to_byte(text_for_x, cs),
                );
                let eb = wy_render::text_measure::caret_box(
                    text_for_x,
                    opts.font_size,
                    char_to_byte(text_for_x, ce),
                );
                let ax = PAD_X + sb.x;
                let bx = PAD_X + eb.x;
                let by = top + sb.top;
                let bh = sb.height;
                scene.fill_rect(
                    Rect::new(ax, by, (bx - ax).max(1.0), bh),
                    Color::rgba(200, 200, 255, 50),
                );
                scene.fill_rect(
                    Rect::new(ax, (by + bh - 2.0).max(by), (bx - ax).max(1.0), 2.0),
                    Color::rgba(0, 0, 0, 140),
                );
            }
        })
    };

    let hit_test_fn = {
        let opts = Rc::clone(&opts);
        Rc::new(move |x: f32, y: f32| {
            (0.0..opts.width).contains(&x) && (0.0..opts.height).contains(&y)
        })
    };

    // --- 点击：聚焦 + 多击手势（单击定位 / 双击选词+拖词 / 三击选段） ---
    let on_down_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut PointerEvent| {
            let mut s = shared.borrow_mut();
            s.focused.set(true);
            let mut core = core.borrow_mut();
            let display_text = core.display_text();
            let byte_idx = wy_render::text_measure::hit_point(
                &display_text,
                opts.font_size,
                event.x - PAD_X,
                event.y - text_top(&opts),
            );
            let count = s.clicks.record(event.x, event.y);
            match count {
                // 三击（及以后）选段：display 段不含 `\n`（Kotlin paragraphRangeAt）
                3.. => {
                    s.word_anchor = None;
                    let disp_idx = byte_to_char(&display_text, byte_idx);
                    let (ls, le) = display_line_ranges(&display_text)
                        .into_iter()
                        .find(|&(ls, le)| disp_idx >= ls && disp_idx <= le)
                        .unwrap_or((0, 0));
                    if ls < le {
                        let l0 = core.display_to_logic_index(ls);
                        let l1 = core.display_to_logic_index(le);
                        core.select_range(l0, l1);
                    }
                }
                // 双击选词，锚定后可拖拽词级扩展
                2 => {
                    let (bs, be) = crate::text_util::word_range_at(&display_text, byte_idx)
                        .unwrap_or((0, display_text.len()));
                    s.word_anchor = Some((bs, be));
                    let l0 = core.display_to_logic_index(byte_to_char(&display_text, bs));
                    let l1 = core.display_to_logic_index(byte_to_char(&display_text, be));
                    core.select_range(l0, l1);
                }
                // 单击定位（shift 扩选）
                _ => {
                    s.word_anchor = None;
                    let logic_idx =
                        core.display_to_logic_index(byte_to_char(&display_text, byte_idx));
                    if event.shift {
                        core.move_to_position(logic_idx, true);
                    } else {
                        core.set_cursor(logic_idx);
                    }
                }
            }
        })
    };

    // --- 拖选：双击锚词级扩展，否则光标扩展（选区跟随指针） ---
    let on_move_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut PointerEvent| {
            let s = shared.borrow_mut();
            let mut core = core.borrow_mut();
            let display_text = core.display_text();
            let byte_idx = wy_render::text_measure::hit_point(
                &display_text,
                opts.font_size,
                event.x - PAD_X,
                event.y - text_top(&opts),
            );
            match s.word_anchor {
                Some((a0, a1)) => {
                    // 锚词固定，目标端吸附到目标处整词边界（Kotlin expandWordSelection）
                    let (b0, b1) = crate::text_util::expand_word_selection(a0, a1, byte_idx, |b| {
                        crate::text_util::word_range_at(&display_text, b)
                    });
                    let l0 = core.display_to_logic_index(byte_to_char(&display_text, b0));
                    let l1 = core.display_to_logic_index(byte_to_char(&display_text, b1));
                    core.select_range(l0, l1);
                }
                None => {
                    let logic_idx =
                        core.display_to_logic_index(byte_to_char(&display_text, byte_idx));
                    core.move_to_position(logic_idx, true);
                }
            }
        })
    };

    // --- 释放：结束双击拖词会话 ---
    let on_up_fn = {
        let shared = Rc::clone(&shared);
        Rc::new(move |_event: &mut PointerEvent| {
            shared.borrow_mut().word_anchor = None;
        })
    };

    // --- 键盘导航/编辑 ---
    let key_fn = {
        let core = Rc::clone(&core);
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut KeyEvent| -> bool {
            let mut core = core.borrow_mut();
            let shift = event.shift;

            // 多行导航键需要布局信息，在此处理
            let handled = match event.key {
                Key::ArrowUp => {
                    let display_text = core.display_text();
                    let display_idx = core.logic_to_display_index(core.cursor());
                    let current_line = line_index_for_offset(&display_text, display_idx);

                    if current_line == 0 {
                        core.set_cursor(0);
                        core.set_preferred_x(f32::NAN);
                    } else {
                        let current_x = if !core.preferred_x().is_nan() {
                            core.preferred_x()
                        } else {
                            let x = cursor_x_on_line(&display_text, opts.font_size, display_idx);
                            core.set_preferred_x(x);
                            x
                        };
                        let target_line = current_line - 1;
                        let target_byte = wy_render::text_measure::hit_line(
                            &display_text,
                            opts.font_size,
                            target_line,
                            current_x,
                        );
                        let logic_target =
                            core.display_to_logic_index(byte_to_char(&display_text, target_byte));
                        if shift {
                            core.move_to_position(logic_target, true);
                        } else {
                            core.move_to_position(logic_target, false);
                        }
                    }
                    true
                }
                Key::ArrowDown => {
                    let display_text = core.display_text();
                    let display_idx = core.logic_to_display_index(core.cursor());
                    let line_ranges = display_line_ranges(&display_text);
                    let current_line = line_index_for_offset(&display_text, display_idx);

                    if current_line >= line_ranges.len().saturating_sub(1) {
                        let end = core.text_len();
                        core.set_cursor(end);
                        core.set_preferred_x(f32::NAN);
                    } else {
                        let current_x = if !core.preferred_x().is_nan() {
                            core.preferred_x()
                        } else {
                            let x = cursor_x_on_line(&display_text, opts.font_size, display_idx);
                            core.set_preferred_x(x);
                            x
                        };
                        let target_line = current_line + 1;
                        let target_byte = wy_render::text_measure::hit_line(
                            &display_text,
                            opts.font_size,
                            target_line,
                            current_x,
                        );
                        let logic_target =
                            core.display_to_logic_index(byte_to_char(&display_text, target_byte));
                        if shift {
                            core.move_to_position(logic_target, true);
                        } else {
                            core.move_to_position(logic_target, false);
                        }
                    }
                    true
                }
                Key::Home => {
                    let display_text = core.display_text();
                    let display_idx = core.logic_to_display_index(core.cursor());
                    let chars: Vec<char> = display_text.chars().collect();
                    let line_start_char = chars[..display_idx.min(chars.len())]
                        .iter()
                        .rposition(|&ch| ch == '\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let logic_target = core.display_to_logic_index(line_start_char);
                    if shift {
                        core.move_to_position(logic_target, true);
                    } else {
                        core.set_cursor(logic_target);
                    }
                    core.set_preferred_x(f32::NAN);
                    true
                }
                Key::End => {
                    let display_text = core.display_text();
                    let display_idx = core.logic_to_display_index(core.cursor());
                    let chars: Vec<char> = display_text.chars().collect();
                    let remaining = &chars[display_idx.min(chars.len())..];
                    let line_end_char = display_idx
                        + remaining
                            .iter()
                            .position(|&ch| ch == '\n')
                            .unwrap_or(remaining.len());
                    let logic_target = core.display_to_logic_index(line_end_char);
                    if shift {
                        core.move_to_position(logic_target, true);
                    } else {
                        core.set_cursor(logic_target);
                    }
                    core.set_preferred_x(f32::NAN);
                    true
                }
                _ => editable_handle_key(&mut core, event),
            };

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

    // --- IME 候选窗定位：返回光标盒（节点局部坐标） ---
    let ime_cursor_fn = {
        let core = Rc::clone(&core);
        let opts = Rc::clone(&opts);
        Rc::new(move || {
            let core = core.borrow();
            let shown = if opts.password && !core.showing_placeholder() {
                mask(core.text())
            } else {
                core.display_text()
            };
            let disp_idx = core.logic_to_display_index(core.cursor());
            let cb = wy_render::text_measure::caret_box(
                &shown,
                opts.font_size,
                char_to_byte(&shown, disp_idx),
            );
            (PAD_X + cb.x, text_top(&opts) + cb.top, 2.0, cb.height)
        })
    };

    let node = Node {
        draw_fn,
        hit_test_fn,
        on_down_fn: Some(on_down_fn),
        on_move_fn: Some(on_move_fn),
        on_up_fn: Some(on_up_fn),
        key_fn: Some(key_fn),
        ime_fn: Some(ime_fn),
        ime_cursor_fn: Some(ime_cursor_fn),
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
/// `Home/End` 此处为文档首尾（单行回退）；多行场景由组件层 key_fn 闭包覆盖为行首尾。
/// `ArrowUp/Down` 由组件层 key_fn 闭包处理（需要行度量），此处不匹配。
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

/// Ctrl 快捷键：Z 撤销 / Y 重做 / Ctrl+Shift+Z 重做 / A 全选 / C 复制 / X 剪切 / V 粘贴。
///
/// 对齐 Kotlin `EditableTextNode.handleKey`（:563-566）与 `Renderer.keyPress`
/// 的剪贴板拦截（:425-447）：
/// - CapsLock/Shift 下平台给大写字母，先按 `lowercaseChar()` 归一；
/// - undo = ctrl && !shift && 'z'；redo = (ctrl && 'y') || (ctrl && shift && 'z')；
/// - paste 空串不插，先终止组合态（`endComposition(restore=false)`）。
fn handle_ctrl_shortcut(core: &mut EditCore, event: &KeyEvent) -> bool {
    let Key::Char(c) = event.key else {
        return false;
    };
    match c.to_ascii_lowercase() {
        'z' if !event.shift => {
            core.undo();
            true
        }
        'y' | 'z' => {
            core.redo();
            true
        }
        'a' => {
            core.select_all();
            true
        }
        'c' => {
            if let Some(sel) = core.selected_text() {
                crate::clipboard::clipboard_set(&sel);
            }
            true
        }
        'x' => {
            if let Some(sel) = core.selected_text() {
                crate::clipboard::clipboard_set(&sel);
                core.delete_selection();
            }
            true
        }
        'v' => {
            if let Some(text) = crate::clipboard::clipboard_get() {
                if !text.is_empty() {
                    core.end_composition(false);
                    core.insert_text(&text);
                }
            }
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
        ImeEvent::Commit(text) => {
            // 对齐 Kotlin 平台层：先移除预编辑窗口，再经常规插入路径写入已确认文本
            core.end_composition(false);
            core.insert_text(text);
            true
        }
    }
}

/// 显示文本按 `\n` 切分为行，返回每行的 (字符起始, 字符结束) 半开区间。
fn display_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (i, ch) in text.chars().enumerate() {
        if ch == '\n' {
            ranges.push((start, i));
            start = i + 1;
        }
    }
    ranges.push((start, text.chars().count()));
    ranges
}

/// 字符偏移所在的行号（0-based）。
fn line_index_for_offset(text: &str, offset: usize) -> usize {
    let mut line = 0;
    for (i, ch) in text.chars().enumerate() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

/// 光标在当前行内的 X 坐标（与绘制同一排版来源，字形簇边界）。
fn cursor_x_on_line(display_text: &str, font_size: f32, display_idx: usize) -> f32 {
    wy_render::text_measure::caret_box(
        display_text,
        font_size,
        char_to_byte(display_text, display_idx),
    )
    .x
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
    fn caps_lock_does_not_break_undo_redo() {
        // 移植 Kotlin EditableShortcutTest：CapsLock/Shift 下平台给大写字母，
        // 快捷键按 lowercaseChar() 归一后仍生效；Ctrl+Shift+Z = redo。
        let mut c = core("");
        c.insert_text("A");
        c.insert_text("B");
        assert_eq!(c.text(), "AB");

        let ev = key(Key::Char('Z'), true, false); // CapsLock → 大写 Z
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "A", "大写 Z 的 Ctrl+Z 应撤销");

        let ev = key(Key::Char('Z'), true, true); // Shift 保持大写
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "AB", "Ctrl+Shift+Z 应重做");

        let ev = key(Key::Char('Z'), true, false);
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "A");

        let ev = key(Key::Char('Y'), true, false); // CapsLock → 大写 Y
        assert!(editable_handle_key(&mut c, &ev));
        assert_eq!(c.text(), "AB", "大写 Y 的 Ctrl+Y 应重做");
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

    /// winit Windows 真实序列：提交时先 `Preedit("")` 清窗，再 `Commit(text)`，
    /// 且 commit 文本可能与 preedit 不同——commit 文本必须写入正文。
    #[test]
    fn ime_winit_windows_commit_sequence() {
        let mut c = core("");
        c.set_cursor(0);
        let ev = ImeEvent::Preedit {
            text: "nihao".into(),
            cursor: Some((5, 5)),
        };
        assert!(editable_handle_ime(&mut c, &ev));
        assert_eq!(c.text(), "nihao");
        // winit：GCS_RESULTSTR → 先清 preedit，再提交
        let ev = ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut c, &ev));
        assert_eq!(c.text(), "");
        let ev = ImeEvent::Commit("你好".into());
        assert!(editable_handle_ime(&mut c, &ev));
        assert_eq!(c.text(), "你好");
        assert!(!c.in_composing());
        assert_eq!(c.cursor(), 2);
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
        c.style_range(0, 5, Some(bold));
        assert_eq!(c.buffer().style_at(2).unwrap().font_weight, 700);
        // Kotlin coerceIn(0, len-1)：style_at(5) → index 4，仍是粗体
        assert_eq!(c.buffer().style_at(5).unwrap().font_weight, 700);
    }

    #[test]
    fn style_range_bumps_revision() {
        let mut c = core("hello");
        let before = c.revision();
        c.style_range(
            0,
            5,
            Some(wy_text::TextStyle::normal().with_color(0xFFFF0000)),
        );
        assert!(
            c.revision() > before,
            "样式写入必须递增 revision 以触发重绘"
        );
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
        core.borrow_mut().style_range(
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

    // -----------------------------------------------------------------------
    // 回归：绘制 memo（RedrawTracker = RecordMemo<Scene>）必须感知编辑器状态
    // -----------------------------------------------------------------------

    /// 在 `RecordMemo<Scene>` 追踪上下文中重录一帧，返回本次是否真正重跑 compute。
    fn record_frame(
        tracker: &wy_signal::RecordMemo<Scene>,
        node: &Node,
        draws: &std::cell::Cell<usize>,
    ) {
        tracker.record(|scene: &mut Scene| {
            draws.set(draws.get() + 1);
            node.run_draw(scene);
        });
    }

    #[test]
    fn cursor_move_retriggers_draw_memo() {
        // 用户报告的核心 bug：方向键只改光标、不改文本 → 不触发 on_change，
        // 旧实现短路复用缓存 Scene，光标不动。信号化后必须重录。
        use std::cell::Cell;
        use wy_signal::RecordMemo;

        let doc = Rc::new(RefCell::new(String::from("hello")));
        let (node, core) = rich_editable_opts(
            {
                let d = Rc::clone(&doc);
                move || d.borrow().clone()
            },
            {
                let d = Rc::clone(&doc);
                move |t| *d.borrow_mut() = t
            },
            RichTextOpts::default(),
        );
        let tracker = RecordMemo::<Scene>::new();
        let draws = Cell::new(0);

        record_frame(&tracker, &node, &draws);
        assert_eq!(draws.get(), 1, "首次录制");
        assert_eq!(core.borrow().cursor(), 0);

        // 右箭头：仅移动光标（anchor/focus 信号变化），文本不变
        let mut ev = KeyEvent {
            key: Key::ArrowRight,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        assert!(node.key_fn.as_ref().unwrap()(&mut ev));
        assert_eq!(core.borrow().cursor(), 1);

        record_frame(&tracker, &node, &draws);
        assert_eq!(draws.get(), 2, "光标移动必须触发绘制 memo 重录");
    }

    #[test]
    fn click_focus_retriggers_draw_memo() {
        // 点击聚焦只改 focused 信号 → 边框/光标应重绘。
        use std::cell::Cell;
        use wy_signal::RecordMemo;

        let doc = Rc::new(RefCell::new(String::from("hi")));
        let (node, _core) = rich_editable_opts(
            {
                let d = Rc::clone(&doc);
                move || d.borrow().clone()
            },
            {
                let d = Rc::clone(&doc);
                move |t| *d.borrow_mut() = t
            },
            RichTextOpts::default(),
        );
        let tracker = RecordMemo::<Scene>::new();
        let draws = Cell::new(0);

        record_frame(&tracker, &node, &draws);
        assert_eq!(draws.get(), 1);

        let mut pe = PointerEvent::new(20.0, 20.0);
        node.on_down_fn.as_ref().unwrap()(&mut pe);

        record_frame(&tracker, &node, &draws);
        assert_eq!(draws.get(), 2, "点击聚焦必须触发绘制 memo 重录");
    }

    #[test]
    fn external_signal_text_change_retriggers_and_rehydrates() {
        // 外部值信号变化 → memo 依赖失效 → 重录时 sync 重灌内核文本。
        // 该路径在 compute 内写信号（revision/anchor），须无 panic。
        use std::cell::Cell;
        use wy_signal::{RecordMemo, Signal};

        let doc = Signal::new(String::from("a"));
        let d1 = doc;
        let d2 = doc;
        let (node, core) = rich_editable_opts(
            move || d1.get(),
            move |t| d2.set(t),
            RichTextOpts::default(),
        );
        let tracker = RecordMemo::<Scene>::new();
        let draws = Cell::new(0);

        record_frame(&tracker, &node, &draws);
        assert_eq!(draws.get(), 1);
        assert_eq!(core.borrow().text(), "a");

        doc.set(String::from("hello"));

        record_frame(&tracker, &node, &draws);
        assert_eq!(draws.get(), 2, "外部信号文本变化必须触发重录");
        assert_eq!(core.borrow().text(), "hello", "sync 重灌内核文本");
    }

    // --- 多击手势：单击定位 / 双击选词 / 三击选段 / 拖选 / shift 扩选 ---

    fn edit_doc(text: &str) -> (Node, Rc<RefCell<EditCore>>) {
        let t = text.to_string();
        rich_editable_opts(move || t.clone(), |_| {}, RichTextOpts::default())
    }

    /// 第 `char_idx` 个字形内的测试点击点（x = 字形左界 + 1px，y = 首行行盒内）。
    fn at(text: &str, char_idx: usize) -> (f32, f32) {
        let opts = RichTextOpts::default();
        let byte = char_to_byte(text, char_idx);
        let cb = wy_render::text_measure::caret_box(text, opts.font_size, byte);
        let cb0 = wy_render::text_measure::caret_box(text, opts.font_size, 0);
        (PAD_X + cb.x + 1.0, text_top(&opts) + cb0.top + 4.0)
    }

    fn down(node: &Node, x: f32, y: f32, shift: bool) {
        let mut ev = PointerEvent::new(x, y);
        ev.shift = shift;
        node.on_down_fn.as_ref().unwrap()(&mut ev);
    }

    fn mv(node: &Node, x: f32, y: f32) {
        let mut ev = PointerEvent::new(x, y);
        node.on_move_fn.as_ref().unwrap()(&mut ev);
    }

    #[test]
    fn double_click_selects_word() {
        let (node, core) = edit_doc("Hello world");
        let (x, y) = at("Hello world", 2);
        down(&node, x, y, false);
        assert_eq!(core.borrow().cursor(), 2, "单击定位到命中字形");
        down(&node, x, y, false);
        let c = core.borrow();
        assert_eq!((c.sel_start(), c.sel_end()), (0, 5), "双击选整词 Hello");
    }

    #[test]
    fn triple_click_selects_paragraph() {
        let text = "hi there\nxy";
        let (node, core) = edit_doc(text);
        let (x, y) = at(text, 4);
        down(&node, x, y, false);
        down(&node, x, y, false);
        down(&node, x, y, false);
        let c = core.borrow();
        assert_eq!(
            (c.sel_start(), c.sel_end()),
            (0, 8),
            "三击选整段（段不含 \\n）"
        );
    }

    #[test]
    fn drag_extends_selection() {
        let text = "Hello world";
        let (node, core) = edit_doc(text);
        let (x1, y1) = at(text, 1);
        let (x2, y2) = at(text, 8);
        down(&node, x1, y1, false);
        assert_eq!(core.borrow().cursor(), 1);
        mv(&node, x2, y2);
        let c = core.borrow();
        assert_eq!((c.sel_start(), c.sel_end()), (1, 8), "拖选扩展选区");
    }

    #[test]
    fn shift_click_extends_selection() {
        let text = "Hello world";
        let (node, core) = edit_doc(text);
        let (x1, y1) = at(text, 1);
        let (x2, y2) = at(text, 8);
        down(&node, x1, y1, false);
        down(&node, x2, y2, true);
        let c = core.borrow();
        assert_eq!((c.sel_start(), c.sel_end()), (1, 8), "shift+点击扩选");
    }

    #[test]
    fn double_click_drag_expands_word() {
        let text = "Hello world";
        let (node, core) = edit_doc(text);
        let (x1, y1) = at(text, 2);
        let (x2, y2) = at(text, 9);
        down(&node, x1, y1, false);
        down(&node, x1, y1, false);
        assert_eq!((core.borrow().sel_start(), core.borrow().sel_end()), (0, 5));
        mv(&node, x2, y2);
        let c = core.borrow();
        assert_eq!(
            (c.sel_start(), c.sel_end()),
            (0, 11),
            "锚词 Hello 固定，目标端吸附到 world"
        );
    }

    #[test]
    fn double_click_cjk_roundtrips_char_byte() {
        // 中文 = 多字节：hit_point/word_range_at 走字节，EditCore 走字符，
        // 双击选词必须正确往返（回归：光标/选区按字节当字符会错位）。
        let text = "中文 测试";
        let (node, core) = edit_doc(text);
        let (x, y) = at(text, 1); // '文' 字形（字节 3）
        down(&node, x, y, false);
        assert_eq!(core.borrow().cursor(), 1, "单击命中 '文' 前字符边界");
        down(&node, x, y, false);
        let c = core.borrow();
        assert_eq!((c.sel_start(), c.sel_end()), (0, 2), "双击选 '中文' 两字符");
    }

    #[test]
    fn ctrl_c_x_v_clipboard_roundtrip() {
        use crate::clipboard::{set_clipboard, Clipboard};

        struct Mem(RefCell<Option<String>>);
        impl Clipboard for Mem {
            fn get_text(&self) -> Option<String> {
                self.0.borrow().clone()
            }
            fn set_text(&self, t: &str) {
                *self.0.borrow_mut() = Some(t.to_string());
            }
        }

        crate::clipboard::reset_for_tests();
        set_clipboard(Mem(RefCell::new(None)));

        let (node, core) = edit_doc("Hello");
        core.borrow_mut().select_range(0, 2);
        let mut ev = key(Key::Char('c'), true, false);
        assert!(node.key_fn.as_ref().unwrap()(&mut ev));
        assert_eq!(
            crate::clipboard::clipboard_get().as_deref(),
            Some("He"),
            "复制写入剪贴板"
        );
        assert_eq!(core.borrow().text(), "Hello", "复制不改文本");

        let mut ev = key(Key::Char('x'), true, false);
        assert!(node.key_fn.as_ref().unwrap()(&mut ev));
        assert_eq!(core.borrow().text(), "llo", "剪切删除选区，光标在 0");
        assert_eq!(crate::clipboard::clipboard_get().as_deref(), Some("He"));

        // 首次粘贴：0 处插入 "He" → "Hello"，光标推进到 2
        let mut ev = key(Key::Char('v'), true, false);
        assert!(node.key_fn.as_ref().unwrap()(&mut ev));
        assert_eq!(core.borrow().text(), "Hello");
        // 二次粘贴：2 处插入 "He" → "HeHello"
        let mut ev = key(Key::Char('v'), true, false);
        assert!(node.key_fn.as_ref().unwrap()(&mut ev));
        assert_eq!(core.borrow().text(), "HeHello", "粘贴两次");

        crate::clipboard::reset_for_tests();
    }
}
