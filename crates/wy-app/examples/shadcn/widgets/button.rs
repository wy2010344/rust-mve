//! shadcn 按钮：四变体 + hover / pressed 反馈（Signal 驱动重绘）。

use std::rc::Rc;

use wy_mve::{flex, node2, rec, AlignItem, DirectionJustify, Fc, Icon, Layout, Node2, Scene};
use wy_render::{Color, Rect};
use wy_signal::{GetValue, SetValue, Signal};

use crate::tokens::Theme;
use crate::widgets::icon::draw_icon_at;

/// 按钮变体。
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Btn {
    /// 主按钮（实心）。
    #[default]
    Primary,
    /// 次要按钮（浅底 + 边框）。
    Secondary,
    /// 危险操作（红）。
    Destructive,
    /// 幽灵按钮（默认透明，仅 hover / pressed 时出底）。
    Ghost,
}

/// 颜色缩放（hover / pressed 反馈）。
fn shade(c: Color, f: f32) -> Color {
    let s = |v: u8| ((v as f32) * f) as u8;
    Color::rgba(s(c.red()), s(c.green()), s(c.blue()), c.alpha())
}

/// 默认尺寸按钮。
#[rec]
pub fn button(
    theme: Theme,
    #[getter]
    #[props(required)]
    label: String,
    #[props(default)] variant: Btn,
    #[props(default)] icon: Option<Icon>,
    #[props(required)] on_click: impl Fn() + 'static,
) -> Fc {
    button_node(theme, label, variant, 96.0, 36.0, icon, on_click)
}

/// 指定尺寸的按钮（如列表内窄按钮）。
#[rec]
pub fn button_sized(
    theme: Theme,
    #[getter]
    #[props(required)]
    label: String,
    #[props(default)] variant: Btn,
    w: f32,
    h: f32,
    #[props(default)] icon: Option<Icon>,
    #[props(required)] on_click: impl Fn() + 'static,
) -> Fc {
    button_node(theme, label, variant, w, h, icon, on_click)
}

/// `button` / `button_sized` 共用实现。参数已展开成存储形态（`Rc<dyn Fn>`）。
///
/// 结构与 Kotlin `ButtonBase` 一致：**按钮是容器，文案是子节点**。
/// 背景 / 边框在 `draw` 里画，内容（文本或图标）交给 flex 排：
/// `direction: Row` + 主轴居中 + 交叉轴居中 + `align_fix: true`
/// （`alignFix` 让"居中"以按钮自身高度为基准，而不是最高子节点）。
fn button_node(
    theme: Theme,
    label: Rc<dyn Fn() -> String>,
    variant: Btn,
    w: f32,
    h: f32,
    icon: Option<Icon>,
    on_click: Rc<dyn Fn()>,
) -> Fc {
    let pressed = Signal::new(false);
    let hover = Signal::new(false);

    // 内容：有图标就放图标节点，否则放文本节点。
    // 位置全由 flex 算，不再手算 `(w - lw) / 2` / `(h - lh) / 2`。
    let content: Vec<Fc> = match icon {
        Some(ic) => {
            let side = (h - 8.0).clamp(10.0, 16.0);
            vec![node2! {
                width: side,
                height: side,
                draw: move |s: &mut Scene, _n: &Node2| {
                    let t = theme.tokens();
                    let fg = match variant {
                        Btn::Primary => t.primary_fg,
                        Btn::Secondary | Btn::Ghost => t.foreground,
                        Btn::Destructive => t.destructive_fg,
                    };
                    draw_icon_at(s, ic, 0.0, 0.0, side, fg);
                },
            }]
        }
        None => vec![text_label! {
            content: label(),
            font_size: 14.0,
            color: match variant {
                Btn::Primary => theme.tokens().primary_fg,
                Btn::Secondary | Btn::Ghost => theme.tokens().foreground,
                Btn::Destructive => theme.tokens().destructive_fg,
            },
        }],
    };

    node2! {
        width: w,
        height: h,
        focusable: true,
        mouse_click: move |_e| on_click(),
        mouse_down: move |_e| pressed.set(true),
        mouse_up: move |_e| pressed.set(false),
        on_hover: move |on: bool| hover.set(on),
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            direction_justify: DirectionJustify::Center,
            align_fix: true,
        },
        draw: move |s: &mut Scene, _n: &Node2| {
            let t = theme.tokens();
            let (base_bg, _fg) = match variant {
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
        },
        children: content,
    }
}
