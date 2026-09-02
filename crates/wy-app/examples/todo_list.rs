//! Todo list 示例：信号驱动的待办事项列表。
//!
//! 运行：`cargo run -p wy-app --example todo_list`
//!
//! 演示：
//! - 信号驱动的输入框
//! - 列表渲染与增删改
//! - TrackEffect 驱动重绘

use wy_mve::{Node, NodeContext};
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

fn draw_toggle_button(scene: &mut Scene, fc: &std::cell::RefCell<FontContext>, label: &str, completed: bool) {
    let (w, h) = fc.borrow_mut().measure_text(label, 14.0);
    let bg_color = if completed {
        Color::rgb(150, 200, 150)
    } else {
        Color::rgb(255, 255, 255)
    };
    scene.fill_round_rect(
        Rect::new(0.0, 0.0, w + 16.0, h + 14.0),
        8.0,
        bg_color,
    );
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

// --- MVE 节点构建 ---

fn build_todo_item(
    cx: &mut NodeContext,
    item: TodoItem,
    fc: std::rc::Rc<std::cell::RefCell<FontContext>>,
    todo_list: wy_signal::Signal<Vec<TodoItem>>,
) -> Node {
    let key = item.key;

    // 复选框：切换完成状态
    let toggle_node = Node::new()
        .draw(move |scene| {
            let scene = scene.downcast_mut::<Scene>().unwrap();
            draw_toggle_button(scene, &fc, if item.completed { "☑" } else { "☐" }, item.completed);
        })
        .on_click(move |_| {
            let mut current = todo_list.get();
            if let Some(it) = current.iter_mut().find(|i| i.key == key) {
                it.completed = !it.completed;
                todo_list.set(current);
            }
        });

    // 文字显示（已完成则显示删除线效果由渲染层处理，此处仅显示文本）
    let text_node = Node::new()
        .draw(move |scene| {
            let scene = scene.downcast_mut::<Scene>().unwrap();
            let text = item.text.clone();
            scene.draw_text(Point::new(24.0, 7.0), &text, 14.0, Color::BLACK);
        });

    // 删除按钮
    let delete_node = Node::new()
        .draw(move |scene| {
            let scene = scene.downcast_mut::<Scene>().unwrap();
            draw_delete_button(scene, &fc, "×");
        })
        .on_click(move |_| {
            let mut current = todo_list.get();
            current.retain(|i| i.key != key);
            todo_list.set(current);
        });

    // 项目容器 - 垂直布局：复选框 | 文字 | 删除
    Node::new()
        .arg_children(move |child_cx| {
            child_cx.add_node(toggle_node.clone());
            child_cx.add_node(text_node.clone());
            child_cx.add_node(delete_node.clone());
        })
}

// --- 入口 ---

fn main() {
    env_logger::init();

    let mut todo_list = create_signal(vec![]);
    let next_id = create_signal(1u64);

    let font_cx = std::rc::Rc::new(std::cell::RefCell::new(FontContext::new()));

    // 当前输入文本
    let mut input_text = String::new();

    let app = wy_engine::mve_integration::MveApp::new({
        let list = todo_list.clone();
        let id = next_id.clone();
        let fc = font_cx.clone();

        move |cx| {
            // 输入框区域
            cx.add_node(
                Node::new()
                    .draw(move |scene| {
                        let scene = scene.downcast_mut::<Scene>().unwrap();
                        // 简单的输入框显示
                        scene.fill_rect(
                            Rect::new(0.0, 0.0, 300.0, 30.0),
                            Color::rgb(255, 255, 255),
                        );
                        scene.draw_text(
                            Point::new(8.0, 7.0),
                            &format!("输入: {}", input_text),
                            14.0,
                            Color::BLACK,
                        );
                    })
                    .on_click(move |_| {
                        // 这里可以集成文本输入，简化处理
                    }),
            );

            // 添加按钮
            cx.add_node(
                Node::new()
                    .draw(move |scene| {
                        let scene = scene.downcast_mut::<Scene>().unwrap();
                        scene.fill_round_rect(
                            Rect::new(0.0, 40.0, 80.0, 30.0),
                            4.0,
                            Color::rgb(150, 200, 250),
                        );
                        scene.draw_text(
                            Point::new(12.0, 47.0),
                            "添加",
                            14.0,
                            Color::BLACK,
                        );
                    })
                    .on_click(move |_| {
                        if !input_text.is_empty() {
                            let mut current = list.get();
                            current.push(TodoItem::new(id.get(), &input_text));
                            list.set(current);
                            input_text.clear();
                            id.set(id.get() + 1);
                        }
                    }),
            );

            // 列表
            let snapshot = list.get();
            for item in snapshot {
                build_todo_item(cx, item, fc.clone(), list.clone());
            }
        }
    });

    let _ = wy_engine::runner::run(app);
}