//! 侧边栏：品牌标题、导航按钮、主题切换按钮。

use std::rc::Rc;

use wy_mve::{spacer, Node};
use wy_render::{Point, Rect, Scene};
use wy_signal::{GetValue, SetValue};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{button, text_label, Btn};

use super::SIDEBAR_W;

const NAV: [&str; 4] = ["仪表盘", "项目", "设置", "关于"];

pub(super) fn sidebar(theme: Theme, app: &App) -> Node {
    let nav = app.theme_dark;
    let mut v: Vec<Node> = vec![
        text_label(
            move || String::from("▲ shadcn-mve"),
            17.0,
            theme,
            |t| t.foreground,
        ),
        spacer(0.0, 16.0),
    ];
    for name in NAV {
        let name = name.to_string();
        let mut item = wy_mve::button(
            {
                let name = name.clone();
                move || name.clone()
            },
            || {},
        );
        item.width = SIDEBAR_W;
        item.height = 34.0;
        item.hit_test_fn =
            Rc::new(move |x, y| (0.0..SIDEBAR_W).contains(&x) && (0.0..34.0).contains(&y));
        item.draw_fn = Rc::new(move |scene| {
            if let Some(s) = scene.downcast_mut::<Scene>() {
                let t = theme.tokens();
                s.fill_round_rect(Rect::new(0.0, 0.0, SIDEBAR_W, 34.0), 6.0, t.accent);
                s.draw_text(Point::new(12.0, 9.0), &name, 14.0, t.muted_fg);
            }
        });
        v.push(item);
    }
    v.push(spacer(0.0, 16.0));
    // 主题按钮：文案与配色都在绘制期读信号
    v.push(button(
        theme,
        move || {
            if nav.get() {
                String::from("切换到浅色")
            } else {
                String::from("切换到深色")
            }
        },
        Btn::Ghost,
        move || nav.set(!nav.get()),
    ));
    let mut n = wy_mve::vbox(v);
    n.gap = 4.0;
    n
}
