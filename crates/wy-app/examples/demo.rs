//! DemoList 复刻：`rsx!` 声明式写法 + 每项独立信号 + 局部重绘。
//!
//! 运行：`cargo run -p wy-app --example demo`
//!
//! 对应 Kotlin `DemoList.kt`：
//! - 顶层按钮统计列表长度并添加条目（写主列表）；
//! - 每项在 creater 里持有自己的信号（文本 / 折叠开关），
//!   显示/隐藏明细只写项信号，不触碰主列表 → 该项局部重绘；
//! - 顶层容器 `gap: if compact.get() { .. } else { .. }` —— 属性是动态的，
//!   由宏包成闭包、布局期求值、读信号自动追踪
//!   （Kotlin `override val gap get() = ...`）。
//!
//! 写法与 `todo.rs` 一致的 `rsx!` 宏；自定义绘制（明细条）以普通函数
//! 返回 `Node`，宏按普通组件调用处理。

use std::rc::Rc;

use wy_mve::rsx;
use wy_render::{Color, Rect, Scene};
use wy_signal::{create_signal, GetValue, SetValue, Signal};

#[derive(Clone, PartialEq, Debug)]
struct Item {
    id: u64,
    label: String,
}

/// 明细条：`show` 为真时绘制一条灰色提示带。
///
/// 抽成函数而非内联 `Node { .. }`，是为了让 rsx! 能把它当普通组件调用
/// （返回 `Node`，宏会包成 `Slot`）。
fn detail_bar(show: impl Fn() -> bool + 'static) -> wy_mve::Node {
    wy_mve::Node {
        draw_fn: Rc::new(move |scene| {
            if !show() {
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
        ..wy_mve::Node::default()
    }
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
    // 紧凑模式：驱动顶层容器的动态 gap（Kotlin `override val gap get() = ...`）
    let compact = create_signal(false);

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let node = rsx! {
            vbox_at(50.0, 30.0) {
                // 属性表达式由宏包成闭包，布局期求值、读信号自动追踪
                gap: if compact.get() { 2.0 } else { 10.0 },
                btn {
                    label: if compact.get() { "宽松" } else { "紧凑" },
                    on_click: move || compact.set(!compact.get())
                }
                btn {
                    label: format!("共有{}条 +", state_list.get().len()),
                    on_click: move || {
                        let id = next_id.get();
                        next_id.set(id + 1);
                        let mut v = state_list.get();
                        v.push(Item {
                            id,
                            label: format!("row {id}"),
                        });
                        state_list.set(v);
                    }
                }

                for_each(state_list, |it: &Item| it.id) { |row, _cx| {
                    // 每项自己的信号：改它不影响其它项，也不重列列表
                    let item = create_signal(row.borrow().clone());
                    let detail = create_signal(false);

                    hbox {
                        label(format!("#{} {}", item.get().id, item.get().label))
                        btn {
                            label: if detail.get() { "hide" } else { "show" },
                            on_click: move || detail.set(!detail.get())
                        }
                        detail_bar(move || detail.get())
                        btn {
                            label: "×",
                            on_click: move || {
                                let mut v = state_list.get();
                                let id = row.borrow().id;
                                v.retain(|i| i.id != id);
                                state_list.set(v);
                            }
                        }
                    }
                }}
            }
        };
        cx.child(node.into_node());
    });

    wy_engine::runner::run(app)
}
