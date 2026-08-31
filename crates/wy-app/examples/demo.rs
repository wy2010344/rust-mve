//! Kotlin 风格嵌套组合 Demo。
//!
//! 复刻 C:\github\wy-helper\desktopApp\src\main\kotlin\org\wy\engine\DemoList.kt
//! 使用 `FnWidget` + `ChildBuilderExt` 实现函数嵌套组合。
//!
//! 运行：`cargo run -p wy-app --example demo`

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use wy_engine::runner::{run, WyApp};
use wy_render::composition::FnWidget;
use wy_render::theme::Theme;
use wy_render::widget_tree::WidgetTree;
use wy_render::{Color, Rect, Scene};
use wy_signal::{create_effect, GetValue, SetValue, Signal};

/// 列表项数据（类似 Kotlin 的 `class RowModal(val key: Long)`）。
#[derive(Clone)]
struct RowItem {
    key: u64,
    hide: Signal<bool>,
}

impl PartialEq for RowItem {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl RowItem {
    fn new(key: u64) -> Self {
        Self {
            key,
            hide: Signal::new(false),
        }
    }
}

/// Demo 应用状态。
struct DemoApp {
    tree: Option<WidgetTree>,
    list: Signal<Vec<RowItem>>,
    mouse_x: f64,
    mouse_y: f64,
}

impl DemoApp {
    fn new() -> Self {
        Self {
            tree: None,
            list: Signal::new(Vec::new()),
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }

    /// 构建 UI 树 — Kotlin 风格的 `argChildren()`。
    fn build_ui(&mut self) {
        let list = self.list.clone();
        let tree =
            WidgetTree::new(FnWidget::new(
                |scene, cx| {
                    scene.fill_rect(cx.outer_rect(), Theme::light().colors.background);
                },
                move |cx| {
                    // ── "共有N条数据" 按钮 ──
                    let list_c = list.clone();
                    let list_c2 = list.clone();
                    cx.add_child(
                        FnWidget::new(
                            move |scene, cx| {
                                let rect = cx.outer_rect();
                                let t = Theme::light();
                                let count = list_c.get().len();
                                scene.fill_rect(rect, t.colors.button_background);
                                scene.draw_text(
                                    wy_render::Point::new(rect.x + 4.0, rect.y + 6.0),
                                    &format!("共有{count}条数据 (点击添加)"),
                                    t.sizes.font_size,
                                    t.colors.text,
                                );
                            },
                            |_| {},
                        )
                        .on_click(move |_cx| {
                            let key = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64;
                            let mut items = list_c2.get();
                            items.push(RowItem::new(key));
                            list_c2.set(items);
                        }),
                    );

                    // ── 列表容器 ──
                    let list_for_items = list.clone();
                    cx.add_child(FnWidget::new(
                        |scene, cx| {
                            scene.fill_rect(cx.outer_rect(), Theme::light().colors.background);
                        },
                        move |cx| {
                            let items = list_for_items.get();
                            for (i, item) in items.iter().enumerate() {
                                let item = item.clone();
                                let _item_index = i;
                                let list = list_for_items.clone();

                                // ── 列表项 ──
                                let item_c = item.clone();
                                cx.add_child(FnWidget::new(
                                    move |scene, cx| {
                                        if item_c.hide.get() {
                                            return;
                                        }
                                        let rect = cx.outer_rect();
                                        let t = Theme::light();
                                        scene.fill_round_rect(
                                            rect,
                                            t.sizes.border_radius,
                                            t.colors.button_background,
                                        );
                                        scene.draw_text(
                                            wy_render::Point::new(rect.x + 4.0, rect.y + 8.0),
                                            &format!("key-{}", item_c.key),
                                            t.sizes.font_size,
                                            t.colors.text,
                                        );
                                    },
                                    {
                                        let item = item.clone();
                                        let list = list.clone();
                                        move |cx| {
                                            // "隐藏" 按钮
                                            let item_h = item.clone();
                                            cx.add_child(
                                                FnWidget::new(
                                                    move |scene, cx| {
                                                        if item_h.hide.get() {
                                                            return;
                                                        }
                                                        let rect = cx.outer_rect();
                                                        let t = Theme::light();
                                                        scene.fill_round_rect(
                                                            Rect::new(
                                                                rect.x + rect.width - 60.0,
                                                                rect.y + 4.0,
                                                                52.0,
                                                                22.0,
                                                            ),
                                                            4.0,
                                                            t.colors.primary,
                                                        );
                                                        scene.draw_text(
                                                            wy_render::Point::new(
                                                                rect.x + rect.width - 52.0,
                                                                rect.y + 8.0,
                                                            ),
                                                            "隐藏",
                                                            12.0,
                                                            Color::WHITE,
                                                        );
                                                    },
                                                    |_| {},
                                                )
                                                .on_click({
                                                    let item_h = item.clone();
                                                    move |_cx| {
                                                        item_h.hide.set(true);
                                                    }
                                                }),
                                            );

                                            // "删除" 按钮
                                            let item_d = item.clone();
                                            let list_d = list.clone();
                                            cx.add_child(
                                                FnWidget::new(
                                                    move |scene, cx| {
                                                        if item_d.hide.get() {
                                                            return;
                                                        }
                                                        let rect = cx.outer_rect();
                                                        scene.fill_round_rect(
                                                            Rect::new(
                                                                rect.x + rect.width - 116.0,
                                                                rect.y + 4.0,
                                                                52.0,
                                                                22.0,
                                                            ),
                                                            4.0,
                                                            Color::RED,
                                                        );
                                                        scene.draw_text(
                                                            wy_render::Point::new(
                                                                rect.x + rect.width - 108.0,
                                                                rect.y + 8.0,
                                                            ),
                                                            "删除",
                                                            12.0,
                                                            Color::WHITE,
                                                        );
                                                    },
                                                    |_| {},
                                                )
                                                .on_click(move |_cx| {
                                                    let key = item_d.key;
                                                    let items: Vec<RowItem> = list_d
                                                        .get()
                                                        .into_iter()
                                                        .filter(|it| it.key != key)
                                                        .collect();
                                                    list_d.set(items);
                                                }),
                                            );
                                        }
                                    },
                                ));
                            }
                        },
                    ));
                },
            ));

        self.tree = Some(tree);
    }
}

impl WyApp for DemoApp {
    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        let list = self.list.clone();
        create_effect(move || {
            let _ = list.get();
            request_redraw();
        });
        self.build_ui();
    }

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        if let Some(tree) = &mut self.tree {
            tree.set_layout(0, Rect::new(0.0, 0.0, width, height));
            tree.draw_scene(scene);
        }
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use winit::keyboard::Key;

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x;
                self.mouse_y = position.y;
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(tree) = &mut self.tree {
                    let x = self.mouse_x as f32;
                    let y = self.mouse_y as f32;
                    tree.dispatch_pointer_down(x, y);
                    tree.dispatch_pointer_up(x, y)
                } else {
                    false
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Character(s) if s.as_str() == "\r" => {
                        let key = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                        let mut items = self.list.get();
                        items.push(RowItem::new(key));
                        self.list.set(items);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(DemoApp::new())
}
