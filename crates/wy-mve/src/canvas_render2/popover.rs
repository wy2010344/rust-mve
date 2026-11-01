//! 浮层：模态对话框 [`dialog`] 与锚定下拉 [`dropdown`]，**纯 canvasRender2**。
//!
//! 对标 TS `helper/Popover.ts`（`popoverPanel` + `showPopover`）与 v1 `overlay.rs`
//! 的两种形态，但**换的是原语，不是写法**：
//!
//! | v1（`overlay.rs`） | v2（本模块） |
//! |---|---|
//! | `#[component]` + `Node` 字段（`hide_fn` / `key_fn` / `on_click_fn`） | `#[rec]` 风格的普通函数 + `node2!` |
//! | 浮层根尺寸靠引擎"未声明 → 撑满父容器"的特例 | 显式 `size(axis) = 父 inner`（TS `innerSize(view, axis)`） |
//! | 面板位置靠 `x_fn` / `y_fn` + 自引用 `node_size` | `position(axis)` → [`resolve_placement`]（TS 同款） |
//! | 遮罩绘制用 `current_box()`（v1 全局盒） | `n.width()` / `n.height()`（Node2 自己的几何） |
//! | Esc 靠引擎广播 `key_fn` | 焦点在浮层内走 `handle_key`，否则走全局旁听（两条都接） |
//!
//! # 为什么面板要显式声明尺寸
//!
//! `not_in_layout` 的节点进不了父布局，`size_from_parent` 拿不到 `child_size`。
//! TS 的 `popoverPanel` 因此显式写 `size(d) => sizeFromChildren(ref.get(), d)`。
//! Rust 这边**不写** `size`：`default_size` 会先问 `allows_size_from_children`
//! （列容器 + 无 grow 子 → 真），直接由子项撑起，不会落到 `size_from_parent`。
//! 少写一条正是"用引擎的默认语义"，不是省略。
//!
//! # 关闭的全部路径
//!
//! - 点空白（遮罩 / 面板外）→ 浮层根的 `mouse_click`；面板自己
//!   `stop_propagation`，所以点内容不关闭；
//! - Esc → 焦点在浮层内时 `handle_key` 消费；焦点不在时由
//!   `register_key_press` 全局旁听补上（对应 v1 "焦点链未消费时全局广播"）；
//! - 关闭 = `hide: !open()`：不绘制、不命中、不进焦点链，也不占布局。
//!
//! # 监听的生命周期
//!
//! `Node2App` 的工厂是 `FnOnce`、树只建一次，全局旁听与树同寿命；注销句柄被
//! 返回的 [`Fc`] 捕获（`_alive`），树丢掉时一并释放。

use std::rc::Rc;

use wy_render::{Color, Rect, Scene};

use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::focus::{KeyCode2, KeyEvent2};
use crate::canvas_render2::node2::{Axis, LayoutDir2, Node2};
use crate::canvas_render2::placement::{
    resolve_placement, Placement, PlacementRect, PlacementSize,
};
use crate::canvas_render2::protocol::MouseEvent2;
use crate::canvas_render2::tree::engine_global::register_key_press;
use crate::canvas_render2::tree::geometry::{inner_size, layout_size, LAYOUT_SIZE_0};
use crate::{AlignItem, Layout};

/// 面板圆角（与 v1 `overlay.rs` 一致：dialog 12、dropdown 8）。
const DIALOG_RADIUS: f32 = 12.0;
const DROPDOWN_RADIUS: f32 = 8.0;

/// 模态遮罩色（Kotlin `DialogBase.drawScrim` 同款半透明黑）。
const SCRIM: Color = Color::rgba(0, 0, 0, 110);

/// 浮层根：铺满父 inner、按 `open` 显隐、点空白关闭、Esc 关闭。
///
/// - `modal = true`：画遮罩 + `focus_trap`（dialog）；
/// - `modal = false`：透明、不圈焦点（dropdown）。
fn overlay_root(
    root_ref: &Ref2<Node2>,
    name: &'static str,
    open: &Rc<dyn Fn() -> bool>,
    on_close: &Rc<dyn Fn()>,
    modal: bool,
    panel: Fc,
) -> Fc {
    // 尺寸：铺满父 inner（TS `innerSize(view, axis)`）。
    // 浮层根**不参与布局**，所以这条 `size` 是它唯一的尺寸来源；
    // `inner_size` 读的是父节点的 `width() - padding`，父是画布根即整窗。
    let size_ref = root_ref.clone();
    let open_hide = Rc::clone(open);
    let open_click = Rc::clone(open);
    let close_click = Rc::clone(on_close);
    let open_key = Rc::clone(open);
    let close_key = Rc::clone(on_close);
    let open_trap = Rc::clone(open);

    node2! {
        node_ref: root_ref.clone(),
        not_in_layout: true,
        // `not_in_layout` 节点没有布局位置可分配：`x()`/`y()` 若不显式给，会落到
        // `default_position` → 问父布局要 `child_position(layout_index)`，而
        // `layout_index()` 对不参与布局的节点是硬性错误（TS 同款 throw）。
        // 浮层根就在父的原点，两个 0 就是它的语义。
        x: 0.0,
        y: 0.0,
        size: move |axis: Axis| match size_ref.get().parent() {
            Some(parent) => layout_size(inner_size(&parent, axis), false),
            None => LAYOUT_SIZE_0,
        },
        hide: !open_hide(),
        // 模态：Tab 只在浮层内循环（Kotlin `focusTrap = enabled && trapsFocus`）
        focus_trap: modal && open_trap(),
        // 点击空白（遮罩 / 面板外）关闭；面板拦截冒泡，点内容不会到这里
        mouse_click: move |_e: &mut MouseEvent2| {
            if open_click() {
                close_click();
            }
        },
        // 焦点在浮层内时，Esc 走这条
        handle_key: move |e: &KeyEvent2| {
            if e.code == KeyCode2::ESCAPE && open_key() {
                close_key();
                true
            } else {
                false
            }
        },
        // 非模态不画（原先是 `draw: None`，这里内联成一次判断，省一个分支）。
        draw: move |s: &mut Scene, n: &Node2| {
            if modal {
                s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), SCRIM);
            }
        },
        // 诊断名：调试与测试按名字定位浮层（不参与任何求值）。
        debug_name: name.to_string(),
        children: vec![panel],
    }
}

/// 面板的列排布策略：交叉轴撑满 + 指定间距。
fn column_stack(gap: f32) -> LayoutDir2 {
    flex! {
        direction: Layout::Column,
        align_item: AlignItem::Stretch,
        gap: gap,
    }
}

/// 面板骨架：列排布 + 内边距 + 圆角卡片背景，点击拦截冒泡。
#[allow(clippy::too_many_arguments)]
fn panel_node(
    node_ref: &Ref2<Node2>,
    name: &'static str,
    width: f32,
    radius: f32,
    padding: f32,
    gap: f32,
    position: Rc<dyn Fn(Axis) -> f32>,
    surface: Rc<dyn Fn() -> Color>,
    border: Rc<dyn Fn() -> Color>,
    children: Vec<Fc>,
) -> Fc {
    node2! {
        node_ref: node_ref.clone(),
        width: width,
        not_in_layout: true,
        layout: column_stack(gap),
        // 四向同值，走 `node2` 的简写字段（TS 里也是四个 `paddingInline*` 同源）。
        padding: padding,
        position: move |d| position(d),
        // 点面板不关闭：事件在叶端中断冒泡，浮层根的 `mouse_click` 收不到
        mouse_click: |e: &mut MouseEvent2| e.stop_propagation(),
        draw: move |s: &mut Scene, n: &Node2| {
            let r = Rect::new(0.0, 0.0, n.width(), n.height());
            s.fill_round_rect(r, radius, surface());
            s.stroke_round_rect(r, radius, border(), 1.0);
        },
        debug_name: name.to_string(),
        children,
    }
}

/// 模态对话框：遮罩 + 居中面板，Esc / 点击遮罩关闭，点击面板不关闭。
///
/// ```ignore
/// dialog(
///     move || app.dialog_open.get(),
///     360.0,
///     move || app.dialog_open.set(false),
///     move || theme.tokens().card,
///     move || theme.tokens().border,
///     vec![/* 面板内容 */],
/// )
/// ```
///
/// 浮层必须挂在**画布根**下（而不是页面节点里），这样可视区才是整窗：
///
/// ```ignore
/// build_root_node2(root_arg(viewport), move |cx| {
///     cx.child(page);
///     cx.child(dialog(..));   // 文档序最后 → 绘制在最上层、命中最先命中
/// });
/// ```
pub fn dialog(
    open: impl Fn() -> bool + 'static,
    panel_width: f32,
    on_close: impl Fn() + 'static,
    surface: impl Fn() -> Color + 'static,
    border: impl Fn() -> Color + 'static,
    children: Vec<Fc>,
) -> Fc {
    let open: Rc<dyn Fn() -> bool> = Rc::new(open);
    let on_close: Rc<dyn Fn()> = Rc::new(on_close);
    let surface: Rc<dyn Fn() -> Color> = Rc::new(surface);
    let border: Rc<dyn Fn() -> Color> = Rc::new(border);

    let root_ref = Ref2::<Node2>::new();
    let panel_ref = Ref2::<Node2>::new();

    // 居中：面板左上角 = (可视区 - 面板尺寸) / 2。两轴各自求值。
    let center = panel_ref.clone();
    let position = Rc::new(move |axis: Axis| {
        let me = center.get();
        let view = me.parent().map_or(0.0, |parent| inner_size(&parent, axis));
        let own = match axis {
            Axis::X => me.width(),
            Axis::Y => me.height(),
        };
        ((view - own) * 0.5).max(0.0)
    });

    let panel = panel_node(
        &panel_ref,
        "dialog-panel",
        panel_width,
        DIALOG_RADIUS,
        20.0,
        10.0,
        position,
        surface,
        border,
        children,
    );

    let root = overlay_root(&root_ref, "dialog", &open, &on_close, true, panel);

    // 焦点不在浮层内时的 Esc 兜底（v1 靠引擎"焦点链未消费即广播"）。
    let open_k = Rc::clone(&open);
    let close_k = Rc::clone(&on_close);
    let alive = register_key_press(Rc::new(move |e: &KeyEvent2| {
        if e.code == KeyCode2::ESCAPE && open_k() {
            close_k();
        }
    }));

    Fc::new(move |cx| {
        // 注销句柄随 Fc 存活：树只建一次，与树同寿命
        let _alive = &alive;
        root.mount(cx);
    })
}

/// 下拉浮层：面板锚定动态矩形（如触发按钮），点击外部 / Esc 关闭。
///
/// 面板位置由 [`resolve_placement`] 在**布局期**算：下方放不下就向上翻转，
/// 两侧都放不下就贴边钳制，不会跑到窗口外看不见。面板尺寸取内容实测，
/// 故菜单项增减也能正确翻转。
///
/// `anchor` 一般取触发按钮的绝对矩形（事件期用
/// `(e.global_x - e.x, e.global_y - e.y)` 得到按钮原点，再加按钮宽高）。
/// 不圈定焦点（Tab 可离开，对应 Kotlin Dropdown 的非模态语义）。
#[allow(clippy::too_many_arguments)]
pub fn dropdown(
    open: impl Fn() -> bool + 'static,
    anchor: impl Fn() -> PlacementRect + 'static,
    placement: impl Fn() -> Placement + 'static,
    gap: f32,
    panel_width: f32,
    on_close: impl Fn() + 'static,
    surface: impl Fn() -> Color + 'static,
    border: impl Fn() -> Color + 'static,
    children: Vec<Fc>,
) -> Fc {
    let open: Rc<dyn Fn() -> bool> = Rc::new(open);
    let on_close: Rc<dyn Fn()> = Rc::new(on_close);
    let anchor: Rc<dyn Fn() -> PlacementRect> = Rc::new(anchor);
    let placement: Rc<dyn Fn() -> Placement> = Rc::new(placement);
    let surface: Rc<dyn Fn() -> Color> = Rc::new(surface);
    let border: Rc<dyn Fn() -> Color> = Rc::new(border);

    let root_ref = Ref2::<Node2>::new();
    let panel_ref = Ref2::<Node2>::new();

    let place_ref = panel_ref.clone();
    let place_anchor = Rc::clone(&anchor);
    let place_dir = Rc::clone(&placement);
    let position = Rc::new(move |axis: Axis| {
        let me = place_ref.get();
        let (view_w, view_h) = me.parent().map_or((0.0, 0.0), |parent| {
            (inner_size(&parent, Axis::X), inner_size(&parent, Axis::Y))
        });
        let (x, y) = resolve_placement(
            place_dir(),
            place_anchor(),
            PlacementSize::new(me.width(), me.height()),
            PlacementSize::new(view_w, view_h),
            gap,
        );
        match axis {
            Axis::X => x,
            Axis::Y => y,
        }
    });

    let panel = panel_node(
        &panel_ref,
        "dropdown-panel",
        panel_width,
        DROPDOWN_RADIUS,
        8.0,
        2.0,
        position,
        surface,
        border,
        children,
    );

    let root = overlay_root(&root_ref, "dropdown", &open, &on_close, false, panel);

    let open_k = Rc::clone(&open);
    let close_k = Rc::clone(&on_close);
    let alive = register_key_press(Rc::new(move |e: &KeyEvent2| {
        if e.code == KeyCode2::ESCAPE && open_k() {
            close_k();
        }
    }));

    Fc::new(move |cx| {
        let _alive = &alive;
        root.mount(cx);
    })
}
