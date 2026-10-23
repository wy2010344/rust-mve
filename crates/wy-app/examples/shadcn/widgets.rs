//! 样式组件：卡片 / 徽章 / 按钮 / 文本。
//!
//! 全部是 `#[component]` 组件：字段有**缺省值**、`children` 为**插槽**、
//! 动态属性走 `#[getter]`（裸值或闭包皆可，绘制期按帧求值，复刻 Kotlin
//! 「属性即表达式」的形态）。样式全部写在 `draw` 里（`draw` 读 `current_box()`
//! 自适应尺寸，`node {}` 既是自绘叶子又是可嵌套容器）；颜色一律经
//! [`Theme::tokens`] 在绘制期求值——节点树只构建一次，主题切换靠 draw 期读
//! 信号触发局部重绘（信号模型，非瀑布重建）。

use std::rc::Rc;

use wy_mve::{component, current_box, rsx, AlignItem, Icon, Node};
use wy_render::{text_measure, Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::tokens::Theme;

/// 卡片内边距。
pub const CARD_PAD: f32 = 16.0;

/// shadcn 卡片：底色 + 边框 + 圆角 8，内含 `children` 插槽（Column 排布，带内边距）。
///
/// 用 `node {}` 自绘容器实现：背景/边框在 `draw` 里画（读 `current_box()` 自适应尺寸），
/// 内容经 `padding` 内缩——既像 Kotlin 匿名类就地自绘、又能天然容纳子节点，
/// 不必再手写两层 `vbox` + 手动 `x`/`y` 偏移。尺寸由调用处 `width`/`height` 布局属性决定。
#[component]
pub fn card(theme: Theme, children: Vec<Slot>) -> Node {
    rsx! {
        node {
            padding: CARD_PAD,
            gap: 0.0,
            align_items: AlignItem::Stretch,
            draw: move |s: &mut Scene| {
                let b = current_box();
                let t = theme.tokens();
                s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 8.0, t.card);
                s.stroke_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 8.0, t.border, 1.0);
            },
            ..children
        }
    }
    .into_node()
}

/// shadcn 徽章：小圆角标签。variant 0=default 1=secondary 2=destructive 3=outline。
///
/// `text` / `variant` 均为 `#[getter]`（绘制期求值），故徽章可随信号
/// 动态改文案与配色，无需重建节点。
#[component]
pub fn badge(
    theme: Theme,
    /// 徽章文案（getter）。
    #[getter]
    text: String,
    /// 变体（getter）：0 default / 1 secondary / 2 destructive / 3 outline。
    #[getter]
    variant: u8,
) -> Node {
    const H: f32 = 20.0;
    // getter 是 `Rc<dyn Fn()>`，要被 `width` 与 `draw` 两个闭包各捕获一份，先克隆。
    let width_text = Rc::clone(&text);
    let draw_text = Rc::clone(&text);
    rsx! {
        node {
            height: H,
            width: wy_render::text_measure::measure_text(&width_text(), 12.0).0 + 16.0,
            draw: move |s: &mut Scene| {
                let t = theme.tokens();
                let text = draw_text();
                let w = wy_render::text_measure::measure_text(&text, 12.0).0 + 16.0;
                let (bg, fg, border) = match variant() {
                    2 => (t.destructive, t.destructive_fg, None),
                    3 => (Color::TRANSPARENT, t.foreground, Some(t.border)),
                    1 => (t.muted, t.muted_fg, None),
                    _ => (t.accent, t.foreground, None),
                };
                s.fill_round_rect(Rect::new(0.0, 0.0, w, H), 4.0, bg);
                if let Some(b) = border {
                    s.stroke_round_rect(Rect::new(0.0, 0.0, w, H), 4.0, b, 1.0);
                }
                s.draw_text(Point::new(8.0, 4.0), &text, 12.0, fg);
            },
        }
    }
    .into_node()
}

/// 按钮变体。
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Btn {
    #[default]
    Primary,
    Secondary,
    Destructive,
    Ghost,
}

/// shadcn 按钮：四变体 + hover/pressed 反馈（Signal 驱动重绘）。
///
/// `label` 为 `#[getter]`（绘制期求值），故文案可随信号变化；`variant` 缺省 `Primary`。
///
/// `icon` 传 `Some(...)` 时按钮显示**矢量图标**而非文字——图标自绘路径，
/// 不依赖字体是否含该字形（用字符当图标在缺字形的机器上会渲染成方块）。
#[component]
pub fn button(
    theme: Theme,
    #[getter] label: String,
    #[props(default)] variant: Btn,
    #[props(default)] icon: Option<Icon>,
    on_click: impl Fn(),
) -> Node {
    button_node(theme, label, variant, 96.0, 36.0, icon, on_click)
}

/// 指定尺寸的按钮（如列表内窄按钮）。
#[component]
pub fn button_sized(
    theme: Theme,
    #[getter] label: String,
    #[props(default)] variant: Btn,
    w: f32,
    h: f32,
    #[props(default)] icon: Option<Icon>,
    on_click: impl Fn(),
) -> Node {
    button_node(theme, label, variant, w, h, icon, on_click)
}

/// `button` / `button_sized` 共用的构建逻辑（均走 `node {}` 自绘 + 事件）。
fn button_node(
    theme: Theme,
    label: Rc<dyn Fn() -> String>,
    variant: Btn,
    w: f32,
    h: f32,
    icon: Option<Icon>,
    on_click: Rc<dyn Fn()>,
) -> Node {
    let pressed = Signal::new(false);
    let hover = Signal::new(false);
    rsx! {
        node {
            width: w, height: h,
            focusable: true,
            on_click: move || on_click(),
            on_hover: move |on| hover.set(on),
            on_down: move |_| pressed.set(true),
            on_up: move |_| pressed.set(false),
            draw: move |s: &mut Scene| {
                let t = theme.tokens();
                let (base_bg, fg) = match variant {
                    Btn::Primary => (t.primary, t.primary_fg),
                    Btn::Secondary => (t.muted, t.foreground),
                    Btn::Destructive => (t.destructive, t.destructive_fg),
                    Btn::Ghost => (Color::TRANSPARENT, t.foreground),
                };
                let mut bg = base_bg;
                if variant == Btn::Ghost && (hover.get() || pressed.get()) {
                    bg = t.accent;
                } else if hover.get() {
                    bg = shade(bg, 0.92);
                }
                if pressed.get() {
                    bg = shade(bg, 0.85);
                }
                s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 6.0, bg);
                if variant == Btn::Secondary {
                    s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 6.0, t.border, 1.0);
                }
                let text = label();
                // 图标优先：有图标时居中画图标，文案置空
                match icon {
                    Some(ic) => {
                        let side = (h - 8.0).clamp(10.0, 16.0);
                        let ox = (w - side) / 2.0;
                        let oy = (h - side) / 2.0;
                        draw_icon_at(s, ic, ox, oy, side, fg);
                    }
                    None => {
                        if !text.is_empty() {
                            let lw = wy_render::text_measure::measure_text(&text, 14.0).0;
                            s.draw_text(Point::new((w - lw) / 2.0, 10.0), &text, 14.0, fg);
                        }
                    }
                }
            },
        }
    }
    .into_node()
}

/// 在指定位置绘制矢量图标（`Icon` 的路径数据自绘，不依赖字体）。
///
/// 与 [`wy_mve::icon`] 组件同一套路径，只是这里直接画进调用方的 Scene，
/// 免得再嵌一层子节点（按钮的 hover/pressed 底色需要与图标同层绘制）。
pub fn draw_icon_at(s: &mut Scene, ic: Icon, ox: f32, oy: f32, side: f32, color: Color) {
    let (paths, filled) = ic.icon_paths();
    for seg in paths {
        let pts: Vec<Point> = seg
            .chunks(2)
            .map(|c| Point::new(ox + c[0] * side, oy + c[1] * side))
            .collect();
        if filled {
            s.fill_path(pts, color);
        } else {
            s.stroke_path(pts, (side * 0.09).max(1.0), color);
        }
    }
}

/// 颜色缩放（hover/pressed 反馈）。
fn shade(c: Color, f: f32) -> Color {
    let s = |v: u8| ((v as f32) * f) as u8;
    Color::rgba(s(c.red()), s(c.green()), s(c.blue()), c.alpha())
}

/// shadcn 文本（自绘，可定字号/颜色）。
///
/// `content` / `color` 均为 `#[getter]`：调用处传裸值或闭包皆可，框架按帧重算——
/// 不必再写 `move || ...`。颜色在绘制期经 `theme.tokens()` 求值，主题切换自动重绘。
#[component]
pub fn text_label(
    /// 文本内容（getter：裸字符串或闭包）。
    #[getter]
    content: String,
    /// 字号。不能叫 `size`：`size` 是通用布局属性（`(w, h)`），会撞车。
    #[props(default = 13.0f32)]
    font_size: f32,
    /// 文本颜色（getter：裸 `Color` 或闭包均可，可捕获 `theme` 随主题重绘）。
    #[getter]
    color: Color,
) -> Node {
    // getter 是 `Rc<dyn Fn()>`，要被 `draw` 与 `measure` 两个闭包各捕获一份，先克隆。
    let draw_content = Rc::clone(&content);
    let measure_content = Rc::clone(&content);
    rsx! {
        node {
            draw: move |s: &mut Scene| {
                let b = current_box();
                s.draw_text(
                    Point::new(0.0, (b.height - font_size) / 2.0),
                    &draw_content(),
                    font_size,
                    color(),
                );
            },
            measure: text_measure::measure_text(&measure_content(), font_size),
        }
    }
    .into_node()
}

// ---------------------------------------------------------------------------
// 以下组件从 ooc 侧（shadcn/ui new-york-v4）复刻：
// separator / switch / checkbox / avatar / skeleton / tabs
// ---------------------------------------------------------------------------

/// shadcn Separator：细分隔线（ooc `separator.ooc`）。
///
/// 横向：高 1、宽度随父容器交叉轴拉伸；纵向：宽 1、高度拉伸。
/// 用 `align_self: Stretch` 保证在任何默认对齐的容器里都撑满交叉轴。
#[component]
pub fn separator(
    theme: Theme,
    /// `true` = 纵向分隔线（竖排容器用）。
    #[props(default)]
    vertical: bool,
) -> Node {
    rsx! {
        node {
            height: if vertical { 0.0 } else { 1.0 },
            width: if vertical { 1.0 } else { 0.0 },
            align_self: AlignItem::Stretch,
            draw: move |s: &mut Scene| {
                let b = current_box();
                s.fill_rect(Rect::new(0.0, 0.0, b.width.max(1.0), b.height.max(1.0)), theme.tokens().border);
            },
        }
    }
    .into_node()
}

/// shadcn Switch：滑块开关（ooc `switch.ooc`；`switch` 是 Rust 关键字，故名 `toggle`）。
///
/// 受控组件：`checked` 为 getter（读信号），`on_change(!checked)` 由组件回调；
/// 业务在 `on_change` 里写信号即可，绘制期读信号自动重绘。
#[component]
pub fn toggle(
    theme: Theme,
    /// 是否打开（getter：一般 `move || signal.get()`）。
    #[getter]
    checked: bool,
    /// 状态变化回调（参数为新状态）。
    on_change: impl Fn(bool) + 'static,
) -> Node {
    const W: f32 = 36.0;
    const H: f32 = 20.0;
    // getter 是 `Rc<dyn Fn()>`：on_click 与 draw 各捕获一份，先克隆
    let click_checked = Rc::clone(&checked);
    let draw_checked = Rc::clone(&checked);
    rsx! {
        node {
            width: W, height: H,
            on_click: move || on_change(!click_checked()),
            draw: move |s: &mut Scene| {
                let t = theme.tokens();
                let on = draw_checked();
                s.fill_round_rect(Rect::new(0.0, 0.0, W, H), H / 2.0, if on { t.primary } else { t.input_bg });
                if !on {
                    s.stroke_round_rect(Rect::new(0.5, 0.5, W - 1.0, H - 1.0), H / 2.0, t.border, 1.0);
                }
                // 滑块：开 = 靠右，关 = 靠左（ooc thumbOn/thumbOff）
                let thumb_x = if on { W - H + 2.0 } else { 2.0 };
                s.fill_round_rect(Rect::new(thumb_x, 2.0, H - 4.0, H - 4.0), (H - 4.0) / 2.0, t.card);
            },
        }
    }
    .into_node()
}

/// shadcn Checkbox：勾选框（ooc `checkbox.ooc`）。
///
/// 受控组件同 [`toggle`]：`checked` getter + `on_change(!checked)` 回调。
#[component]
pub fn checkbox(
    theme: Theme,
    /// 是否勾选（getter）。
    #[getter]
    checked: bool,
    /// 状态变化回调（参数为新状态）。
    on_change: impl Fn(bool) + 'static,
) -> Node {
    const S: f32 = 18.0;
    // getter 是 `Rc<dyn Fn()>`：on_click 与 draw 各捕获一份，先克隆
    let click_checked = Rc::clone(&checked);
    let draw_checked = Rc::clone(&checked);
    rsx! {
        node {
            width: S, height: S,
            on_click: move || on_change(!click_checked()),
            draw: move |s: &mut Scene| {
                let t = theme.tokens();
                let r = Rect::new(0.5, 0.5, S - 1.0, S - 1.0);
                if draw_checked() {
                    s.fill_round_rect(r, 4.0, t.primary);
                    // 勾用**矢量路径**自绘（不用 "✓" 字符）：
                    // 字符依赖字体含该字形，缺字形的机器上会渲染成空心方块。
                    draw_icon_at(s, Icon::Check, 3.0, 3.0, S - 6.0, t.primary_fg);
                } else {
                    s.fill_round_rect(r, 4.0, t.input_bg);
                    s.stroke_round_rect(r, 4.0, t.border, 1.0);
                }
            },
        }
    }
    .into_node()
}

/// shadcn Avatar：圆形头像（ooc `avatar.ooc` 的 AvatarFallback 形态——
/// 无图片资源，直接渲染首字母/图标占位）。
#[component]
pub fn avatar(
    theme: Theme,
    /// 头像文本（首字母、缩写等；getter）。
    #[getter]
    label: String,
    /// 直径。
    #[props(default = 32.0f32)]
    size: f32,
) -> Node {
    rsx! {
        node {
            width: size, height: size,
            draw: move |s: &mut Scene| {
                let t = theme.tokens();
                s.fill_round_rect(Rect::new(0.0, 0.0, size, size), size / 2.0, t.muted);
                let text = label();
                let lw = text_measure::measure_text(&text, 13.0).0;
                s.draw_text(Point::new((size - lw) / 2.0, (size - 13.0) / 2.0), &text, 13.0, t.muted_fg);
            },
        }
    }
    .into_node()
}

/// shadcn Skeleton：加载占位灰块（ooc `skeleton.ooc`）。
///
/// 宽度随父容器交叉轴拉伸（`w: 0.0`），高度由 `h` 给定；
/// 作为空态/加载态的占位行使用。
#[component]
pub fn skeleton(
    theme: Theme,
    /// 占位块高度。
    #[props(default = 12.0f32)]
    h: f32,
) -> Node {
    rsx! {
        node {
            height: h,
            draw: move |s: &mut Scene| {
                let b = current_box();
                s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 4.0, theme.tokens().accent);
            },
        }
    }
    .into_node()
}

/// shadcn Tabs：分段选择器（ooc `tabs.ooc` 的 TabsList+TabsTrigger 形态）。
///
/// 受控：`active` 读信号高亮当前段，点击写回信号；段宽按文本实测。
#[component]
pub fn tabs(
    theme: Theme,
    /// 段文案列表。
    items: Vec<String>,
    /// 当前选中段下标（Signal 共享：绘制期读、点击写）。
    active: Signal<usize>,
) -> Node {
    const ITEM_H: f32 = 26.0;
    const PAD: f32 = 12.0;
    let mut slots: Vec<wy_mve::Slot> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        // 每个闭包各捕获一份文案
        let draw_item = item.clone();
        let measure_item = item.clone();
        slots.push(
            rsx! {
                node {
                    height: ITEM_H,
                    width: text_measure::measure_text(&measure_item, 12.0).0 + PAD * 2.0,
                    on_click: move || active.set(i),
                    draw: move |s: &mut Scene| {
                        let t = theme.tokens();
                        let on = active.get() == i;
                        let r = Rect::new(0.0, 0.0, text_measure::measure_text(&draw_item, 12.0).0 + PAD * 2.0, ITEM_H);
                        if on {
                            s.fill_round_rect(r, 6.0, t.card);
                            s.stroke_round_rect(r, 6.0, t.border, 1.0);
                        }
                        s.draw_text(Point::new(PAD, (ITEM_H - 12.0) / 2.0), &draw_item, 12.0, if on { t.foreground } else { t.muted_fg });
                    },
                }
            }
            .into_node()
            .into(),
        );
    }
    rsx! {
        hbox {
            gap: 2.0,
            padding: 3.0,
            ..slots
        }
    }
    .into_node()
}
