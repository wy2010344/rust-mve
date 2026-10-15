//! 样式组件：卡片 / 徽章 / 按钮 / 文本。
//!
//! `Node` 无样式字段，样式全部写在 `draw_fn` 里。
//! 颜色一律经 [`Theme::tokens`] 在**绘制期**求值——节点树只构建一次，
//! 主题切换靠 draw 期读信号触发局部重绘（信号模型，非瀑布重建）。

use std::rc::Rc;

use wy_mve::Node;
use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::tokens::{Theme, Tokens};

/// 卡片内边距。
pub const CARD_PAD: f32 = 16.0;

/// shadcn 卡片：底色 + 边框 + 圆角 8，内含子节点（Column 排布，带内边距）。
///
/// 固定尺寸走 `size_fn`——布局容器默认按子节点累加算尺寸、会忽略 `width/height`，
/// 那样兄弟节点定位会错乱。内边距用一层偏移容器实现（内容整体下移/右移 [`CARD_PAD`]）。
pub fn card(theme: Theme, w: f32, h: f32, children: Vec<Node>) -> Node {
    let mut inner = wy_mve::vbox(children);
    inner.gap = 0.0;
    inner.x = CARD_PAD;
    inner.y = CARD_PAD;

    let mut n = wy_mve::vbox(vec![inner]);
    n.gap = 0.0;
    n.size_fn = Some(Rc::new(move || (w, h)));
    n.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            let t = theme.tokens();
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 8.0, t.card);
            s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 8.0, t.border, 1.0);
        }
    });
    n
}

/// shadcn 徽章：小圆角标签。variant 0=default 1=secondary 2=destructive 3=outline。
pub fn badge(theme: Theme, text: &str, variant: u8) -> Node {
    let text = text.to_string();
    let w = wy_render::text_measure::measure_text(&text, 12.0).0 + 16.0;
    let h = 20.0;
    let mut n = wy_mve::text({
        let text = text.clone();
        move || text.clone()
    });
    n.width = w;
    n.height = h;
    n.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            let t = theme.tokens();
            let (bg, fg, border) = match variant {
                2 => (t.destructive, t.destructive_fg, None),
                3 => (Color::TRANSPARENT, t.foreground, Some(t.border)),
                1 => (t.muted, t.muted_fg, None),
                _ => (t.accent, t.foreground, None),
            };
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 4.0, bg);
            if let Some(b) = border {
                s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 4.0, b, 1.0);
            }
            s.draw_text(Point::new(8.0, 4.0), &text, 12.0, fg);
        }
    });
    n
}

/// 按钮变体。
#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Primary,
    Secondary,
    Destructive,
    Ghost,
}

/// shadcn 按钮：四变体 + hover/pressed 反馈（Signal 驱动重绘）。
///
/// `label` 为闭包（绘制期求值），故文案可随信号变化（如主题切换按钮）。
pub fn button(
    theme: Theme,
    label: impl Fn() -> String + 'static,
    variant: Btn,
    on_click: impl Fn() + 'static,
) -> Node {
    button_sized(theme, label, variant, 96.0, 36.0, on_click)
}

/// 指定尺寸的按钮（如列表内窄按钮）。
pub fn button_sized(
    theme: Theme,
    label: impl Fn() -> String + 'static,
    variant: Btn,
    w: f32,
    h: f32,
    on_click: impl Fn() + 'static,
) -> Node {
    let label = Rc::new(label);
    let pressed = Signal::new(false);
    let hover = Signal::new(false);

    let mut n = wy_mve::button(
        {
            let label = Rc::clone(&label);
            move || label()
        },
        on_click,
    );
    n.width = w;
    n.height = h;
    n.hit_test_fn = Rc::new(move |x, y| (0.0..w).contains(&x) && (0.0..h).contains(&y));
    n.draw_fn = Rc::new({
        let label = Rc::clone(&label);
        move |scene| {
            if let Some(s) = scene.downcast_mut::<Scene>() {
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
                let lw = wy_render::text_measure::measure_text(&text, 14.0).0;
                s.draw_text(Point::new((w - lw) / 2.0, 10.0), &text, 14.0, fg);
            }
        }
    });
    let pr = pressed;
    n.on_down_fn = Some(Rc::new(move |_| pr.set(true)));
    let pr2 = pressed;
    n.on_up_fn = Some(Rc::new(move |_| pr2.set(false)));
    n
}

/// 颜色缩放（hover/pressed 反馈）。
fn shade(c: Color, f: f32) -> Color {
    let s = |v: u8| ((v as f32) * f) as u8;
    Color::rgba(s(c.red()), s(c.green()), s(c.blue()), c.alpha())
}

/// shadcn 文本（自绘，可定字号/颜色）。
///
/// `color` 是 token 选择器，在绘制期对当前主题求值。
pub fn text_label(
    content: impl Fn() -> String + 'static,
    size: f32,
    theme: Theme,
    color: fn(&Tokens) -> Color,
) -> Node {
    let content = Rc::new(content);
    let mut n = wy_mve::text({
        let content = Rc::clone(&content);
        move || content()
    });
    n.draw_fn = Rc::new({
        let content = Rc::clone(&content);
        move |scene| {
            if let Some(s) = scene.downcast_mut::<Scene>() {
                let c = color(&theme.tokens());
                s.draw_text(Point::new(0.0, 0.0), &(content)(), size, c);
            }
        }
    });
    n.measure_fn = Some(Rc::new(move || {
        wy_render::text_measure::measure_text(&(content)(), size)
    }));
    n
}
