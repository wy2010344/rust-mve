//! DemoList 复刻：`renderForEach` + 每项独立信号 + 局部重绘。
//!
//! 运行：`cargo run -p wy-app --example demo`
//!
//! 对应 Kotlin `DemoList.kt`：
//! - 顶层按钮统计列表长度并添加条目（写主列表）；
//! - 每项在 creater 里持有自己的信号（文本 / 折叠开关），
//!   显示/隐藏明细只写项信号，不触碰主列表 → 该项局部重绘。
//!
//! 组件写法用 FC 风格 DSL：`vbox_at`/`hbox` 子节点作剩余参数，
//! 原子组件用 `btn`/`label`（返回 `Slot`），可与 `for_each` 混在同一数组。

use std::rc::Rc;

use wy_mve::{btn, for_each, hbox, label, vbox_at, Node, Slot};
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
        let each_list = state_list.clone();

        cx.child(vbox_at(
            50.0,
            30.0,
            [
                btn(
                    move || format!("共有{}条 +", count_list.get().len()),
                    move || {
                        let id = next.get();
                        next.set(id + 1);
                        let mut v = push_list.get();
                        v.push(Item {
                            id,
                            label: format!("row {id}"),
                        });
                        push_list.set(v);
                    },
                ),
                for_each(
                    move |emit| {
                        for it in each_list.get() {
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
                        let del_src = state_list.clone();

                        item_cx.child(hbox([
                            label(move || {
                                let i = item_text.get();
                                format!("#{} {}", i.id, i.label)
                            }),
                            btn(
                                move || {
                                    if toggle_lbl.get() {
                                        "hide".into()
                                    } else {
                                        "show".into()
                                    }
                                },
                                move || toggle_set.set(!toggle_set.get()),
                            ),
                            // 明细条：条件显示，直接用 Node 字面量包成 Slot
                            Slot::One(Node {
                                draw_fn: Rc::new(move |scene| {
                                    if !detail_show.get() {
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
                            }),
                            btn(
                                || "×".into(),
                                move || {
                                    let mut v = del_src.get();
                                    v.retain(|i| i.id != id);
                                    del_src.set(v);
                                },
                            ),
                        ]));
                    },
                ),
            ],
        ));
    });

    wy_engine::runner::run(app)
}
