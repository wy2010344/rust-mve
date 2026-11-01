//! shadcn 仪表盘——**canvasRender2（纯 Node2）**版本。
//!
//! 整棵树里没有一个 v1 `Node`：容器是 `node2!`、组件是 `#[rec]` + `!` 宏、
//! 输入框是 `editable_text_node`、滚动是 `create_scroll` 三段拼装、浮层是
//! [`wy_mve::canvas_render2::dialog`] / [`dropdown`](wy_mve::canvas_render2::dropdown)。
//! 入口走 `Node2App`（`build_root_node2`），**不经过** v1 `Node` 的 `to_node`
//! 桥接，也没有 `#[component]` / `rsx!`。
//!
//! # 结构
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`widgets`] | 基础组件（**一组件一模块**）：card / badge / button / text_label / separator / toggle / checkbox / avatar / skeleton / tabs |
//! | [`stats`] | 统计卡（`#[rec]` 组件） |
//! | [`sidebar`] | 侧栏：品牌、导航、深色开关、浮层触发器 |
//! | [`form`] / [`todos`] | 联系表单 / 待办清单（`for_each_node` 增量列表 + 滚动） |
//! | [`dashboard`] | 页面组装 + 浮层（dialog / dropdown） |
//!
//! # `#[rec]` 的路径约束（下面那串 `pub use` 就是它）
//!
//! `#[rec]` 生成的 `!` 宏经 `$crate::XxxArg` 引用产物，而 `$crate` 是**导出
//! 该宏的 crate 根**，指向不了定义所在的子模块。所以组件定义在子模块时，
//! 必须把 `XxxArg` 再导出到 crate 根。
//!
//! 运行：`cargo run -p wy-app --example shadcn`
//! 出图：`cargo run -p wy-app --example shadcn -- --snapshot docs/shot-shadcn.png --snapshot-size 1100x760`

// `#[macro_use]` 的可见性按声明顺序：提供宏的模块必须排在消费者之前
// （dashboard / form / sidebar / todos 都会用到 widgets / stats 的 `!` 宏）。
#[macro_use]
mod widgets;
#[macro_use]
mod stats;

mod dashboard;
mod form;
mod sidebar;
mod state;
mod todos;
mod tokens;

// `#[rec]` 的 `!` 宏产物必须能在 crate 根被找到（见模块文档）。
// form / sidebar / todos / dashboard 里只有普通函数，不需要再导出。
#[doc(hidden)]
pub use stats::StatCardArg;
#[doc(hidden)]
pub use widgets::{
    AvatarArg, BadgeArg, ButtonArg, ButtonSizedArg, CardArg, CheckboxArg, SeparatorArg,
    SkeletonArg, TabsArg, TextLabelArg, ToggleArg,
};

use std::cell::Cell;
use std::rc::Rc;

use wy_engine::node2_app::Node2App;
use wy_engine::runner::run;
use wy_mve::canvas_render2::build_root_node2;
use wy_mve::canvas_render2::node2::{always, get};
use wy_mve::Node2Props;
use wy_signal::SetValue;

/// 初始窗口尺寸（截图与实跑同一套）。
const WINDOW: (f32, f32) = (1100.0, 760.0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = state::App::new();
    let args: Vec<String> = std::env::args().collect();
    // 深色主题验证用：`--dark` 把初始主题切到深色
    if args.iter().any(|a| a == "--dark") {
        app.theme_dark.set(true);
    }
    // 浮层验证用：`--dialog` / `--menu` 让浮层开局就是打开的（截图用）
    if args.iter().any(|a| a == "--dialog") {
        app.dialog_open.set(true);
    }
    if args.iter().any(|a| a == "--menu") {
        // 锚点取「更多菜单」触发器在窗口里的矩形（真实运行时由 `mouse_click`
        // 用 `global - local` 现算，这里给同一个位置以便截图）
        app.menu_anchor.set((24.0, 700.0, 160.0, 36.0));
        app.menu_open.set(true);
    }
    let app = Rc::new(app);

    // `--snapshot <路径>`：离屏出图后退出（不开窗口）
    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        let mve = make_app(Rc::clone(&app), w as f32, h as f32);
        wy_app::snapshot::save_snapshot(&mve, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    run(make_app(app, WINDOW.0, WINDOW.1))
}

/// 画布根 + 页面 + 浮层，包成 [`Node2App`]。
///
/// 浮层在这里挂（而不是在页面里挂）：它们的可视区取父节点的 inner，只有直接
/// 挂在画布根下才等于整窗。文档序最后 → 绘制在最上层。
fn make_app(app: Rc<state::App>, w: f32, h: f32) -> Node2App {
    Node2App::new(move |viewport| {
        let page_vp = viewport.clone();
        let app_page = Rc::clone(&app);
        let app_overlays = Rc::clone(&app);
        build_root_node2(root_arg(viewport), move |cx| {
            cx.child(dashboard::page(&app_page, page_vp));
            for overlay in dashboard::overlays(&app_overlays) {
                cx.child(overlay);
            }
        })
    })
    .with_size(w, h)
    .with_title("wy-ui · shadcn · canvasRender2")
}

/// 画布根：原点 `(0,0)`、尺寸等于视口（TS 画布根同款）。
///
/// 根没有父节点，`defaultPosition` 无从推导，所以 `x`/`y` 必须显式给 0。
fn root_arg(viewport: Rc<Cell<(f32, f32)>>) -> Node2Props {
    let (vw, vh) = (viewport.clone(), viewport);
    Node2Props {
        x: Some(always(0.0)),
        y: Some(always(0.0)),
        width: Some(get(move || vw.get().0)),
        height: Some(get(move || vh.get().1)),
        ..Node2Props::default()
    }
}

// ============================================================================
// 端到端断言（纯 Node2：建树 → 读几何 → 派发消息）
// ============================================================================
//
// 旧 `shadcn/main.rs` 的 8 条断言全部建立在 v1 `Node` 的 `on_click_fn` /
// `last_box` / `walk_find` 上——那套东西随 v1 一起退役。这里换成 v2 的等价手段：
// `Node2::width()/height()/x()/y()`（pull 式实时几何）、`raw_children()`（不过滤
// hide 的遍历）、`Node2::mouse_click()`（节点消息，不经引擎）。
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use wy_mve::canvas_render2::protocol::{create_mouse_event2, MouseEventInit2, MouseType2};
    use wy_mve::canvas_render2::tree::geometry::clear_layout_memo;
    use wy_mve::Node2;
    use wy_mve::{flex, node2};
    use wy_signal::GetValue;

    const VW: f32 = 1100.0;
    const VH: f32 = 760.0;

    /// 建整棵树（页面 + 两个浮层）并清掉布局 memo。
    fn build() -> (Rc<state::App>, Node2) {
        let app = Rc::new(state::App::new());
        let viewport = Rc::new(Cell::new((VW, VH)));
        let page_vp = Rc::clone(&viewport);
        let app_page = Rc::clone(&app);
        let app_overlays = Rc::clone(&app);
        let root = build_root_node2(root_arg(viewport), move |cx| {
            cx.child(dashboard::page(&app_page, page_vp));
            for overlay in dashboard::overlays(&app_overlays) {
                cx.child(overlay);
            }
        });
        // 一帧的开头（`Node2App::draw` 里同样做）
        clear_layout_memo();
        (app, root)
    }

    /// 按 `debug_name` 找节点（用 `raw_children` 以便找到隐藏的浮层）。
    fn by_name(node: &Node2, name: &str) -> Option<Node2> {
        if node.debug_name().as_deref() == Some(name) {
            return Some(node.clone());
        }
        node.raw_children()
            .into_iter()
            .find_map(|c| by_name(&c, name))
    }

    fn named(root: &Node2, name: &str) -> Node2 {
        by_name(root, name).unwrap_or_else(|| panic!("未找到节点 {name}"))
    }

    // ---- 文本输入：点进去 → 打字 → 回写信号（端到端，含指针路由与键盘路由）----
    #[test]
    fn clicking_the_input_then_typing_writes_the_signal() {
        use winit::dpi::PhysicalPosition;
        use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
        use wy_engine::runner::WyApp;

        let app = Rc::new(state::App::new());
        let mut app2 = make_app(Rc::clone(&app), VW, VH);
        let root = app2.root().clone();
        let outer = named(&root, "新的待办...");
        let editor = outer.raw_children()[0].clone();
        assert!(
            editor.focusable(),
            "编辑器必须可聚焦——不可聚焦就点不进去，键盘/IME 永远收不到"
        );

        let (cx, cy) = (
            editor.abs_x() + editor.width() / 2.0,
            editor.abs_y() + editor.height() / 2.0,
        );
        assert!(
            cx > 0.0 && cy > 0.0,
            "编辑器应有真实位置，实际 ({cx}, {cy})"
        );

        // ① 指针移入 + 按下 + 抬起：路由把焦点给命中链里最近的可聚焦节点
        app2.handle_event(&WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(f64::from(cx), f64::from(cy)),
        });
        for state in [ElementState::Pressed, ElementState::Released] {
            app2.handle_event(&WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
            });
        }
        assert!(
            wy_mve::canvas_render2::focus::active_editor().is_some(),
            "点进输入框后应成为活跃编辑器"
        );

        // ② 键入：`Node2App` → `route_key_event` → 活跃编辑器 → `on_text_change`
        app2.handle_key_event(&wy_engine::KeyEvent::new(wy_engine::Key::Char('a'), true));
        assert_eq!(app.todo_input.get(), "a", "字符应写回信号");

        // ③ 退格
        app2.handle_key_event(&wy_engine::KeyEvent::new(wy_engine::Key::Backspace, true));
        assert_eq!(app.todo_input.get(), "", "退格应删掉刚输入的字符");
    }

    /// 点输入框的**左内边距**也要能聚焦。
    ///
    /// 命中测试只认节点自己的矩形：内边距若留在外层容器上，靠边那 10px 属于
    /// "容器空白"，命中链里没有可聚焦节点 → 焦点被清空。用户感知就是
    /// "点输入框边缘，看不到光标"。内边距归编辑器后整块矩形都属于它。
    #[test]
    fn clicking_the_input_padding_still_focuses_the_editor() {
        use winit::dpi::PhysicalPosition;
        use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
        use wy_engine::runner::WyApp;

        let app = Rc::new(state::App::new());
        let mut app2 = make_app(Rc::clone(&app), VW, VH);
        let root = app2.root().clone();
        let outer = named(&root, "新的待办...");
        let editor = outer.raw_children()[0].clone();

        // 贴着**外框**左边缘 2px：内边距留在外层时，这 2px 属于"容器空白"，
        // 命中链里没有可聚焦节点，焦点会被清掉。
        let (cx, cy) = (outer.abs_x() + 2.0, outer.abs_y() + outer.height() / 2.0);
        app2.handle_event(&WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(f64::from(cx), f64::from(cy)),
        });
        for state in [ElementState::Pressed, ElementState::Released] {
            app2.handle_event(&WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
            });
        }
        assert!(
            wy_mve::canvas_render2::focus::is_focused(&editor),
            "点左内边距也应把焦点给编辑器"
        );
        assert!(
            wy_mve::canvas_render2::focus::active_editor().is_some(),
            "聚焦后应成为活跃编辑器（键盘 / IME 的路由目标）"
        );
    }

    /// 点击一个节点（节点消息直接派发，不经命中链）。
    fn click(node: &Node2) {
        let mut ev = create_mouse_event2(MouseEventInit2 {
            type_: MouseType2::Click,
            x: 1.0,
            y: 1.0,
            global_x: 1.0,
            global_y: 1.0,
            device: None,
            pointer_id: None,
            button: None,
            buttons: None,
            wheel_delta_x: None,
            wheel_delta_y: None,
            pressure: None,
        });
        node.mouse_click(&mut ev);
    }

    #[test]
    fn page_fills_the_viewport() {
        let (_, root) = build();
        let page = named(&root, "page");
        assert_eq!((page.width(), page.height()), (VW, VH), "页面应铺满视口");
    }

    #[test]
    fn sidebar_keeps_its_fixed_width() {
        let (_, root) = build();
        let sidebar = named(&root, "sidebar");
        assert_eq!(sidebar.width(), 160.0, "侧栏固定宽");
        // 侧栏是主体行的 Stretch 子项：高度 = 父给的那一档，不是窗口高
        let main_row = named(&root, "page").raw_children()[1].clone();
        assert_eq!(sidebar.height(), main_row.height(), "侧栏与主体行等高");
    }

    #[test]
    fn stat_cards_split_the_row_evenly() {
        let (_, root) = build();
        let page = named(&root, "page");
        // 页面 → 主体行 → 右列 → 统计行
        let main_row = page.raw_children()[1].clone();
        let right_col = main_row.raw_children()[1].clone();
        let stats_row = right_col.raw_children()[0].clone();

        let cards: Vec<Node2> = stats_row.raw_children();
        assert_eq!(cards.len(), 3, "统计行应有 3 张卡");
        let widths: Vec<f32> = cards.iter().map(Node2::width).collect();
        let expected = (stats_row.width() - 2.0 * 16.0) / 3.0;
        for (i, w) in widths.iter().enumerate() {
            assert!(
                (w - expected).abs() < 0.5,
                "第 {i} 张卡宽 {w} 应约等于 {expected}（等分 + gap 16）"
            );
        }
        let heights: Vec<f32> = cards.iter().map(Node2::height).collect();
        assert!(
            heights[0] > 40.0,
            "卡片高度应由内容撑开，实际 {}",
            heights[0]
        );
        assert_eq!(heights[0], heights[1], "同行卡片等高");
        assert_eq!(heights[1], heights[2], "同行卡片等高");
    }

    #[test]
    fn body_cards_fill_the_remaining_height() {
        let (_, root) = build();
        let page = named(&root, "page");
        let main_row = page.raw_children()[1].clone();
        let right_col = main_row.raw_children()[1].clone();
        let stats_row = right_col.raw_children()[0].clone();
        let body_row = right_col.raw_children()[1].clone();

        assert!(
            (body_row.height() - (right_col.height() - stats_row.height() - 16.0)).abs() < 0.5,
            "表单/待办行应吃掉统计行之外的剩余高度"
        );
        let left = body_row.raw_children()[0].width();
        let right = body_row.raw_children()[1].width();
        assert!(
            (left - right).abs() < 0.5,
            "两卡等宽，实际 {left} / {right}"
        );
    }

    #[test]
    fn todo_input_fills_the_row_next_to_the_add_button() {
        let (_, root) = build();
        let page = named(&root, "page");
        let main_row = page.raw_children()[1].clone();
        let body_row = main_row.raw_children()[1].clone().raw_children()[1].clone();
        let card = body_row.raw_children()[1].clone();

        // 输入框直接是「输入 + 添加」这一行的主轴子项，靠 `grow` 吃剩余宽度：
        // 卡片宽 - 两侧内边距 - 按钮 56 - gap 8。曾经在外面多包一层带 grow 的
        // 容器，内层拿不到宽度 → 整个框 0 宽（连边框都画不出来）。
        let input = named(&root, "新的待办...");
        let expected = card.width() - 2.0 * 16.0 - 56.0 - 8.0;
        assert!(
            (input.width() - expected).abs() < 0.5,
            "待办输入框宽 {} 应约等于 {}",
            input.width(),
            expected
        );
    }

    #[test]
    fn dialog_is_hidden_until_opened() {
        let (app, root) = build();
        // 隐藏的浮层不进 `children()`（不绘制、不命中、不进焦点链）
        assert_eq!(root.children().len(), 1, "初始只有页面参与");
        let dialog = named(&root, "dialog");
        assert!(dialog.hide(), "对话框初始应关闭");

        app.dialog_open.set(true);
        assert!(!dialog.hide());
        assert_eq!(root.children().len(), 2, "打开后浮层进入文档序");
    }

    #[test]
    fn dialog_panel_is_centered_and_scrim_covers_the_window() {
        let (app, root) = build();
        app.dialog_open.set(true);
        let dialog = named(&root, "dialog");
        assert_eq!(
            (dialog.width(), dialog.height()),
            (VW, VH),
            "遮罩应铺满父 inner（整窗）"
        );
        let panel = named(&root, "dialog-panel");
        let (pw, ph) = (panel.width(), panel.height());
        assert_eq!(pw, 360.0, "面板宽度由参数给定");
        assert!(ph > 0.0, "面板高度应由内容撑开");
        assert!((panel.x() - (VW - pw) / 2.0).abs() < 0.5, "水平居中");
        assert!((panel.y() - (VH - ph) / 2.0).abs() < 0.5, "垂直居中");
    }

    #[test]
    fn dropdown_opens_below_the_anchor_when_there_is_room() {
        let (app, root) = build();
        // 锚点在上半屏：下方够 → 向下展开
        app.menu_anchor.set((40.0, 120.0, 160.0, 34.0));
        app.menu_open.set(true);
        let panel = named(&root, "dropdown-panel");
        assert_eq!(panel.y(), 120.0 + 34.0 + 6.0, "贴着锚点下方 + gap");
        assert_eq!(panel.x(), 40.0, "起始边对齐");
    }

    #[test]
    fn dropdown_flips_up_when_the_anchor_is_near_the_bottom() {
        let (app, root) = build();
        app.menu_anchor.set((40.0, VH - 40.0, 160.0, 34.0));
        app.menu_open.set(true);
        let panel = named(&root, "dropdown-panel");
        let ph = panel.height();
        assert!(ph > 0.0);
        assert!(
            panel.y() + ph <= VH - 40.0,
            "下方放不下时应整体翻到锚点上方，实际 y={}",
            panel.y()
        );
    }

    #[test]
    fn menu_item_click_closes_the_menu_and_writes_status() {
        let (app, root) = build();
        app.menu_anchor.set((40.0, 100.0, 160.0, 34.0));
        app.menu_open.set(true);
        let panel = named(&root, "dropdown-panel");
        let items = panel.raw_children();
        assert_eq!(items.len(), 3, "三个菜单项");

        click(&items[1]);
        assert!(!app.menu_open.get(), "点击菜单项应关闭菜单");
        assert_eq!(app.status.get(), "已选择：导出数据", "状态栏回显所选项");
    }

    #[test]
    fn clicking_the_scrim_closes_the_dialog() {
        let (app, root) = build();
        app.dialog_open.set(true);
        let dialog = named(&root, "dialog");
        // 点空白处（遮罩）→ 关闭；点面板本身由面板 `stop_propagation` 拦住
        click(&dialog);
        assert!(!app.dialog_open.get(), "点遮罩应关闭对话框");
    }

    // ---- 文本是「子节点」而不是画笔 ----

    /// 按钮文案必须是**文本子节点**：宽度自己撑出来、位置由 flex 居中。
    ///
    /// 这条断言锁住两件事：
    /// 1. 文案不再是 `draw` 里的 `draw_text`（那样父布局不知道它多高，居中只能手算）；
    /// 2. `align_fix: true` + `alignItem: center` + `directionJustify: center`
    ///    三个都在（少任何一个都会偏）。
    #[test]
    fn button_label_is_a_text_child_centered_by_flex() {
        let app = Rc::new(state::App::new());
        let theme = tokens::Theme::new(app.theme_dark);
        let viewport = Rc::new(Cell::new((VW, VH)));
        let root = build_root_node2(root_arg(viewport), move |cx| {
            cx.child(button! {
                theme: theme,
                label: "提交".to_string(),
                on_click: || {},
            });
        });
        clear_layout_memo();

        let btn = root.children().first().cloned().expect("按钮应挂在根下");
        let kids = btn.children();
        assert_eq!(kids.len(), 1, "按钮内容应是**一个子节点**（文本）");
        let label = &kids[0];
        assert!(
            label.width() > 0.0 && label.height() > 0.0,
            "文本节点必须自己撑出尺寸（{}×{}），否则父布局排不动",
            label.width(),
            label.height()
        );
        assert!(
            label.width() < btn.width(),
            "文案宽 {} 应小于按钮宽 {}",
            label.width(),
            btn.width()
        );
        // 交叉轴居中：y = (按钮高 − 文本高) / 2
        let want_y = (btn.height() - label.height()) / 2.0;
        assert!(
            (label.y() - want_y).abs() <= 1.0,
            "文本 y={} 应贴住「上下余量相等」的位置 {}",
            label.y(),
            want_y
        );
        // 主轴居中：x = (按钮宽 − 文本宽) / 2
        let want_x = (btn.width() - label.width()) / 2.0;
        assert!(
            (label.x() - want_x).abs() <= 1.0,
            "文本 x={} 应水平居中于 {}",
            label.x(),
            want_x
        );
    }
}
