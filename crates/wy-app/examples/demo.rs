//! Demo：MVE 模式的列表应用。
//!
//! 对应 Kotlin 的 DemoList.kt，使用 `wy-mve` 的 Node + 信号系统。
//! 每帧实时读取信号构建 Node 树，无缓存。

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
    toggle_signal: wy_signal::Signal<u64>,
    list_signal: wy_signal::Signal<Vec<RowItem>>,
) {
    let key = item.key;
    let hide = item.hide;

    if hide {
        return;
    }

    // 行容器
    let fc_ref = fc.clone();
    let toggle_ref = toggle_signal.clone();
    let list_ref = list_signal.clone();

    cx.add_node(
        Node::new()
            .draw(move |scene| {
                let scene = scene.downcast_mut::<Scene>().unwrap();
                scene.fill_round_rect(
                    Rect::new(0.0, 0.0, 300.0, 40.0),
                    8.0,
                    Color::rgb(240, 240, 240),
                );
            })
            .arg_children(move |child_cx| {
                // show 按钮
                {
                    let fc = fc_ref.clone();
                    let toggle = toggle_ref.clone();
                    let k = key;
                    child_cx.add_node(
                        Node::new()
                            .draw(move |scene| {
                                let scene = scene.downcast_mut::<Scene>().unwrap();
                                draw_button(scene, &fc, &format!("show {k}"));
                            })
                            .on_click(move |_| {
                                toggle.set(k);
                            }),
                    );
                }

                // hide 按钮
                {
                    let fc = fc_ref.clone();
                    let toggle = toggle_ref.clone();
                    let k = key;
                    child_cx.add_node(
                        Node::new()
                            .draw(move |scene| {
                                let scene = scene.downcast_mut::<Scene>().unwrap();
                                draw_button(scene, &fc, &format!("hide {k}"));
                            })
                            .on_click(move |_| {
                                toggle.set(k);
                            }),
                    );
                }

                // delete 按钮
                {
                    let fc = fc_ref.clone();
                    let list = list_ref.clone();
                    let k = key;
                    child_cx.add_node(
                        Node::new()
                            .draw(move |scene| {
                                let scene = scene.downcast_mut::<Scene>().unwrap();
                                draw_button(scene, &fc, &format!("delete {k}"));
                            })
                            .on_click(move |_| {
                                // 实时读取当前列表（不使用缓存）
                                let current = list.get();
                                let filtered: Vec<RowItem> =
                                    current.into_iter().filter(|x| x.key != k).collect();
                                list.set(filtered);
                            }),
                    );
                }
            }),
    );
}

// --- 入口 ---

fn main() {
    // 初始化日志：过滤 Parley/ICU4X 的 CJK 分段警告
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let state_list = create_signal(vec![
        RowItem::new(0),
        RowItem::new(1),
        RowItem::new(2),
        RowItem::new(3),
        RowItem::new(4),
    ]);

    let toggle_id = create_signal(0u64);
    let next_id = create_signal(5u64);

    let font_cx = std::rc::Rc::new(std::cell::RefCell::new(FontContext::new()));

    // MVE 应用：每帧实时读取信号构建 Node 树
    let app = wy_engine::mve_integration::MveApp::new({
        let list = state_list.clone();
        let fc = font_cx.clone();
        let toggle = toggle_id.clone();
        let next = next_id.clone();

        move |cx| {
            // 计数按钮（实时读取 list 长度）
            let list_ref = list.clone();
            let next_ref = next.clone();
            let fc_ref = fc.clone();
            let count = list.get().len();
            cx.add_node(
                Node::new()
                    .draw(move |scene| {
                        let scene = scene.downcast_mut::<Scene>().unwrap();
                        draw_button(scene, &fc_ref, &format!("共有{count}条数据"));
                    })
                    .on_click(move |_| {
                        let id = next_ref.get();
                        next_ref.set(id + 1);
                        let mut v = list_ref.get();
                        v.push(RowItem::new(id));
                        list_ref.set(v);
                    }),
            );

            // 列表（实时读取 list 信号）
            let snapshot = list.get();
            for item in snapshot {
                build_list_item(cx, item, fc.clone(), toggle.clone(), list.clone());
            }
        }
    });

    // 启动渲染
    let _ = wy_engine::runner::run(app);
}
