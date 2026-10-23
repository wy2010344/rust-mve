//! 浮层（复刻 Kotlin `helper/Overlay.kt` + `Dialog.kt` + `Dropdown.kt`）。
//!
//! Kotlin `OverlayBase` 的核心语义在 Rust 里落成一个共用的浮层根骨架：
//! - `not_in_layout` + 未声明尺寸（引擎自动撑满父容器内区）：脱离布局流、
//!   文档序最后声明（绘制在最上层、命中最先命中——引擎对顶层节点倒序做
//!   hit test）；
//! - 关闭 = 整个浮层 `hide`，不占布局也不拦事件；
//! - 根 `on_click` = 点击空白关闭；内容面板用 `on_click_ev` + `stop_propagation`
//!   拦截冒泡（点面板不关，点遮罩/外部关）；
//! - 根 `key_fn` 响应 Esc：引擎在焦点链未消费时全局广播按键，故 Esc 关闭
//!   不依赖焦点位置（对应 Kotlin `engineGlobal.registerKeyPress`）；
//! - `focus_trap` 把 Tab 圈定在浮层内（模态语义，对应 Kotlin `trapsFocus`）。
//!
//! 差异仅在锚点与遮罩：`dialog` 居中面板 + 半透明遮罩；`dropdown` 面板锚定
//! 动态坐标（随点击位置），无遮罩。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::{
    component, AlignItem, DirectionJustify, Key, KeyEvent, Layout, LayoutBox, Node, NodeContext,
    PointerEvent,
};
use wy_render::{Color, Rect, Scene};

/// 模态遮罩色（Kotlin `DialogBase.drawScrim` 同款半透明黑）。
const SCRIM: Color = Color::rgba(0, 0, 0, 110);

/// 面板锚点坐标的动态读取器（`(x, y)`）。
///
/// 锚点是**动态**的：dropdown 的锚点跟着点击位置走，调用处传的是读信号的闭包，
/// 所以这里存闭包而不是坐标本身。
type Anchor = (Rc<dyn Fn() -> f32>, Rc<dyn Fn() -> f32>);

/// 可视区句柄：浮层根的 `last_box`（引擎在布局/命中期写入）。
///
/// `Node::clone` 共享同一份 `Rc<Cell>`，故浮层根与面板看到的是同一份数据。
type ViewportBox = Rc<Cell<LayoutBox>>;

/// 可视区句柄的可选容器：浮层根建好后回填，面板再取。
type ViewportSlot = Rc<RefCell<Option<ViewportBox>>>;

/// 模态对话框：遮罩 + 居中面板，Esc / 点击遮罩关闭，点击面板不关闭。
///
/// 打开状态由 `open` 闭包驱动（一般读信号），关闭时整个浮层隐藏；
/// `on_close` 在 Esc 与点击遮罩时回调，业务在里面写回信号即可。
/// 打开期间 `focus_trap` 圈定焦点，Tab 只在面板内循环。
///
/// 浮层根未声明任何尺寸——引擎对"宽高均为 0 的 `not_in_layout` 子节点"
/// 自动分配父容器内区，因此浮层天然**撑满父容器**（顶层即铺满窗口，
/// 窗口缩放自动跟随），无需调用处传窗口尺寸。
///
/// ```ignore
/// dialog {
///     open: move || app.dialog_open.get(),
///     on_close: move || app.dialog_open.set(false),
///     surface: move || theme.tokens().card,
///     border: move || theme.tokens().border,
///     // 子节点即面板内容
/// }
/// ```
#[component]
pub fn dialog(
    /// 是否打开（getter：一般 `move || signal.get()`）。
    open: impl Fn() -> bool + 'static,
    /// 面板宽度（高度由内容撑起）。
    #[props(default = 380.0f32)]
    panel_width: f32,
    /// 关闭回调（Esc / 点击遮罩触发）。
    on_close: impl Fn() + 'static,
    /// 面板底色（getter，随主题切换重绘）。
    surface: impl Fn() -> Color + 'static,
    /// 面板边框色（getter）。
    border: impl Fn() -> Color + 'static,
    children: Vec<Slot>,
) -> Node {
    // 宏已把 `impl Fn` 参数转成 `Rc<dyn Fn>`，直接克隆共享即可
    // 面板：圆角卡片，点击拦截冒泡（不触发遮罩关闭）
    let panel_children = Rc::new(RefCell::new(Some(children)));
    let panel = Node {
        layout: Some(Layout::Column),
        width: panel_width,
        gap: 10.0,
        padding: 20.0,
        align_items: AlignItem::Stretch,
        on_click_fn: Some(Rc::new(|ev: &mut PointerEvent| ev.stop_propagation())),
        draw_fn: Rc::new(move |any: &mut dyn std::any::Any| {
            let Some(s) = any.downcast_mut::<Scene>() else {
                return;
            };
            let b = crate::current_box();
            let r = Rect::new(0.0, 0.0, b.width, b.height);
            s.fill_round_rect(r, 12.0, surface());
            s.stroke_round_rect(r, 12.0, border(), 1.0);
        }),
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            if let Some(c) = panel_children.borrow_mut().take() {
                for s in c {
                    cx.child(s);
                }
            }
        }),
        ..Node::default()
    };
    overlay_root(0.0, 0.0, open, true, on_close, panel)
}

/// 下拉浮层：面板锚定动态坐标（如触发按钮下方），点击外部 / Esc 关闭。
///
/// 锚点在事件期捕获——触发按钮的 `on_click_ev` 里
/// `anchor.set((ev.root_x - ev.x, ev.root_y - ev.y))` 即按钮绝对坐标，
/// `anchor_x`/`anchor_y` 传读该信号的闭包（面板 y 一般再加按钮高度做间距）。
/// 不圈定焦点（Tab 可离开，对应 Kotlin Dropdown 的非模态语义）。
/// 浮层根与 dialog 一样未声明尺寸，自动撑满父容器（顶层即铺满窗口）。
///
/// # 自动翻转（下方没空间时向上弹）
///
/// 触发按钮在窗口底部时，面板若一味向下展开就会跑到屏幕外——**完全看不见**。
/// 这里按浮层可视区（浮层根的布局盒 = 窗口内区）与面板自身高度计算最终位置：
/// - 下方够放 → 正常向下展开；
/// - 下方不够 → **向上翻转**，贴在按钮上方；
/// - 两侧都不够 → 钳制在可视区内（不跑到屏幕外）。
///
/// 面板高度按内容实测（`node_size`），故菜单项增减也能正确翻转。
#[component]
pub fn dropdown(
    /// 是否打开（getter）。
    open: impl Fn() -> bool + 'static,
    /// 面板锚点 x（覆盖层局部坐标；getter，可随点击位置动态变化）。
    ///
    /// 注意不叫 `x`/`y`：这两个名字是通用布局属性，会落到浮层根上。
    anchor_x: impl Fn() -> f32 + 'static,
    /// 面板锚点 y（getter）。
    anchor_y: impl Fn() -> f32 + 'static,
    /// 面板宽度（高度由内容撑起）。
    #[props(default = 160.0f32)]
    panel_width: f32,
    /// 关闭回调（Esc / 点击外部触发）。
    on_close: impl Fn() + 'static,
    /// 面板底色（getter）。
    surface: impl Fn() -> Color + 'static,
    /// 面板边框色（getter）。
    border: impl Fn() -> Color + 'static,
    children: Vec<Slot>,
) -> Node {
    // 宏已把 `impl Fn` 参数转成 `Rc<dyn Fn>`，直接克隆共享即可
    // 面板：绝对定位（not_in_layout + x_fn/y_fn），点击拦截冒泡
    let panel_children = Rc::new(RefCell::new(Some(children)));
    let panel = Node {
        layout: Some(Layout::Column),
        width: panel_width,
        gap: 2.0,
        padding: 8.0,
        align_items: AlignItem::Stretch,
        not_in_layout: true,
        // 位置由 `place_panel` 装上（下方不够则向上翻转）
        on_click_fn: Some(Rc::new(|ev: &mut PointerEvent| ev.stop_propagation())),
        draw_fn: Rc::new(move |any: &mut dyn std::any::Any| {
            let Some(s) = any.downcast_mut::<Scene>() else {
                return;
            };
            let b = crate::current_box();
            let r = Rect::new(0.0, 0.0, b.width, b.height);
            s.fill_round_rect(r, 8.0, surface());
            s.stroke_round_rect(r, 8.0, border(), 1.0);
        }),
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            if let Some(c) = panel_children.borrow_mut().take() {
                for s in c {
                    cx.child(s);
                }
            }
        }),
        ..Node::default()
    };

    let anchor = (Rc::clone(&anchor_x), Rc::clone(&anchor_y));
    // 面板自引用：翻转计算要按内容实测面板高度（`node_size`）
    let self_ref: Rc<RefCell<Option<Node>>> = Rc::new(RefCell::new(None));

    let (panel, area) = place_panel(panel, anchor, self_ref.clone());

    let root = overlay_root(0.0, 0.0, open, false, on_close, panel);

    // 回填浮层根的布局盒句柄：面板据此判断可视区（窗口内区），
    // 决定向下展开还是向上翻转。
    // 引擎在布局/命中期写入 `last_box`，而 `Node::clone` 共享同一份
    // `Rc<Cell>` —— 故浮层根与面板看到的是同一份数据。
    *area.borrow_mut() = Some(Rc::clone(&root.last_box));

    root
}

/// 给下拉面板装上「自动翻转」的位置计算。
///
/// 面板位置在**布局/命中期**算（`x_fn`/`y_fn` 每次都重新求值）：
/// - 可视区来自浮层根的布局盒（= 窗口内区）；
/// - 面板尺寸按**内容实测**（`node_size`），故菜单项增减也能正确翻转；
/// - 下方空间不足 → 向上翻转；两侧都不足 → 钳制在可视区内。
///
/// 可视区从浮层根的 `last_box` 取——引擎在布局/命中期写入它，而
/// `Node::clone` 共享同一份 `Rc<Cell>`，故浮层根与面板看到的是同一份数据。
fn place_panel(
    mut panel: Node,
    anchor: Anchor,
    self_ref: Rc<RefCell<Option<Node>>>,
) -> (Node, ViewportSlot) {
    let (ax, ay) = anchor;
    // 可视区：先用占位，浮层根建好后回填其 `last_box` 句柄。
    let area: ViewportSlot = Rc::new(RefCell::new(None));

    // x：钳制在可视区右缘内（面板不超出右缘）
    let area_x = Rc::clone(&area);
    let size_x = Rc::clone(&self_ref);
    let ax2 = Rc::clone(&ax);
    panel.x_fn = Some(Rc::new(move || {
        let vw = area_x
            .borrow()
            .as_ref()
            .map(|b| b.get().width)
            .unwrap_or(0.0);
        let pw = size_x
            .borrow()
            .as_ref()
            .map(|n| crate::node_size(n).0)
            .unwrap_or(0.0);
        if vw <= 0.0 {
            return ax2();
        }
        let max_x = (vw - pw).max(0.0);
        ax2().min(max_x).max(0.0)
    }));

    // y：下方够 → 向下；不够 → 向上翻转；都不够 → 钳制
    let area_y = Rc::clone(&area);
    let size_y = Rc::clone(&self_ref);
    let ay2 = Rc::clone(&ay);
    panel.y_fn = Some(Rc::new(move || {
        let vh = area_y
            .borrow()
            .as_ref()
            .map(|b| b.get().height)
            .unwrap_or(0.0);
        let ph = size_y
            .borrow()
            .as_ref()
            .map(|n| crate::node_size(n).1)
            .unwrap_or(0.0);
        let y = ay2();
        if vh <= 0.0 || ph <= 0.0 {
            return y;
        }
        // 下方剩余空间
        if vh - y >= ph {
            return y; // 下方够：正常向下
        }
        // 上方剩余空间（翻转后贴在 y 上方）
        if y >= ph {
            return y - ph; // 上方够：向上翻转
        }
        // 两侧都不够：钳制在可视区内，保证面板完整可见
        (vh - ph).max(0.0)
    }));

    // 自引用：让上面的闭包能实测面板尺寸
    *self_ref.borrow_mut() = Some(panel.clone());

    (panel, area)
}

/// 浮层根骨架（Kotlin `OverlayBase`）：
/// 铺满、绝对定位、按 `open` 显隐、点击空白关闭、Esc 关闭。
///
/// `modal` 为 true 时额外画半透明遮罩并圈定焦点；面板节点由调用方给定
/// （dialog 居中参与布局，dropdown 绝对定位不参与布局）。
fn overlay_root(
    width: f32,
    height: f32,
    open: Rc<dyn Fn() -> bool>,
    modal: bool,
    on_close: Rc<dyn Fn()>,
    panel: Node,
) -> Node {
    let mut panel = panel;
    panel.layout = Some(Layout::Column);
    let open_hide = Rc::clone(&open);
    let open_click = Rc::clone(&open);
    let open_key = Rc::clone(&open);
    let close_click = Rc::clone(&on_close);
    let close_key = Rc::clone(&on_close);
    let (justify, align_items) = if modal {
        // 模态面板居中：面板是唯一参与布局的子节点，主轴/交叉轴都居中
        (DirectionJustify::Center, AlignItem::Center)
    } else {
        (DirectionJustify::Grow, AlignItem::Start)
    };
    Node {
        layout: Some(Layout::Column),
        width,
        height,
        // 脱离父容器布局流：x/y 即窗口原点，文档序最后声明 → 最上层
        not_in_layout: true,
        x: 0.0,
        y: 0.0,
        justify,
        align_items,
        hide_fn: Some(Rc::new(move || !open_hide())),
        // 点击空白（遮罩/外部）关闭；面板拦截冒泡，点内容不会到这里
        on_click_fn: Some(Rc::new(move |_ev: &mut PointerEvent| {
            if open_click() {
                close_click();
            }
        })),
        // Esc 关闭：引擎在焦点链未消费时全局广播按键，无需焦点在浮层内
        key_fn: Some(Rc::new(move |ev: &mut KeyEvent| {
            if ev.key == Key::Escape && open_key() {
                close_key();
                true
            } else {
                false
            }
        })),
        // 模态：Tab 只在浮层内循环（Kotlin focusTrap = enabled && trapsFocus）
        focus_trap: modal,
        draw_fn: Rc::new(move |any: &mut dyn std::any::Any| {
            if !modal {
                return;
            }
            let Some(s) = any.downcast_mut::<Scene>() else {
                return;
            };
            let b = crate::current_box();
            s.fill_rect(Rect::new(0.0, 0.0, b.width, b.height), SCRIM);
        }),
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            cx.add_node(panel.clone());
        }),
        ..Node::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Slot;

    /// 构造一个 dropdown 浮层根：面板高度固定 `panel_h`，
    /// 锚点在 `(ax, ay)`。返回根节点与面板节点。
    fn make_dropdown(open: bool, ax: f32, ay: f32, panel_w: f32, panel_h: f32) -> (Node, Node) {
        let panel_children = Rc::new(RefCell::new(Some(vec![Slot::One(Node {
            width: panel_w,
            height: panel_h,
            ..Node::default()
        })])));
        let panel = Node {
            layout: Some(Layout::Column),
            width: panel_w,
            gap: 2.0,
            padding: 8.0,
            align_items: AlignItem::Stretch,
            not_in_layout: true,
            on_click_fn: Some(Rc::new(|ev: &mut PointerEvent| ev.stop_propagation())),
            draw_fn: Rc::new(|_any: &mut dyn std::any::Any| {}),
            arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
                if let Some(c) = panel_children.borrow_mut().take() {
                    for s in c {
                        cx.child(s);
                    }
                }
            }),
            ..Node::default()
        };

        let anchor = (
            Rc::new(move || ax) as Rc<dyn Fn() -> f32>,
            Rc::new(move || ay) as Rc<dyn Fn() -> f32>,
        );
        let self_ref: Rc<RefCell<Option<Node>>> = Rc::new(RefCell::new(None));
        let (panel, area) = place_panel(panel, anchor, self_ref.clone());

        let root = overlay_root(
            0.0,
            0.0,
            Rc::new(move || open),
            false,
            Rc::new(|| {}),
            panel.clone(),
        );
        *area.borrow_mut() = Some(Rc::clone(&root.last_box));
        (root, panel)
    }

    /// 模拟引擎给浮层根分配可视区，并触发面板位置计算。
    fn place(root: &Node, panel: &Node, viewport: (f32, f32)) -> (f32, f32) {
        root.last_box
            .set(crate::LayoutBox::new(0.0, 0.0, viewport.0, viewport.1));
        (panel.pos_x(), panel.pos_y())
    }

    #[test]
    fn dropdown_flips_up_when_no_space_below() {
        // 触发按钮在窗口底部（y=560），窗口高 600，面板高 100：
        // 下方只剩 40px 放不下 → 必须向上翻转。
        let (root, panel) = make_dropdown(true, 100.0, 560.0, 150.0, 100.0);
        let (_, y) = place(&root, &panel, (800.0, 600.0));
        assert!(y < 560.0, "下方空间不足时应向上翻转，实际 y={y}");
        assert!(
            y + 100.0 <= 600.0,
            "翻转后面板仍应完整落在窗口内，实际 y={y}"
        );
    }

    #[test]
    fn dropdown_opens_downward_when_space_below() {
        // 触发按钮在窗口上部（y=100），窗口高 600：下方够 → 正常向下。
        let (root, panel) = make_dropdown(true, 100.0, 100.0, 150.0, 100.0);
        let (_, y) = place(&root, &panel, (800.0, 600.0));
        assert_eq!(y, 100.0, "下方空间充足时应按锚点向下展开");
    }

    #[test]
    fn dropdown_is_clamped_inside_viewport() {
        // 面板比窗口还高：两侧都不够 → 钳制在可视区内，
        // 不能跑到屏幕外（否则完全看不见）。
        let (root, panel) = make_dropdown(true, 100.0, 300.0, 150.0, 500.0);
        let (_, y) = place(&root, &panel, (800.0, 600.0));
        assert!(y >= 0.0, "面板不应超出上缘，实际 y={y}");
        assert!(y + 500.0 <= 600.0 + 0.5, "面板不应超出下缘，实际 y={y}");
    }

    #[test]
    fn dropdown_x_is_clamped_inside_viewport() {
        // 锚点靠右：面板不应超出右缘。
        let (root, panel) = make_dropdown(true, 780.0, 100.0, 150.0, 100.0);
        let (x, _) = place(&root, &panel, (800.0, 600.0));
        assert!(x + 150.0 <= 800.0 + 0.5, "面板不应超出右缘，实际 x={x}");
    }

    #[test]
    fn dropdown_without_viewport_falls_back_to_anchor() {
        // 还没拿到可视区（首帧）时退化为锚点位置，不做翻转。
        let (_, panel) = make_dropdown(true, 100.0, 560.0, 150.0, 100.0);
        assert_eq!(panel.pos_y(), 560.0);
    }
}
