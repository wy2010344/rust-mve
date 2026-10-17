//! 侧边栏：品牌标题、导航按钮、主题切换按钮。

use std::rc::Rc;

use wy_mve::{rsx, spacer, Node, Slot};
use wy_render::{Point, Rect, Scene};
use wy_signal::{GetValue, SetValue};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{button, text_label, Btn};

use super::SIDEBAR_W;

const NAV: [&str; 4] = ["仪表盘", "项目", "设置", "关于"];

pub(super) fn sidebar(theme: Theme, app: &App) -> Node {
    let nav = app.theme_dark;
    let mut v: Vec<Slot> = vec![
        rsx! { text_label { content: move || String::from("▲ shadcn-mve"), size: 17.0, theme: theme, color: |t| t.foreground } },
        spacer(0.0, 16.0).into(),
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
        v.push(item.into());
    }
    v.push(spacer(0.0, 16.0).into());
    // 主题按钮：文案与配色都在绘制期读信号（组件版）
    v.push(rsx! {
        button {
            theme: theme,
            label: move || {
                if nav.get() {
                    String::from("切换到浅色")
                } else {
                    String::from("切换到深色")
                }
            },
            variant: Btn::Ghost,
            on_click: move || nav.set(!nav.get()),
        }
    });
    let mut n = wy_mve::vbox(v);
    n.gap = 4.0;
    n
}
