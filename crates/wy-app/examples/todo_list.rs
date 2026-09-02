//! Todo list 示例：信号驱动的待办事项列表。
//!
//! 运行：`cargo run -p wy-app --example todo_list`
//!
//! 演示：
//! - 信号驱动的输入框
//! - 列表渲染与增删改
//! - effect 监听信号，自动重建 Node 树

use std::rc::Rc;

use wy_mve::Node;
use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{create_signal, GetValue, SetValue};
use wy_text::FontContext;

// --- 数据模型 ---

#[derive(Clone, Debug, PartialEq)]
struct TodoItem {
    key: u64,
    text: String,
    completed: bool,
}

impl TodoItem {
    fn new(key: u64, text: &str) -> Self {
        Self {
            key,
            text: text.to_string(),
            completed: false,
        }
    }
}

// --- 辅助：绘制项目 ---

fn draw_toggle_button(
    scene: &mut Scene,
    fc: &std::cell::RefCell<FontContext>,
    label: &str,
    completed: bool,
) {
    let (w, h) = fc.borrow_mut().measure_text(label, 14.0);
    let bg_color = if completed {
        Color::rgb(150, 200, 150)
    } else {
        Color::rgb(255, 255, 255)
    };
    scene.fill_round_rect(Rect::new(0.0, 0.0, w + 16.0, h + 14.0), 8.0, bg_color);
    scene.stroke_round_rect(
        Rect::new(0.0, 0.0, w + 16.0, h + 14.0),
        8.0,
        Color::rgb(180, 180, 180),
        1.0,
    );
    scene.draw_text(Point::new(8.0, 7.0), label, 14.0, Color::BLACK);
}

fn draw_delete_button(scene: &mut Scene, fc: &std::cell::RefCell<FontContext>, label: &str) {
    let (w, h) = fc.borrow_mut().measure_text(label, 14.0);
    scene.fill_round_rect(
        Rect::new(0.0, 0.0, w + 16.0, h + 14.0),
        8.0,
        Color::rgb(255, 230, 230),
    );
    scene.stroke_round_rect(
        Rect::new(0.0, 0.0, w + 16.0, h + 14.0),
        8.0,
        Color::rgb(220, 150, 150),
        1.0,
    );
    scene.draw_text(Point::new(8.0, 7.0), label, 14.0, Color::BLACK);
}

// --- 入口 ---

fn main() {
    env_logger::init();

    // 创建信号（Kotlin 的 remember { mutableStateOf(...) }）
    let todo_list = create_signal(vec![]);
    let next_id = create_signal(1u64);
    let font_cx = Rc::new(std::cell::RefCell::new(FontContext::new()));
    let input_text = Rc::new(std::cell::RefCell::new(String::new()));

    // MveApp::new(callback) — callback 中读取的信号会被 effect 自动追踪
    let app = wy_engine::mve_integration::MveApp::new({
        let list = todo_list.clone();
        let id = next_id.clone();
        let fc = font_cx.clone();
        let input = input_text.clone();

        move |cx| {
            // 输入框区域
            {
                let input = input.clone();
                cx.add_node(Node {
                    draw_fn: Rc::new(move |scene| {
                        let scene = scene.downcast_mut::<Scene>().unwrap();
                        scene
                            .fill_rect(Rect::new(0.0, 0.0, 300.0, 30.0), Color::rgb(255, 255, 255));
                        let text = input.borrow();
                        scene.draw_text(
                            Point::new(8.0, 7.0),
                            &format!("输入: {text}"),
                            14.0,
                            Color::BLACK,
                        );
                    }),
                    ..Node::default()
                });
            }

            // 添加按钮
            {
                let list = list.clone();
                let id = id.clone();
                let input = input.clone();
                cx.add_node(Node {
                    draw_fn: Rc::new(move |scene| {
                        let scene = scene.downcast_mut::<Scene>().unwrap();
                        scene.fill_round_rect(
                            Rect::new(0.0, 40.0, 80.0, 30.0),
                            4.0,
                            Color::rgb(150, 200, 250),
                        );
                        scene.draw_text(Point::new(12.0, 47.0), "添加", 14.0, Color::BLACK);
                    }),
                    on_click_fn: Some(Rc::new(move |_| {
                        let text = input.borrow();
                        if !text.is_empty() {
                            let mut current = list.get();
                            current.push(TodoItem::new(id.get(), &text));
                            list.set(current);
                            drop(text);
                            input.borrow_mut().clear();
                            id.set(id.get() + 1);
                        }
                    })),
                    ..Node::default()
                });
            }

            // 列表
            let snapshot = list.get();
            for item in snapshot {
                let key = item.key;
                let text = item.text.clone();
                let completed = item.completed;

                // 复选框：切换完成状态
                {
                    let fc = fc.clone();
                    let todo_list = list.clone();
                    cx.add_node(Node {
                        draw_fn: Rc::new(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            draw_toggle_button(
                                scene,
                                &fc,
                                if completed { "☑" } else { "☐" },
                                completed,
                            );
                        }),
                        on_click_fn: Some(Rc::new(move |_| {
                            let mut current = todo_list.get();
                            if let Some(it) = current.iter_mut().find(|i| i.key == key) {
                                it.completed = !it.completed;
                                todo_list.set(current);
                            }
                        })),
                        ..Node::default()
                    });
                }

                // 文字显示
                {
                    cx.add_node(Node {
                        draw_fn: Rc::new(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            scene.draw_text(Point::new(24.0, 7.0), &text, 14.0, Color::BLACK);
                        }),
                        ..Node::default()
                    });
                }

                // 删除按钮
                {
                    let fc = fc.clone();
                    let todo_list = list.clone();
                    cx.add_node(Node {
                        draw_fn: Rc::new(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            draw_delete_button(scene, &fc, "×");
                        }),
                        on_click_fn: Some(Rc::new(move |_| {
                            let mut current = todo_list.get();
                            current.retain(|i| i.key != key);
                            todo_list.set(current);
                        })),
                        ..Node::default()
                    });
                }
            }
        }
    });

    let _ = wy_engine::runner::run(app);
}
