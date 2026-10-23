mod dashboard;
mod state;
mod tokens;
mod widgets;

use std::rc::Rc;

use wy_engine::mve_integration::MveApp;
use wy_mve::NodeContext;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = state::App::new();
    let app = Rc::new(app);

    let app_handle = Rc::clone(&app);
    let mve = MveApp::new(move |cx: &mut NodeContext| {
        let node = dashboard::dashboard(&app_handle);
        cx.child(node);
    });

    // `--snapshot <路径>`：离屏出图后退出（README 配图用，不开窗口）
    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        wy_app::snapshot::save_snapshot(&mve, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    wy_engine::runner::run(mve)
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::rc::Rc;

    use wy_engine::mve_integration::MveApp;
    use wy_engine::runner::WyApp;
    use wy_mve::{
        children_nodes, has_handler, layout_boxes, node_size, Node, NodeContext, PointerEvent,
        Scene,
    };
    use wy_signal::{GetValue, SetValue};

    fn build() -> MveApp {
        build_with_state().0
    }

    /// 构建应用并返回状态句柄（测试要读写状态时用）。
    fn build_with_state() -> (MveApp, Rc<crate::state::App>) {
        let app = Rc::new(crate::state::App::new());
        let h = Rc::clone(&app);
        let mve = MveApp::new(move |cx: &mut NodeContext| {
            cx.child(crate::dashboard::dashboard(&h));
        });
        (mve, app)
    }

    #[derive(Clone, Copy, Debug)]
    struct BoxInfo {
        idx: usize,
        depth: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        padding: f32,
        click: bool,
    }

    fn collect(
        node: &Node,
        px: f32,
        py: f32,
        avail: (f32, f32),
        depth: usize,
        out: &mut Vec<BoxInfo>,
        idx: &mut usize,
    ) {
        if node.is_hidden() {
            return;
        }
        let my = *idx;
        *idx += 1;
        let ax = px + node.pos_x();
        let ay = py + node.pos_y();
        out.push(BoxInfo {
            idx: my,
            depth,
            x: ax,
            y: ay,
            w: avail.0,
            h: avail.1,
            padding: node.padding,
            click: node.on_click_fn.is_some(),
        });
        let children = children_nodes(node);
        if node.layout.is_some() {
            let boxes = layout_boxes(node, avail, &children);
            for (c, b) in children.iter().zip(boxes.iter()) {
                collect(
                    c,
                    ax + b.x,
                    ay + b.y,
                    (b.width, b.height),
                    depth + 1,
                    out,
                    idx,
                );
            }
        } else {
            for c in children.iter() {
                let own = node_size(c);
                collect(c, ax, ay, own, depth + 1, out, idx);
            }
        }
    }

    fn collect_click(
        nodes: &[Node],
        px: f32,
        py: f32,
        avail: (f32, f32),
        out: &mut Vec<(usize, f32, f32, Rc<dyn Fn(&mut PointerEvent)>)>,
    ) {
        for node in nodes {
            if node.is_hidden() {
                continue;
            }
            let ax = px + node.pos_x();
            let ay = py + node.pos_y();
            if let Some(f) = &node.on_click_fn {
                out.push((out.len(), ax + 3.0, ay + 3.0, f.clone()));
            }
            let children = children_nodes(node);
            if node.layout.is_some() {
                let boxes = layout_boxes(node, avail, &children);
                for (c, b) in children.iter().zip(boxes.iter()) {
                    collect_click(
                        std::slice::from_ref(c),
                        ax + b.x,
                        ay + b.y,
                        (b.width, b.height),
                        out,
                    );
                }
            } else {
                for c in children.iter() {
                    let own = node_size(c);
                    collect_click(std::slice::from_ref(c), ax, ay, own, out);
                }
            }
        }
    }

    #[test]
    fn dump_layout_and_click_audit() {
        let app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let mut out = Vec::new();
        let mut idx = 0;
        for n in app.root().nodes() {
            collect(&n, 0.0, 0.0, (800.0, 600.0), 0, &mut out, &mut idx);
        }

        println!("=== BOXES ({} total) ===", out.len());
        for b in &out {
            let tag = if b.padding > 0.5 { "  <-- CARD" } else { "" };
            println!(
                "[{}] d{} x={:.1} y={:.1} w={:.1} h={:.1} pad={:.1} click={}{}",
                b.idx, b.depth, b.x, b.y, b.w, b.h, b.padding, b.click, tag
            );
        }

        let mut clickables: Vec<(usize, f32, f32, Rc<dyn Fn(&mut PointerEvent)>)> = Vec::new();
        collect_click(
            &app.root().nodes(),
            0.0,
            0.0,
            (800.0, 600.0),
            &mut clickables,
        );

        println!("=== CLICK {} nodes ===", clickables.len());
        for (i, x, y, f) in &clickables {
            println!("click #{i} at ({x:.0},{y:.0})");
            let mut ev = PointerEvent::new(*x, *y);
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                f(&mut ev);
            }));
            assert!(r.is_ok(), "CLICK #{i} PANICKED");
        }

        let mut scene2 = Scene::new();
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            app.draw(&mut scene2, 800.0, 600.0);
        }));
        assert!(r.is_ok(), "REDRAW AFTER CLICK PANICKED");
    }

    /// 忠实复刻引擎的 `MouseInput::Pressed` 处理：hit_test → dispatch(onDown+onClick) → focus。
    /// 这是 `run_on_click` 直接调用所绕过的真实点击路径，用于定位"点击报错"的 panic。
    fn sim_hit_test(
        node: &Node,
        origin: (f32, f32),
        x: f32,
        y: f32,
        avail: (f32, f32),
    ) -> Option<Vec<(Node, (f32, f32))>> {
        if node.is_hidden() {
            return None;
        }
        let pos = (origin.0 + node.pos_x(), origin.1 + node.pos_y());
        let (lx, ly) = (x - pos.0, y - pos.1);
        let (w, h) = if avail.0 > 0.0 && avail.1 > 0.0 {
            avail
        } else {
            (node.effective_width(), node.effective_height())
        };
        // clip 守卫（与引擎 hit_test_node 一致）：盒外内容不可见也不可点
        if node.clip && !(w > 0.0 && h > 0.0 && lx >= 0.0 && ly >= 0.0 && lx <= w && ly <= h) {
            return None;
        }
        let children = children_nodes(node);
        let boxes = if node.layout.is_some() {
            layout_boxes(node, avail, &children)
        } else {
            vec![wy_mve::LayoutBox::ZERO; children.len()]
        };
        for (child, b) in children.iter().zip(boxes.iter()).rev() {
            let child_origin = (pos.0 + b.x, pos.1 + b.y);
            let child_avail = if node.layout.is_some() {
                (b.width, b.height)
            } else {
                node_size(child)
            };
            if let Some(mut chain) = sim_hit_test(child, child_origin, x, y, child_avail) {
                chain.push((node.clone(), pos));
                return Some(chain);
            }
        }
        let (lx, ly) = (x - pos.0, y - pos.1);
        let in_box = w > 0.0 && h > 0.0 && lx >= 0.0 && ly >= 0.0 && lx <= w && ly <= h;
        if has_handler(node) && in_box && node.run_hit_test(lx, ly) {
            return Some(vec![(node.clone(), pos)]);
        }
        None
    }

    fn sim_dispatch(chain: &[(Node, (f32, f32))], x: f32, y: f32) {
        let mut ev = PointerEvent::new(x, y);
        for (node, pos) in chain {
            ev.x = ev.root_x - pos.0;
            ev.y = ev.root_y - pos.1;
            node.run_on_down(&mut ev);
        }
        for (node, pos) in chain {
            ev.x = ev.root_x - pos.0;
            ev.y = ev.root_y - pos.1;
            node.run_on_click(&mut ev);
        }
    }

    fn sim_focus(chain: &[(Node, (f32, f32))]) {
        if let Some(idx) = chain.iter().position(|(n, _)| n.focusable) {
            chain[idx].0.run_focus();
        }
    }

    /// 用**真实** `handle_event`（`CursorMoved` + `MouseInput::Pressed`）扫遍整窗，
    /// 覆盖 headless 里被简化的 `sim_*` 漏掉的真实入口：`apply_focus`/`locate`/
    /// 文本框 `on_focus`/`on_down`。任何坐标 panic 都会被捕获并打出真实错误信息。
    fn real_press(app: &mut MveApp, x: f64, y: f64) {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        let cursor = WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let _ = app.handle_event(&cursor);
        let press = WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        };
        let _ = app.handle_event(&press);
        let release = WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
        };
        let _ = app.handle_event(&release);
    }

    fn panic_msg(r: Result<(), Box<dyn std::any::Any + Send>>) -> String {
        match r {
            Ok(()) => String::new(),
            Err(e) => match e.downcast_ref::<&str>() {
                Some(s) => (*s).to_string(),
                None => match e.downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => "non-string panic".to_string(),
                },
            },
        }
    }

    #[test]
    fn real_handle_event_click_sweep() {
        let mut app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let mut crashes: Vec<String> = Vec::new();
        // 扫遍整窗（步长 20），每个点走完整 CursorMoved→Pressed→Released
        for y in (0..600).step_by(20) {
            for x in (0..800).step_by(20) {
                let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    real_press(&mut app, x as f64, y as f64);
                }));
                if r.is_err() {
                    crashes.push(format!("click({},{}): {}", x, y, panic_msg(r)));
                }
            }
        }
        // 额外：专点输入框（表单第一个输入框大致区域 / 待办输入框）
        for (x, y) in [(343.0, 266.0), (637.0, 286.0), (343.0, 395.0)] {
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                real_press(&mut app, x, y);
            }));
            if r.is_err() {
                crashes.push(format!("input click({},{}): {}", x, y, panic_msg(r)));
            }
        }

        // 聚焦输入框后，复刻 runner 每帧的 IME 同步路径（about_to_wait）
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = real_press(&mut app, 343.0, 266.0);
            let _allowed = app.ime_allowed();
            let _area = app.ime_cursor_area();
        }));
        if r.is_err() {
            crashes.push(format!("ime after focus: {}", panic_msg(r)));
        }

        // 聚焦输入框后敲一个字符（走真实 key_fn → push_change → on_change）
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = real_press(&mut app, 343.0, 266.0);
            app.handle_key_event(&wy_engine::KeyEvent {
                key: wy_engine::Key::Char('a'),
                pressed: true,
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
            });
        }));
        if r.is_err() {
            crashes.push(format!("type after focus: {}", panic_msg(r)));
        }

        if !crashes.is_empty() {
            eprintln!("=== REAL CLICK CRASHES ===");
            for c in &crashes {
                eprintln!("{c}");
            }
        }
        assert!(
            crashes.is_empty(),
            "真实点击存在 panic:\n{}",
            crashes.join("\n")
        );
    }

    /// 复现真实交互序列（单点扫描覆盖不到）：切主题 → 填表 → 提交 → 增删待办 → 再切主题。
    /// 每一步都用真实 `handle_event`，任何 panic 都会被捕获并打印。
    #[test]
    fn real_interaction_sequence() {
        let mut app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let mut crashes: Vec<String> = Vec::new();
        let press = |app: &mut MveApp, x: f64, y: f64| real_press(app, x, y);
        let type_char = |app: &mut MveApp, c: char| {
            app.handle_key_event(&wy_engine::KeyEvent {
                key: wy_engine::Key::Char(c),
                pressed: true,
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
            });
        };
        let mut run = |app: &mut MveApp, label: &str, f: &dyn Fn(&mut MveApp)| {
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| f(app)));
            if let Err(e) = &r {
                let msg = match e.downcast_ref::<&str>() {
                    Some(s) => (*s).to_string(),
                    None => match e.downcast_ref::<String>() {
                        Some(s) => s.clone(),
                        None => "non-string panic".to_string(),
                    },
                };
                eprintln!("SEQ PANIC @ {label}: {msg}");
                crashes.push(label.into());
            }
        };

        // 1) 切主题
        run(&mut app, "theme-toggle-1", &|a| press(a, 104.0, 411.0));
        // 2) 点进表单第一个输入框并键入
        run(&mut app, "form-input-1", &|a| press(a, 343.0, 290.0));
        for c in ['J', 'o', 'h', 'n'] {
            let label = format!("type-name-{c}");
            run(&mut app, &label, &|a| type_char(a, c));
        }
        // 3) 提交
        run(&mut app, "submit", &|a| press(a, 268.0, 403.0));
        // 4) 待办输入 + 添加
        run(&mut app, "todo-input", &|a| press(a, 637.0, 244.0));
        for c in ['m', 'i', 'l', 'k'] {
            let label = format!("type-todo-{c}");
            run(&mut app, &label, &|a| type_char(a, c));
        }
        run(&mut app, "add-todo", &|a| press(a, 637.0, 244.0));
        // 5) 删除第一个待办（若有）
        run(&mut app, "del-todo", &|a| press(a, 708.0, 286.0));
        // 6) 再切主题 + 重绘
        run(&mut app, "theme-toggle-2", &|a| press(a, 104.0, 411.0));
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut s = Scene::new();
            app.draw(&mut s, 800.0, 600.0);
        }));
        if r.is_err() {
            crashes.push("redraw".into());
        }

        assert!(
            crashes.is_empty(),
            "交互序列存在 panic:\n{}",
            crashes.join("\n")
        );
    }

    /// 复现「resize 后点击」：真实 runner 在 `on_resize` 后每帧重绘，
    /// 若布局盒随视口变化而点击路径未同步会 panic。
    #[test]
    fn real_resize_then_click() {
        let mut app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let mut crashes: Vec<String> = Vec::new();
        // 先 resize 到更大窗口并重绘
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            app.on_resize(1024.0, 768.0);
            let mut s = Scene::new();
            app.draw(&mut s, 1024.0, 768.0);
        }));
        if r.is_err() {
            crashes.push(format!("resize+draw: {}", panic_msg(r)));
        }
        // 再扫一遍新视口内的点击
        for y in (0..768).step_by(40) {
            for x in (0..1024).step_by(40) {
                let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    real_press(&mut app, x as f64, y as f64);
                }));
                if r.is_err() {
                    crashes.push(format!("click({},{}): {}", x, y, panic_msg(r)));
                }
            }
        }
        assert!(
            crashes.is_empty(),
            "resize 后点击存在 panic:\n{}",
            crashes.join("\n")
        );
    }

    #[test]
    fn engine_click_simulation() {
        let app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        // 覆盖：主题按钮、提交、添加、三个删除、空白、卡片区、侧栏标题
        let coords: Vec<(f32, f32)> = vec![
            (59.0, 397.0),  // 侧栏主题切换
            (223.0, 432.0), // 表单提交
            (327.0, 432.0), // 表单重置
            (517.0, 272.0), // 待办添加
            (687.0, 320.0), // 待办删除#1
            (689.0, 368.0), // 待办删除#2
            (701.0, 416.0), // 待办删除#3
            (24.0, 69.4),   // 空白/侧栏标题区
            (400.0, 300.0), // 内容区空白
        ];
        for (x, y) in coords {
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let nodes = app.root().nodes();
                // 顶层倒序（与引擎 hit 顺序一致：后声明的浮层在上）
                if let Some(chain) = nodes
                    .iter()
                    .rev()
                    .find_map(|n| sim_hit_test(n, (0.0, 0.0), x, y, (800.0, 600.0)))
                {
                    sim_dispatch(&chain, x, y);
                    sim_focus(&chain);
                }
            }));
            assert!(r.is_ok(), "CLICK at ({x},{y}) PANICKED");
        }

        // 点击后整页重绘（引擎每帧都会做）
        let mut scene2 = Scene::new();
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            app.draw(&mut scene2, 800.0, 600.0);
        }));
        assert!(r.is_ok(), "REDRAW AFTER CLICK PANICKED");
    }

    // ===== 滚动容器 / 浮层（dialog/dropdown）端到端流程 =====

    /// 真实滚轮事件（先移光标再滚，滚轮命中用上一次光标位置）。
    fn real_wheel(app: &mut MveApp, x: f64, y: f64, dy: f64) -> bool {
        use winit::event::{MouseScrollDelta, WindowEvent};
        let _ = app.handle_event(&WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        });
        app.handle_event(&WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, dy as f32),
            phase: winit::event::TouchPhase::Moved,
        })
    }

    fn press_escape(app: &mut MveApp) {
        app.handle_key_event(&wy_engine::KeyEvent {
            key: wy_engine::Key::Escape,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        });
    }

    /// 按谓词找节点（含隐藏节点，浮层关闭时也要找得到），返回 (节点, 绝对盒)。
    fn walk_find(
        nodes: &[Node],
        px: f32,
        py: f32,
        avail: (f32, f32),
        pred: &dyn Fn(&Node) -> bool,
    ) -> Option<(Node, f32, f32, f32, f32)> {
        for node in nodes {
            let ax = px + node.pos_x();
            let ay = py + node.pos_y();
            if pred(node) {
                let w = if avail.0 > 0.0 {
                    avail.0
                } else {
                    node.effective_width()
                };
                let h = if avail.1 > 0.0 {
                    avail.1
                } else {
                    node.effective_height()
                };
                return Some((node.clone(), ax, ay, w, h));
            }
            let children = children_nodes(node);
            let mut found = None;
            if node.layout.is_some() {
                let boxes = layout_boxes(node, avail, &children);
                for (c, b) in children.iter().zip(boxes.iter()) {
                    if found.is_none() {
                        found = walk_find(
                            std::slice::from_ref(c),
                            ax + b.x,
                            ay + b.y,
                            (b.width, b.height),
                            pred,
                        );
                    }
                }
            } else {
                for c in children.iter() {
                    if found.is_none() {
                        let own = node_size(c);
                        found = walk_find(std::slice::from_ref(c), ax, ay, own, pred);
                    }
                }
            }
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// 往待办列表里加 n 项（直接写状态，造出可滚动的溢出内容）。
    ///
    /// 这里刻意**不走 UI**：本测试验的是滚动容器的行为，"新增待办"
    /// 那条交互链路另有专门的测试（`todo_add_flows_through_ui`）。
    fn add_todos(state: &Rc<crate::state::App>, n: usize) {
        let mut ids = state.todo_ids.get();
        let mut next = state.next_id.get();
        for _ in 0..n {
            ids.push(next);
            state.todo_store.done_of(next).set(false);
            state
                .todo_store
                .text_of(next)
                .set(String::from("批量新增项"));
            next += 1;
        }
        state.next_id.set(next);
        state.todo_ids.set(ids);
    }

    /// 待办「新增」走 UI：输入文本 → 点「添加」→ 列表增长一项。
    ///
    /// 验的是「结构信号 `todo_ids` 变化 → 列表 memo 重算 → 行重建」这条链路。
    #[test]
    fn todo_add_flows_through_ui() {
        let (mut app, state) = build_with_state();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let before = state.todo_ids.get().len();

        // 按 tag 精确定位控件（不依赖几何特征——布局一改坐标就失效，
        // 这是这类 UI 测试最常见的假失败来源）。
        let nodes = app.root().nodes();
        let (bx, by) = find_tag(&nodes, 0.0, 0.0, (800.0, 600.0), "todo-add")
            .map(|(x, y)| (x + 10.0, y + 10.0))
            .expect("应能找到「添加」按钮");
        let (input_x, input_y) = find_tag(&nodes, 0.0, 0.0, (800.0, 600.0), "input-todo")
            .map(|(x, y)| (x + 10.0, y + 10.0))
            .expect("应能找到待办输入框");

        // 点输入框拿到焦点
        real_press(&mut app, input_x as f64, input_y as f64);
        assert!(!app.focus_path().is_empty(), "点击输入框后焦点应落在它上面");
        // 输入文字
        for ch in ['n', 'e', 'w'] {
            app.handle_key_event(&wy_engine::KeyEvent {
                key: wy_engine::Key::Char(ch),
                pressed: true,
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
            });
        }
        // 探测：焦点是否落在输入框
        eprintln!("focus path len = {}", app.focus_path().len());
        // 探测：输入后 signal 里有没有字
        eprintln!("focus path len = {}", app.focus_path().len());
        // 点「添加」
        real_press(&mut app, bx as f64, by as f64);
        app.draw(&mut scene, 800.0, 600.0);
        assert!(
            state.todo_ids.get().len() > before,
            "点添加后待办数应增加：{} -> {}",
            before,
            state.todo_ids.get().len()
        );
    }

    /// 在**指定子树**内查找节点（按文档序，返回第一个匹配的）。
    ///
    /// 页面里有多个同类控件时（如表单卡与待办卡都有输入框），
    /// 光按"特征"找会抓到别处去——必须限定在目标子树内。
    fn find_tag(
        nodes: &[Node],
        px: f32,
        py: f32,
        avail: (f32, f32),
        tag: &str,
    ) -> Option<(f32, f32)> {
        for node in nodes {
            let ax = px + node.pos_x();
            let ay = py + node.pos_y();
            if node.tag == Some(tag) {
                return Some((ax, ay));
            }
            let children = children_nodes(node);
            if node.layout.is_some() {
                let boxes = layout_boxes(node, avail, &children);
                for (c, b) in children.iter().zip(boxes.iter()) {
                    if let Some(found) = find_tag(
                        std::slice::from_ref(c),
                        ax + b.x,
                        ay + b.y,
                        (b.width, b.height),
                        tag,
                    ) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    fn walk_find_in(
        nodes: &[Node],
        px: f32,
        py: f32,
        avail: (f32, f32),
        pred: &dyn Fn(&Node) -> bool,
    ) -> Option<(Node, f32, f32, f32, f32)> {
        for node in nodes {
            let ax = px + node.pos_x();
            let ay = py + node.pos_y();
            if pred(node) {
                let w = if avail.0 > 0.0 {
                    avail.0
                } else {
                    node.effective_width()
                };
                let h = if avail.1 > 0.0 {
                    avail.1
                } else {
                    node.effective_height()
                };
                return Some((node.clone(), ax, ay, w, h));
            }
            let children = children_nodes(node);
            if node.layout.is_some() {
                let boxes = layout_boxes(node, avail, &children);
                for (c, b) in children.iter().zip(boxes.iter()) {
                    if let Some(found) = walk_find_in(
                        std::slice::from_ref(c),
                        ax + b.x,
                        ay + b.y,
                        (b.width, b.height),
                        pred,
                    ) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// 图标必须是**路径图元**而非文本图元。
    ///
    /// 回归：「shadcn 里很多方块」——早先图标用 `✕ ✓ ▾` 等**字符**绘制，
    /// 字体缺该字形时渲染成空心方块。改矢量路径后与字体无关。
    #[test]
    fn icons_are_vector_paths_not_text() {
        let app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let paths = scene
            .iter()
            .filter(|p| matches!(p, wy_render::Primitive::Path { .. }))
            .count();
        assert!(paths > 0, "场景里应有矢量路径图元（图标）");

        // 关键：不应有任何"图标字符"文本——检查常见的图标字形
        let icon_chars = ["✕", "✓", "▾", "▲", "▼", "≡", "⋯"];
        for prim in scene.iter() {
            if let wy_render::Primitive::Text { text, .. } = prim {
                for c in icon_chars {
                    assert!(
                        !text.contains(c),
                        "不应出现图标字符 `{c}`（应改用矢量路径）"
                    );
                }
            }
        }
    }

    /// 滚动容器：滚轮驱动内容偏移（滚动条滑块下移），视口外滚轮无效。
    #[test]
    fn scroll_wheel_moves_thumb_and_content() {
        let (mut app, state) = build_with_state();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        // 先加够待办，让列表**溢出**视口（内容不足时滚轮不该被消费——
        // 那正是正确行为：没有可滚的内容就不消费滚轮）。
        add_todos(&state, 12);
        app.draw(&mut scene, 800.0, 600.0);
        eprintln!("todo count after add = {}", state.todo_ids.get().len());
        eprintln!("visible = {:?}", state.visible_todo_ids().len());
        eprintln!("ids = {:?}", state.todo_ids.get());
        eprintln!("filter = {}", state.todo_filter.get());
        for id in state.todo_ids.get() {
            eprintln!("  id={id} done={}", state.todo_store.done_get(id));
        }

        let nodes = app.root().nodes();
        let (scroll, sx, sy, sw, sh) =
            walk_find(&nodes, 0.0, 0.0, (800.0, 600.0), &|n| n.clip).expect("应能找到滚动容器");
        assert!(sw > 0.0 && sh > 0.0);
        let thumb = |app: &MveApp| -> (f32, f32) {
            let nodes = app.root().nodes();
            walk_find(&nodes, 0.0, 0.0, (800.0, 600.0), &|n| {
                n.not_in_layout && n.width < 5.0
            })
            .map(|(_, x, y, _, _)| (x, y))
            .expect("应能找到滚动条滑块")
        };
        let _ = scroll;
        let (tx0, ty0) = thumb(&app);
        assert!(
            (ty0 - (sy + 8.0)).abs() < 1.0,
            "未滚动时滑块应贴轨道顶：{ty0} vs {}",
            sy + 8.0
        );

        // 探测：scroll 视口与内容的实际高度
        {
            let nodes = app.root().nodes();
            let (vp, _, _, vw, vh) =
                walk_find(&nodes, 0.0, 0.0, (800.0, 600.0), &|n| n.clip).expect("scroll");
            eprintln!("scroll viewport = {vw}x{vh} at ({sx},{sy})");
            let content = children_nodes(&vp);
            eprintln!("scroll children = {}", content.len());
            for c in content.iter() {
                eprintln!("  child size = {:?}", node_size(c));
                let gc = children_nodes(c);
                eprintln!("    grand children = {}", gc.len());
                for g in gc.iter().take(4) {
                    eprintln!(
                        "      g size = {:?} layout={:?}",
                        node_size(g),
                        g.layout.is_some()
                    );
                }
            }
        }

        // 视口内滚动 3 格：滑块下移
        let cx = sx + sw / 2.0;
        let cy = sy + sh - 10.0;
        assert!(
            real_wheel(&mut app, cx as f64, cy as f64, 3.0),
            "滚动容器应消费视口内滚轮"
        );
        app.draw(&mut scene, 800.0, 600.0);
        let (tx1, ty1) = thumb(&app);
        assert!((tx1 - tx0).abs() < 0.5, "滑块 x 不应变化");
        assert!(ty1 > ty0 + 5.0, "滚动后滑块应下移：{ty0} → {ty1}");

        // 视口外的滚轮（卡片下方）：命中不到滚动容器，不滚动
        let ty2 = ty1;
        assert!(!real_wheel(
            &mut app,
            cx as f64,
            (sy + sh + 40.0) as f64,
            3.0
        ));
        app.draw(&mut scene, 800.0, 600.0);
        let (_, ty3) = thumb(&app);
        assert!((ty3 - ty2).abs() < 0.5, "视口外滚轮不应滚动");
    }

    /// 模态对话框：点击"打开对话框"打开 → Esc 关 → 点遮罩关 → 点面板不关。
    #[test]
    fn dialog_open_esc_scrim_and_panel_flow() {
        let mut app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let nodes = app.root().nodes();
        let (dialog, _, _, _, _) = walk_find(&nodes, 0.0, 0.0, (800.0, 600.0), &|n| {
            n.not_in_layout && n.key_fn.is_some() && n.focus_trap
        })
        .expect("应能找到模态对话框浮层根");
        assert!(dialog.is_hidden(), "初始应关闭");

        // 逐个点击可点击节点，直到对话框打开（即找到"打开对话框"按钮）
        let mut clickables: Vec<(usize, f32, f32, Rc<dyn Fn(&mut PointerEvent)>)> = Vec::new();
        collect_click(
            &app.root().nodes(),
            0.0,
            0.0,
            (800.0, 600.0),
            &mut clickables,
        );
        let mut open_at = None;
        for (_i, x, y, _f) in &clickables {
            real_press(&mut app, *x as f64, *y as f64);
            if !dialog.is_hidden() {
                open_at = Some((*x, *y));
                break;
            }
            if !dialog.is_hidden() {
                unreachable!();
            }
        }
        let (bx, by) = open_at.expect("应有点击能打开对话框");
        assert!(!dialog.is_hidden());

        // Esc 关闭
        press_escape(&mut app);
        assert!(dialog.is_hidden(), "Esc 应关闭对话框");

        // 再打开，点遮罩（窗口顶部空白）关闭
        real_press(&mut app, bx as f64, by as f64);
        assert!(!dialog.is_hidden());
        real_press(&mut app, 400.0, 30.0);
        assert!(dialog.is_hidden(), "点击遮罩应关闭");

        // 再打开，点面板内容（不冒泡到遮罩）→ 保持打开
        real_press(&mut app, bx as f64, by as f64);
        assert!(!dialog.is_hidden());
        real_press(&mut app, 400.0, 300.0);
        assert!(!dialog.is_hidden(), "点击面板不应关闭");
        app.draw(&mut scene, 800.0, 600.0);

        // Esc 收尾
        press_escape(&mut app);
        assert!(dialog.is_hidden());
    }

    /// 下拉菜单：触发器打开（面板锚定按钮下方）→ 点菜单项关闭 → 点外部 / Esc 关闭。
    #[test]
    fn dropdown_anchor_item_and_dismiss_flow() {
        let mut app = build();
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        let nodes = app.root().nodes();
        let (dropdown, _, _, _, _) = walk_find(&nodes, 0.0, 0.0, (800.0, 600.0), &|n| {
            n.not_in_layout && n.key_fn.is_some() && !n.focus_trap
        })
        .expect("应能找到下拉浮层根");
        assert!(dropdown.is_hidden(), "初始应关闭");

        let mut clickables: Vec<(usize, f32, f32, Rc<dyn Fn(&mut PointerEvent)>)> = Vec::new();
        collect_click(
            &app.root().nodes(),
            0.0,
            0.0,
            (800.0, 600.0),
            &mut clickables,
        );
        let mut trigger_at = None;
        for (_i, x, y, _f) in &clickables {
            real_press(&mut app, *x as f64, *y as f64);
            if !dropdown.is_hidden() {
                trigger_at = Some((*x, *y));
                break;
            }
            // 途中若误开了对话框（对话框按钮在触发器之前），Esc 关掉继续找
            if let Some((d, _, _, _, _)) =
                walk_find(&app.root().nodes(), 0.0, 0.0, (800.0, 600.0), &|n| {
                    n.not_in_layout && n.key_fn.is_some() && n.focus_trap
                })
            {
                if !d.is_hidden() {
                    press_escape(&mut app);
                }
            }
        }
        let (tx, ty) = trigger_at.expect("应有点击能打开下拉菜单");
        assert!(!dropdown.is_hidden());

        // 面板锚定在按钮（左上角 = 点击坐标 - 3px 局部偏移）下方 38px 处；
        // 第一个菜单项中心 ≈ 面板 padding 8 + 项宽 134/2、y + 8 + 15
        let (ax, ay) = (tx - 3.0, ty - 3.0);
        let item1 = (ax + 8.0 + 67.0, ay + 38.0 + 8.0 + 15.0);
        real_press(&mut app, item1.0 as f64, item1.1 as f64);
        assert!(dropdown.is_hidden(), "点击菜单项应关闭菜单");

        // 再打开，点外部（远离面板）关闭
        real_press(&mut app, tx as f64, ty as f64);
        assert!(!dropdown.is_hidden());
        real_press(&mut app, 700.0, 560.0);
        assert!(dropdown.is_hidden(), "点击外部应关闭");

        // 再打开，Esc 关闭
        real_press(&mut app, tx as f64, ty as f64);
        assert!(!dropdown.is_hidden());
        press_escape(&mut app);
        assert!(dropdown.is_hidden(), "Esc 应关闭菜单");
        app.draw(&mut scene, 800.0, 600.0);
    }

    /// 响应式专项：窗口放大后布局必须跟随（grow/Stretch 重排），
    /// 卡片行等宽等高、间隔一致——"窗口一放大布局不跟随"的回归测试。
    #[test]
    fn responsive_layout_follows_window() {
        let mut app = build();
        let mut scene = Scene::new();

        let mut layout_at = |app: &mut MveApp, w: f32, h: f32| -> Vec<BoxInfo> {
            app.draw(&mut scene, w, h);
            let mut out: Vec<BoxInfo> = Vec::new();
            let mut idx = 0;
            let nodes = app.root().nodes();
            for n in &nodes {
                collect(n, 0.0, 0.0, (w, h), 0, &mut out, &mut idx);
            }
            out
        };

        let at_800 = layout_at(&mut app, 800.0, 600.0);
        // 根节点铺满窗口（不再写死 800×600，而是 flex 分配）
        let root = at_800.first().expect("根节点存在");
        assert!(
            (root.w - 800.0).abs() < 0.5 && (root.h - 600.0).abs() < 0.5,
            "根节点应铺满 800×600，实际 {}×{}",
            root.w,
            root.h
        );

        // 卡片（padding=16）分类：3 张统计卡 + 表单卡 + 待办卡
        let cards: Vec<BoxInfo> = at_800
            .iter()
            .copied()
            .filter(|b| (b.padding - 16.0).abs() < 0.1)
            .collect();
        assert_eq!(cards.len(), 5, "应有 5 张卡片，实际 {:?}", cards);
        // 统计卡：行内三张等宽（grow 均分）
        let stat_ws: Vec<f32> = cards.iter().take(3).map(|b| b.w).collect();
        assert!(
            (stat_ws[0] - stat_ws[1]).abs() < 0.5 && (stat_ws[1] - stat_ws[2]).abs() < 0.5,
            "统计卡应等宽（grow 均分），实际 {stat_ws:?}"
        );
        assert!((stat_ws[0] - stat_ws[1]).abs() < 0.5);
        // 表单/待办行：等宽等高、同一行、间隔 = 16
        let (form, todos) = (cards[3], cards[4]);
        assert!(
            (form.w - todos.w).abs() < 0.5,
            "表单/待办卡应等宽，实际 {} vs {}",
            form.w,
            todos.w
        );
        assert!(
            (form.h - todos.h).abs() < 0.5,
            "表单/待办卡应等高（Stretch），实际 {} vs {}",
            form.h,
            todos.h
        );
        assert!((form.y - todos.y).abs() < 0.5, "表单/待办卡应同一行");
        assert!(
            ((todos.x - form.x - form.w) - 16.0).abs() < 0.5,
            "行内间隔应为 16"
        );
        // 行高填充：卡片高度远大于内容自然高度（grow 吃掉窗口剩余高度）
        assert!(form.h > 300.0, "卡片应填满剩余窗口高度，实际 h={}", form.h);

        // 放大窗口 → 卡片跟着变大（布局跟随）
        let at_1200 = layout_at(&mut app, 1200.0, 800.0);
        let cards2: Vec<BoxInfo> = at_1200
            .iter()
            .copied()
            .filter(|b| (b.padding - 16.0).abs() < 0.1)
            .collect();
        assert_eq!(cards2.len(), 5);
        let (form2, todos2) = (cards2[3], cards2[4]);
        assert!(
            form2.w > form.w + 50.0,
            "放大后表单卡应变宽：{} -> {}",
            form.w,
            form2.w
        );
        assert!(
            form2.h > form.h + 50.0,
            "放大后表单卡应变高：{} -> {}",
            form.h,
            form2.h
        );
        assert!(
            (form2.w - todos2.w).abs() < 0.5 && (form2.h - todos2.h).abs() < 0.5,
            "放大后仍应等宽等高"
        );
        let stat2 = &cards2[0];
        assert!(
            stat2.w > stat_ws[0] + 20.0,
            "放大后统计卡应变宽：{} -> {}",
            stat_ws[0],
            stat2.w
        );
        let root2 = at_1200.first().unwrap();
        assert!(
            (root2.w - 1200.0).abs() < 0.5 && (root2.h - 800.0).abs() < 0.5,
            "根节点应铺满 1200×800，实际 {}×{}",
            root2.w,
            root2.h
        );
    }
}
