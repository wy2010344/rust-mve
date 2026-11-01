//! 矩形容器（对标 TS `canvasRender2/RectNode.ts`）。
//!
//! TS 原文就一句组合：
//! ```ignore
//! export const rectNode = fc(function (ctx, arg: RectNodeArg) {
//!   const { background, borderColor, borderWidth = () => 1, radius = () => 0, ...rest } = arg;
//!   node2({ ...rest, draw(ctx2, n) { ...; rest.draw?.(ctx2, n); } })(ctx);
//! });
//! ```
//! 这里逐句对照：组件体就一行 `node2! { ..rest, draw: .. }`——`..rest` 是 TS 的
//! `...rest`，`draw` 是覆盖项；四个视觉属性全是读取式 getter。
//!
//! # 继承 `node2`（`#[rec(extends = node2)]`）
//!
//! 节点字段（`width` / `padding` / `layout` / `children` / `exts` …）由宏内联转发进
//! 自动注入的 `rest: Node2Props`，调用处写 `rect_node! { background: .., width: .. }`，
//! 与 `node2!` 完全同构。节点句柄是**普通参数** `node_ref`（对齐 TS `arg.nodeRef`），
//! 不传就内部新建、直接返回 `Fc`，不再有 `.of()`。

use wy_render::{Color, Rect, Scene};

use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::node2::Node2;
use crate::{rec, Node2Props};

/// 矩形容器（对标 TS `RectNode.ts`）。
///
/// 绘制语义（贴近 CSS 盒子，与 TS 逐行一致）：
/// - 背景铺满除边框外的整个盒子（含 padding 区）；
/// - 边框沿盒子外沿描边，线宽居中（内缩 `borderWidth/2` 保证 crisp）；
/// - `radius > 0` 时背景与边框都走圆角路径；
/// - 最后调用 `rest.draw`（TS 的 `rest.draw?.(ctx2, n)`）。
#[rec(extends = node2)]
#[must_use]
pub fn rect_node(
    /// 背景色；getter 返回 `None` 表示不填充。
    #[getter]
    background: Option<Color>,
    /// 边框色；getter 返回 `None` 表示不描边。
    #[getter]
    border_color: Option<Color>,
    /// 边框宽度（线宽居中于盒子外沿），默认 1。
    #[getter]
    #[props(default = 1.0f32)]
    border_width: f32,
    /// 圆角半径，默认 0。
    #[getter]
    #[props(default = 0.0f32)]
    radius: f32,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）：**普通参数**，不传则内部新建、
    /// 调用方拿不到该节点；需要句柄时显式传一个 `Ref2<Node2>`。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    // TS: `draw(ctx2, n) { ...; rest.draw?.(ctx2, n); }`——先把继承来的 draw 摘出来
    // （只 clone 这一个字段，`..rest` 仍按值展开），自己的画完再调它。
    let rest_draw = rest.draw.clone();
    node2! {
        ..rest,
        node_ref: node_ref.clone(),
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            if w > 0.0 && h > 0.0 {
                let bw = border_width();
                let bg = background();
                let bc = border_color();
                let has_border = bc.is_some() && bw > 0.0;
                let r = radius();
                // 边框线宽居中：盒子外沿内缩 bw/2。
                let (x, y, bw2, bh2) = if has_border {
                    (bw / 2.0, bw / 2.0, (w - bw).max(0.0), (h - bw).max(0.0))
                } else {
                    (0.0, 0.0, w, h)
                };
                if let Some(bg) = bg {
                    let rect = Rect::new(x, y, bw2, bh2);
                    if r > 0.0 {
                        s.fill_round_rect(rect, r, bg);
                    } else {
                        s.fill_rect(rect, bg);
                    }
                }
                if has_border {
                    if let Some(bc) = bc {
                        let rect = Rect::new(x, y, bw2, bh2);
                        s.stroke_round_rect(rect, r.max(0.0), bc, bw);
                    }
                }
            }
            if let Some(f) = &rest_draw {
                f(s, n);
            }
        },
    }
}
