//! DemoList 复刻：`renderForEach` + 每项独立信号 + 局部重绘。
//!
//! 运行：`cargo run -p wy-app --example demo`
//!
//! 对应 Kotlin `DemoList.kt`：
//! - 顶层按钮统计列表长度并添加条目（写主列表）；
//! - 每项在 creater 里持有自己的信号（文本 / 折叠开关），
//!   显示/隐藏明细只写项信号，不触碰主列表 → 该项局部重绘。

use std::rc::Rc;

use wy_mve::{button, column_at, row, text_signal, Node};
use wy_render::{Color, Rect, Scene};
use wy_signal::{create_signal, GetValue, SetValue, Signal};

#[derive(Clone, PartialEq, Debug)]
struct Item {
    id: u64,
    label: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let state_list = Signal::new(vec![
        Item {
            id: 0,
            label: "row 0".into(),
        },
        Item {
            id: 1,
            label: "row 1".into(),
        },
        Item {
            id: 2,
            label: "row 2".into(),
        },
    ]);
    let next_id = Signal::new(3u64);

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let state_list = state_list.clone();
        let next_id = next_id.clone();
        let push_list = state_list.clone();
        let next = next_id.clone();
        let count_list = state_list.clone();
        cx.child(column_at(50.0, 30.0, move |cx| {
            let cnt = count_list.clone();
            let nxt = next.clone();
            let pl = push_list.clone();
            cx.child(button(
                move || format!("共有{}条 +", cnt.get().len()),
                move || {
                    let id = nxt.get();
                    nxt.set(id + 1);
                    let mut v = pl.get();
                    v.push(Item {
                        id,
                        label: format!("row {id}"),
                    });
                    pl.set(v);
                },
            ));

            let candidates = state_list.clone();
            let for_del = state_list.clone();
            cx.render_for_each(
                move |emit| {
                    for it in candidates.get() {
                        emit(it.id, it);
                    }
                },
                move |id, value, item_cx| {
                    let item = create_signal(value.borrow().clone());
                    let detail = create_signal(false);

                    let item_text = item.clone();
                    let toggle_lbl = detail.clone();
                    let toggle_set = detail.clone();
                    let detail_show = detail.clone();
                    let del_src = for_del.clone();
                    item_cx.child(row(move |cx| {
                        let it = item_text.clone();
                        cx.child(text_signal(move || {
                            let i = it.get();
                            Box::new(format!("#{} {}", i.id, i.label))
                        }));

                        let lbl = toggle_lbl.clone();
                        let set = toggle_set.clone();
                        cx.child(button(
                            move || {
                                if lbl.get() {
                                    "hide".into()
                                } else {
                                    "show".into()
                                }
                            },
                            move || set.set(!set.get()),
                        ));

                        let d = detail_show.clone();
                        cx.child(Node {
                            draw_fn: Rc::new(move |scene| {
                                if !d.get() {
                                    return;
                                }
                                if let Some(scene) = scene.downcast_mut::<Scene>() {
                                    scene.fill_round_rect(
                                        Rect::new(0.0, 40.0, 140.0, 20.0),
                                        4.0,
                                        Color::from_u32(0xFF_F0F0F0),
                                    );
                                }
                            }),
                            ..Node::default()
                        });

                        let del = del_src.clone();
                        cx.child(button(
                            move || "×".into(),
                            move || {
                                let mut v = del.get();
                                v.retain(|i| i.id != id);
                                del.set(v);
                            },
                        ));
                    }));
                },
            );
        }));
    });

    wy_engine::runner::run(app)
}
