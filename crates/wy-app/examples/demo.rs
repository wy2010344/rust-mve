//! Demo：MVE 模式的列表应用。
//!
//! 对应 Kotlin 的 DemoList.kt，使用 `wy-mve` 的 Node + 信号系统。
//! 复刻 Kotlin MVE 模式：信号在 effect 闭包内读取，不克隆传递。
//! 信号变化时自动重建 Node 树。

use std::rc::Rc;

use wy_mve::Node;
use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{create_signal, GetValue, SetValue};
use wy_text::FontContext;

// --- 数据模型 ---

#[derive(Clone, Debug, PartialEq)]
struct RowItem {
    key: u64,
    hide: bool,
}

impl RowItem {
    fn new(key: u64) -> Self {
        Self { key, hide: false }
    }
}

// --- 辅助：绘制按钮 ---

fn draw_button(scene: &mut Scene, fc: &std::cell::RefCell<FontContext>, label: &str) {
    let (w, h) = fc.borrow_mut().measure_text(label, 14.0);
    scene.fill_round_rect(
        Rect::new(0.0, 0.0, w + 16.0, h + 14.0),
        8.0,
        Color::rgb(232, 232, 232),
    );
    scene.stroke_round_rect(
        Rect::new(0.0, 0.0, w + 16.0, h + 14.0),
        8.0,
        Color::rgb(200, 200, 200),
        2.0,
    );
    scene.draw_text(Point::new(8.0, 7.0), label, 14.0, Color::BLACK);
}

// --- 入口 ---

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // 创建信号（Kotlin 的 remember { mutableStateOf(...) }）
    let state_list = create_signal(vec![
        RowItem::new(0),
        RowItem::new(1),
        RowItem::new(2),
        RowItem::new(3),
        RowItem::new(4),
    ]);
    let toggle_id = create_signal(0u64);
    let next_id = create_signal(5u64);
    let font_cx = Rc::new(std::cell::RefCell::new(FontContext::new()));

    // MveApp::new(callback) — callback 中读取的信号会被 effect 自动追踪
    // 信号变化时 effect 自动重建 Node 树
    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        // 计数按钮
        let count = state_list.get().len();
        {
            let fc = font_cx.clone();
            let list = state_list.clone();
            let next = next_id.clone();
            cx.add_node(Node {
                draw_fn: Rc::new(move |scene| {
                    let scene = scene.downcast_mut::<Scene>().unwrap();
                    draw_button(scene, &fc, &format!("共有{count}条数据"));
                }),
                on_click_fn: Some(Rc::new(move |_| {
                    let id = next.get();
                    next.set(id + 1);
                    let mut v = list.get();
                    v.push(RowItem::new(id));
                    list.set(v);
                })),
                ..Node::default()
            });
        }

        // 列表
        let snapshot = state_list.get();
        for item in snapshot {
            if item.hide {
                continue;
            }

            let key = item.key;
            let fc = font_cx.clone();
            let toggle = toggle_id.clone();
            let list = state_list.clone();

            cx.add_node(Node {
                draw_fn: Rc::new(move |scene| {
                    let scene = scene.downcast_mut::<Scene>().unwrap();
                    scene.fill_round_rect(
                        Rect::new(0.0, 0.0, 300.0, 40.0),
                        8.0,
                        Color::rgb(240, 240, 240),
                    );
                }),
                arg_children_fn: Rc::new(move |child_cx| {
                    // show 按钮
                    {
                        let fc = fc.clone();
                        let toggle = toggle.clone();
                        child_cx.add_node(Node {
                            draw_fn: Rc::new(move |scene| {
                                let scene = scene.downcast_mut::<Scene>().unwrap();
                                draw_button(scene, &fc, &format!("show {key}"));
                            }),
                            on_click_fn: Some(Rc::new(move |_| {
                                toggle.set(key);
                            })),
                            ..Node::default()
                        });
                    }

                    // hide 按钮
                    {
                        let fc = fc.clone();
                        let toggle = toggle.clone();
                        child_cx.add_node(Node {
                            draw_fn: Rc::new(move |scene| {
                                let scene = scene.downcast_mut::<Scene>().unwrap();
                                draw_button(scene, &fc, &format!("hide {key}"));
                            }),
                            on_click_fn: Some(Rc::new(move |_| {
                                toggle.set(key);
                            })),
                            ..Node::default()
                        });
                    }

                    // delete 按钮
                    {
                        let fc = fc.clone();
                        let list = list.clone();
                        child_cx.add_node(Node {
                            draw_fn: Rc::new(move |scene| {
                                let scene = scene.downcast_mut::<Scene>().unwrap();
                                draw_button(scene, &fc, &format!("delete {key}"));
                            }),
                            on_click_fn: Some(Rc::new(move |_| {
                                let current = list.get();
                                let filtered: Vec<RowItem> =
                                    current.into_iter().filter(|x| x.key != key).collect();
                                list.set(filtered);
                            })),
                            ..Node::default()
                        });
                    }
                }),
                ..Node::default()
            });
        }
    });

    let _ = wy_engine::runner::run(app);
}
