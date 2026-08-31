use wy_engine::composition::run_composition;
use wy_render::composition::FnWidget;
use wy_render::{Color, Point, Rect};
use wy_signal::{create_memo, create_signal, GetValue, SetValue};
use wy_text::FontContext;

#[derive(Clone, PartialEq, Debug)]
struct RowItem {
    key: u64,
    show: bool,
}

impl RowItem {
    fn new(key: u64) -> Self {
        Self { key, show: true }
    }
}

fn measure_text(label: &str, fc: &std::cell::RefCell<FontContext>) -> (f32, f32) {
    fc.borrow_mut().measure_text(label, 14.0)
}

fn draw_button(scene: &mut wy_render::Scene, label: &str, fc: &std::cell::RefCell<FontContext>) {
    let (w, h) = measure_text(label, fc);
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

fn main() {
    let state_list = create_signal(vec![
        RowItem::new(0),
        RowItem::new(1),
        RowItem::new(2),
        RowItem::new(3),
        RowItem::new(4),
    ]);

    let toggle_id = create_signal(1u64);
    let next_id = create_signal(5u64);

    let n_memo = create_memo({
        let list = state_list.clone();
        move || {
            let len = list.get().len();
            format!("共有{len}条数据")
        }
    });

    let font_cx = std::rc::Rc::new(std::cell::RefCell::new(FontContext::new()));

    run_composition(move |cx| {
        // WrappedTextNode —— "共有N条数据" 按钮
        let fc = font_cx.clone();
        let counter_text = n_memo.clone();
        let counter_list = state_list.clone();
        let counter_next = next_id.clone();
        cx.add_child(
            FnWidget::new(
                move |scene, _cx| {
                    let label = counter_text.get();
                    draw_button(scene, &label, &fc);
                },
                |_| {},
            )
            .on_click(move |_| {
                let id = counter_next.get();
                counter_next.set(id + 1);
                let mut v = counter_list.get();
                v.push(RowItem::new(id));
                counter_list.set(v);
            }),
        );

        // SimpleScrollNode —— 列表容器
        let list_for_scroll = state_list.clone();
        let fc_for_scroll = font_cx.clone();
        let toggle_for_scroll = toggle_id.clone();
        let state_for_scroll = state_list.clone();
        cx.add_child(FnWidget::new(
            |scene, _| {
                scene.fill_rect(Rect::new(0.0, 0.0, 300.0, 600.0), Color::WHITE);
            },
            move |cx| {
                let snapshot = list_for_scroll.get();
                for item in snapshot {
                    let item = item.clone();
                    let show = item.show;
                    let key = item.key;

                    let fc = fc_for_scroll.clone();
                    let toggle_id = toggle_for_scroll.clone();
                    let state_list = state_for_scroll.clone();

                    // 每行容器
                    cx.add_child(FnWidget::new(
                        move |scene, cx| {
                            if !show {
                                return;
                            }
                            let rect = cx.outer_rect();
                            scene.fill_round_rect(rect, 8.0, Color::rgb(240, 240, 240));
                        },
                        move |cx| {
                            if !show {
                                return;
                            }

                            // show 按钮
                            {
                                let toggle_id = toggle_id.clone();
                                let show_label = create_memo(move || format!("show {key}"));
                                let fc = fc.clone();
                                cx.add_child(
                                    FnWidget::new(
                                        move |scene, _| {
                                            let label = show_label.get();
                                            draw_button(scene, &label, &fc);
                                        },
                                        |_| {},
                                    )
                                    .on_click(move |_| {
                                        toggle_id.set(key);
                                    }),
                                );
                            }

                            // hide 按钮
                            {
                                let toggle_id = toggle_id.clone();
                                let hide_label = create_memo(move || format!("hide {key}"));
                                let fc = fc.clone();
                                cx.add_child(
                                    FnWidget::new(
                                        move |scene, _| {
                                            let label = hide_label.get();
                                            draw_button(scene, &label, &fc);
                                        },
                                        |_| {},
                                    )
                                    .on_click(move |_| {
                                        toggle_id.set(key);
                                    }),
                                );
                            }

                            // delete 按钮
                            {
                                let state_list = state_list.clone();
                                let del_label = create_memo(move || format!("delete {key}"));
                                let fc = fc.clone();
                                cx.add_child(
                                    FnWidget::new(
                                        move |scene, _| {
                                            let label = del_label.get();
                                            draw_button(scene, &label, &fc);
                                        },
                                        |_| {},
                                    )
                                    .on_click(move |_| {
                                        let k = key;
                                        let current = state_list.get();
                                        state_list.set(
                                            current.into_iter().filter(|x| x.key != k).collect(),
                                        );
                                    }),
                                );
                            }
                        },
                    ));
                }
            },
        ));
    });
}
