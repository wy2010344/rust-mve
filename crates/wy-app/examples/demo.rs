//! Demo：MVE 模式的列表应用。
//!
//! 对应 Kotlin 的 DemoList.kt，使用 `wy-mve` 的 Node + 信号系统。
//! 每帧实时读取信号构建 Node 树，无缓存。
//!
//! 实现了以下 Kotlin 特性：
//! - createSignal 创建信号（一次性构建，非反复 render）
//! - list 的增删改查
//! - 信号驱动的隐藏/显示
//! - 计数显示与悬停效果

use wy_mve::{Node, NodeContext};
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

// --- MVE 节点构建 ---

fn build_list_item(
    cx: &mut NodeContext,
    item: RowItem,
    fc: std::rc::Rc<std::cell::RefCell<FontContext>>,
    list_signal: wy_signal::Signal<Vec<RowItem>>,
) -> Node {
    let key = item.key;

    if item.hide {
        return Node::new();
    }

    let fc_ref = fc.clone();
    let list_ref = list_signal.clone();

    // 行容器 - 使用 Flex 布局，方向为水平，两端对齐
    let row_node = Node::new()
        .draw(move |scene| {
            let scene = scene.downcast_mut::<Scene>().unwrap();
            scene.fill_round_rect(
                Rect::new(0.0, 0.0, 300.0, 40.0),
                8.0,
                Color::rgb(240, 240, 240),
            );
        })
        .arg_children(move |child_cx| {
            // key 文本
            {
                let fc = fc_ref.clone();
                child_cx.add_node(
                    Node::new()
                        .draw(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            scene.draw_text(
                                Point::new(8.0, 7.0),
                                &format!("key-{}", key),
                                12.0,
                                Color::BLACK,
                            );
                        }),
                );
            }

            // show 按钮：显示所有项
            {
                let fc = fc_ref.clone();
                child_cx.add_node(
                    Node::new()
                        .draw(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            draw_button(scene, &fc, "show");
                        })
                        .on_click(move |_| {
                            // Kotlin: list.forEach { it.hide = false }
                            let mut current = list_ref.get();
                            for it in current.iter_mut() {
                                it.hide = false;
                            }
                            list_ref.set(current);
                        }),
                );
            }

            // hide 按钮：隐藏当前项
            {
                let fc = fc_ref.clone();
                child_cx.add_node(
                    Node::new()
                        .draw(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            draw_button(scene, &fc, "hide");
                        })
                        .on_click(move |_| {
                            // Kotlin: it.value.hide = true
                            let mut current = list_ref.get();
                            if let Some(it) = current.iter_mut().find(|i| i.key == key) {
                                it.hide = true;
                                list_ref.set(current);
                            }
                        }),
                );
            }

            // delete 按钮：删除当前项（并用时间戳替换）
            {
                let fc = fc_ref.clone();
                let list = list_ref.clone();
                let k = key;
                child_cx.add_node(
                    Node::new()
                        .draw(move |scene| {
                            let scene = scene.downcast_mut::<Scene>().unwrap();
                            draw_button(scene, &fc, "delete");
                        })
                        .on_click(move |_| {
                            // Kotlin: list = mutableListOf<RowModal>().also { it.addAll(list); it.add(RowModal(Date().time)) }
                            let current = list.get();
                            let mut new_list = current
                                .into_iter()
                                .filter(|x| x.key != k)
                                .collect();
                            // 添加新条目，键作为标识
                            new_list.push(RowItem::new(k + 1000));
                            list.set(new_list);
                        }),
                );
            }
        });

    row_node
}

// --- 入口 ---

fn main() {
    // 初始化日志
    env_logger::init();

    let mut state_list = create_signal(vec![
        RowItem::new(0),
        RowItem::new(1),
        RowItem::new(2),
        RowItem::new(3),
        RowItem::new(4),
    ]);

    let next_id = create_signal(5u64);

    let font_cx = std::rc::Rc::new(std::cell::RefCell::new(FontContext::new()));

    // MVE 应用：每帧实时读取信号构建 Node 树
    let app = wy_engine::mve_integration::MveApp::new({
        let list = state_list.clone();
        let fc = font_cx.clone();
        let id = next_id.clone();

        move |cx| {
            // + 按钮：添加新项
            let list_ref = list.clone();
            let next_ref = next_id.clone();
            let fc_ref = fc.clone();
            cx.add_node(
                Node::new()
                    .draw(move |scene| {
                        let scene = scene.downcast_mut::<Scene>().unwrap();
                        // 计数文本
                        let count = list_ref.get().len();
                        let text = format!("共有{}条数据 ", count);
                        scene.fill_round_rect(
                            Rect::new(0.0, 0.0, 200.0, 40.0),
                            8.0,
                            Color::rgb(240, 240, 240),
                        );
                        scene.draw_text(
                            Point::new(8.0, 7.0),
                            &text,
                            14.0,
                            Color::BLACK,
                        );
                    })
                    .on_click(move |_| {
                        let id = next_ref.get();
                        next_ref.set(id + 1);
                        let mut v = list_ref.get();
                        v.push(RowItem::new(id));
                        list_ref.set(v);
                    }),
            );

            // 列表：实时读取 list 信号
            let snapshot = list.get();
            for item in snapshot {
                build_list_item(cx, item, fc.clone(), list.clone());
            }
        }
    });

    // 启动渲染
    let _ = wy_engine::runner::run(app);
}