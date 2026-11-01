//! 计数器 —— **纯 Node2 运行时**（`Node2App`，不经过 v1 `to_node` 桥接）。
//!
//! 全文件只有**一种**声明式语法：
//!
//! ```ignore
//! node2! {                       // 基础节点：命名参数，省略即默认
//!     layout: flex! { .. },      // 容器布局：一条 LayoutDir 策略（缺省绝对定位）
//!     width: 220.0,              // #[getter] 字段：写表达式，宏包成 move || ..
//!     children: vec![
//!         label! { text: "标题", size: 15.0, color: TEXT },
//!         btn! { text: "+", fill: ACCENT, on_click: .. },
//!     ],
//! }
//! ```
//!
//! **声明层**没有 `XxxArg { .. }.fc()`、没有 `.of()`、没有手写 `impl Default`：
//! 组件就是 `#[rec]` 注解的普通函数，`!` 宏点完 setter 直接 `.build()`
//! 出函数签名上写的返回类型（这里全是 `Fc`）。
//!
//! `wrapped_text_node!` 也直接产 `Fc`（TS `wrappedTextNode` 的逐字对应）：
//! 它只做参数合成、节点身份交给 `rich_text_node`，调用处不再需要 `.of()`。
//!
//! `#[getter]` 字段写的是**表达式**，不是它此刻的值：`text: format!("{}", c.get())`
//! 会被宏包成 `move || format!(..)`，绘制期重新求值——信号一变下次重绘就反映出来。
//! 同一个 `Get<T>` 要往下传时写 `text: readout()`（括号即求值），别写 `text: readout`。
//!
//! 运行：`cargo run -p wy-app --example node2-counter`
//! 出图：`cargo run -p wy-app --example node2-counter -- --snapshot docs/shot-node2-counter.png --snapshot-size 420x600`

mod theme;

use std::cell::Cell;
use std::rc::Rc;

use theme::*;
use wy_engine::node2_app::Node2App;
use wy_engine::runner::run;
use wy_mve::canvas_render2::build_root_node2;
use wy_mve::wrapped_text_node;
use wy_mve::{
    flex, grow, node2, rec, AlignItem, Color, CursorType, DirectionJustify, Fc, Layout, Node2,
    Scene,
};
use wy_render::text_measure::measure_text;
use wy_render::{Point, Rect};
use wy_signal::{GetValue, SetValue, Signal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = Signal::new(0i32);

    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        let app = make_app(count);
        wy_app::snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    run(make_app(count))?;
    Ok(())
}

/// 把声明式页面挂到画布根下，包成 [`Node2App`]。
fn make_app(count: Signal<i32>) -> Node2App {
    Node2App::new(move |viewport| {
        let page_vp = viewport.clone();
        build_root_node2(theme::root_arg(viewport), move |cx| {
            cx.child(page! { count, viewport: page_vp });
        })
    })
    .with_title("wy-ui · node2 · counter")
}

// ============================================================================
// 页面装配
// ============================================================================

/// 整页：页头 + 大数字面板 + 控制条 + 页脚。
#[rec]
fn page(count: Signal<i32>, viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    // 页面铺满画布：TS 里页面节点同样拿画布的 `width`/`height`（不靠布局去猜）。
    let (vw, vh) = (viewport.clone(), viewport);
    let (c_head, c_disp, c_foot) = (count, count, count);
    let (c_dec, c_reset, c_inc) = (count, count, count);

    node2! {
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Stretch,
            align_fix: true,
            gap: 14.0,
        },
        width: vw.get().0,
        height: vh.get().1,
        padding: 20.0,
        draw: |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), BG);
        },
        children: vec![
            // 页头：左标题 + 弹性空隙 + 右侧实时读数
            header! { readout: format!("count = {}", c_head.get()) },
            spacer! {},
            // 中间面板：大数字 + 一行说明（grow 吃掉剩余高度）
            panel! {
                grow: 1.0,
                children: vec![
                    count_display! { count: c_disp.get() },
                    label! { text: "点击下方按钮改变计数", size: 12.0, color: FAINT },
                ],
            },
            // 控制条：− / 重置 / ＋
            node2! {
                layout: flex! {
                    direction: Layout::Row,
                    align_item: AlignItem::Center,
                    direction_justify: DirectionJustify::Center,
                    gap: 12.0,
                },
                children: vec![
                    btn! { text: "-", fill: PANEL2, on_click: move || c_dec.set(c_dec.get() - 1) },
                    btn! { text: "重置", fill: PANEL2, on_click: move || c_reset.set(0) },
                    btn! { text: "+", fill: ACCENT, on_click: move || c_inc.set(c_inc.get() + 1) },
                ],
            },
            // 页脚
            label! { text: format!("当前 {}", c_foot.get()), size: 11.0, color: FAINT },
        ],
    }
}

/// 页头：标题 + 右侧读数。
///
/// `readout` 是 `#[getter]`，所以调用处能直接给活闭包，绘制期每次都重算。
#[rec]
fn header(
    #[getter]
    #[props(default = String::new())]
    readout: String,
) -> Fc {
    node2! {
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            gap: 10.0,
        },
        children: vec![
            label! { text: "计数器·纯 Node2", size: 15.0 },
            spacer! {},
            label! { text: readout(), size: 12.0, color: MUTED },
        ],
    }
}

// ============================================================================
// 基础组件：`#[rec]` + `!` 宏，默认值写在字段上
// ============================================================================

/// 文本标签：`text` / `size` / `color` 全是 getter，缺省即 `""` / 13 / `TEXT`。
///
/// 走 canvasRender2 自己的 [`wrapped_text_node`]（TS `WrappedTextNode.ts`），
/// **不用** v1 的 `text_styled`——那会把旧 `Node` 经 `From<Node> for Fc`
/// 桥接塞进新树，等于把两套体系焊在一起。
#[rec]
fn label(
    #[getter]
    #[props(default = String::new())]
    text: String,
    #[getter]
    #[props(default = 13.0f32)]
    size: f32,
    #[getter]
    #[props(default = TEXT)]
    color: Color,
) -> Fc {
    wrapped_text_node! {
        text: text(),
        font_size: size(),
        color: color(),
    }
}

/// 弹性空隙：`grow` 吃掉主轴剩余空间，把左右两组内容推到两端。
///
/// `grow` 不在 props 里——它是布局扩展，走 `exts`（TS `exts: [grow(..)]`）。
#[rec]
fn spacer(#[props(default = 1.0f32)] weight: f32) -> Fc {
    node2! { exts: vec![grow(weight)] }
}

/// 圆角面板：画底 + 边框，子节点在内部按 Column 居中排布。
#[rec]
fn panel(
    #[props(default = 0.0f32)] grow: f32,
    #[props(default = PANEL)] fill: Color,
    children: Vec<Fc>,
) -> Fc {
    node2! {
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Center,
            direction_justify: DirectionJustify::Center,
            // 交叉轴（宽）由父分配：面板是整页的 stretch 子节点，宽度跟着页面走，
            // 而不是缩到内容宽（`count_display` 的 220）。
            align_fix: true,
            gap: 6.0,
        },
        exts: vec![wy_mve::grow(grow)],
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 12.0, fill);
            s.stroke_round_rect(
                Rect::new(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0)),
                12.0,
                LINE,
                1.0,
            );
        },
        children,
    }
}

/// 主按钮：圆角底板 + 居中文字（文字是子节点，自然尺寸撑起按钮）。
#[rec]
fn btn(
    #[getter]
    #[props(default = String::new())]
    text: String,
    #[props(default = ACCENT)] fill: Color,
    #[props(required)] on_click: impl Fn() + 'static,
) -> Fc {
    node2! {
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            // 自动宽按钮的**宽**必须来自子节点（`Grow` 是"由内容撑开"的那一支）；
            // 写成 `Center` 就等于"尺寸向父级要"，而父级又要先量它 → 成环取 0，
            // 按钮只剩 padding。文字本来就正好撑满，居中是多余的一步。
            direction_justify: DirectionJustify::Grow,
        },
        padding_inline_start: 18.0,
        padding_inline_end: 18.0,
        padding_block_start: 8.0,
        padding_block_end: 8.0,
        // 可点击 → 手型光标（TS `cursor: () => CursorType.POINTER`）
        cursor: CursorType::Pointer,
        mouse_click: move |_e| on_click(),
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 8.0, fill);
        },
        children: vec![label! { text: text(), size: 14.0, color: BG }],
    }
}

/// 大数字显示：定尺寸 + 居中绘制（`count` 是 getter，值变即重绘）。
#[rec]
fn count_display(
    #[getter]
    #[props(default = 0)]
    count: i32,
) -> Fc {
    node2! {
        width: 220.0,
        height: 104.0,
        draw: move |s: &mut Scene, n: &Node2| {
            let t = count().to_string();
            let (w, h) = measure_text(&t, 72.0);
            s.draw_text(
                Point::new((n.width() - w) / 2.0, (n.height() - h) / 2.0),
                &t,
                72.0,
                ACCENT,
            );
        },
    }
}
