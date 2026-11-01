//! 布局工具（对标 TS `canvasRender2/util.ts`）。
//!
//! 仅 `MAX_LAYOUT_WIDTH` 被编辑器使用；`fillOuterRect` / `fillInnerRect` 一并补齐，与 TS 同名函数同构。

use wy_render::{Color, Rect, Scene};

use crate::canvas_render2::node2::Node2;

/// 布局用的"无限宽"：CanvasKit 按 float32 解释，Number.MAX_VALUE 会溢出成
/// Infinity，参与减法/对齐后产出 NaN。取 f32 可表示的最大有限值
/// （= TS `3.4028234663852886e38`）。
pub const MAX_LAYOUT_WIDTH: f32 = f32::MAX;

/// 填充节点外包矩形。
pub fn fill_outer_rect(node: &Node2, s: &mut Scene, color: Color) {
    s.fill_rect(Rect::new(0.0, 0.0, node.width(), node.height()), color);
}

/// 填充节点内容（inner）矩形。
pub fn fill_inner_rect(node: &Node2, s: &mut Scene, color: Color) {
    let px = node.padding_inline_start();
    let py = node.padding_block_start();
    let w = (node.width() - px - node.padding_inline_end()).max(0.0);
    let h = (node.height() - py - node.padding_block_end()).max(0.0);
    s.fill_rect(Rect::new(px, py, w, h), color);
}
