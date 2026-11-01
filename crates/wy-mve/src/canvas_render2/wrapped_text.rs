//! 单片段文本节点（对标 TS `canvasRender2/WrappedTextNode.ts`）。
//!
//! TS 原文把 `text` + 样式参数合成**一个 span** 委托给 `richTextNode`，
//! 因此自动获得 `richTextNode` 的全部能力（自动换行、`autoWidth`、
//! 跨组件文本选择、双击选词等）：
//! ```ignore
//! const { text = () => '', fontFamily = () => null, fontSize = () => 16,
//!         fontWeight = () => 400, italic = () => false, color = () => textDefaults.text,
//!         lineHeightMultiplier = () => 1.4, letterSpacing = () => 0, wordSpacing = () => 0,
//!         spans, ...rest } = arg;
//! richTextNode.create({
//!   ...rest,
//!   spans: () => spans?.() ?? [{ text: text(), style: {..} }],
//! })(node)(ctx);
//! ```
//! 这里逐句对照——**本节点只做参数合成，节点身份由 `richTextNode` 创建**。
//!
//! # 继承的是 `rich_text_node`，不是 `node2`
//!
//! TS 的声明就是这个意思：
//!
//! ```text
//! interface WrappedTextNodeArg extends Omit<RichTextNodeArg, 'spans'> { .. }
//! ```
//!
//! 也就是说，「节点参数」这一层（`width` / `padding` / `layout` / `spans` /
//! `auto_width` / `no_wrap` / `max_lines` / `ellipsis` / `text_align` /
//! `selection_color` / `selection_manager` / `node_ref` …）**全部继承**自
//! `richTextNode`，`node2` 那一层只是它再往上的一级。写成
//! `#[rec(extends = node2)]` 会丢掉中间这一层：`no_wrap`、`node_ref` 这类字段
//! 就没入口了（早期实现只手抄了 6 个「透传」字段，正好漏了 `no_wrap`）。
//!
//! 所以这里写 `#[rec(extends = rich_text_node)]`：`rest` 的类型是
//! `RichTextNodeArg`，`!` 宏把 key 分三层转发（自有 → `RichTextNodeArg` →
//! `Node2Props`，见 `wy-mve-macros/src/rec/extends.rs`），调用语法与
//! `rich_text_node!` 完全同构、且多出一套文本样式简写：
//!
//! ```ignore
//! wrapped_text_node! {
//!     text: "标题",
//!     font_size: 13.0,
//!     color: TEXT,
//!     no_wrap: true,       // ← 继承自 richTextNode 的字段，直接可用
//!     width: 200.0,        // ← 再往上一级（node2）的字段，同样直接可用
//! }
//! ```
//!
//! 唯一的语义差异是 `spans`：TS 把它从基类里 `Omit` 掉再重新声明为**可空**
//! （nil = 由 text + 样式合成一段），并在函数体里 `{ ...rest, spans: .. }` 落地。
//! Rust 一一对应写成 `#[rec(extends = rich_text_node, omit(spans))]` + 自己再声明
//! 一个 `spans` 参数：宏不再把它转发进 `RichTextNodeArg::builder()`，`rest.spans`
//! 只剩函数体这一个写入点。

use std::rc::Rc;

use wy_render::Color;
use wy_text::{TextDecoration, TextStyle};

use crate::canvas_render2::fc::Fc;
use crate::canvas_render2::node2::{always, Get};
use crate::canvas_render2::rich_text::{RichTextNodeArg, RichTextSpan};
use crate::canvas_render2::text_defaults::text_color;

use crate::rec;

/// 单片段文本节点（对标 TS `wrappedTextNode`）。
///
/// 自己只声明**文本样式**这一层（全部 `#[getter]`，写表达式、宏包 `move ||`）：
/// 没写的回落到 TS 同款中性默认（字号 16、字重 400、颜色 `text_color`、行高 1.4）。
/// 节点参数（`width` / `padding` / `layout` / `no_wrap` / `max_lines` /
/// `ellipsis` / `text_align` / `selection_*` / `node_ref` …）一个都不重抄：它们经
/// 宏转发进自动注入的 `rest: RichTextNodeArg`，最后整体交给 `rich_text_node`。
///
/// 只有 `spans` 例外——它与 TS 一样从基类 `omit` 掉，自己重新声明（可空），
/// 由函数体决定「写的优先，没写就由 text + 样式合成一段」。
#[rec(extends = rich_text_node, omit(spans))]
// 参数就是 TS `WrappedTextNodeArg` 去掉继承部分后的逐字段摊平，拆结构体只会多一层。
#[allow(clippy::too_many_arguments)]
pub fn wrapped_text_node(
    /// 文本内容，默认 `''`。
    #[getter]
    text: String,
    /// 字体族，默认 `None`（系统默认）。
    #[getter]
    #[props(default = None)]
    font_family: Option<String>,
    /// 字号，默认 16。
    #[getter]
    #[props(default = 16.0f32)]
    font_size: f32,
    /// 字重，默认 400。
    #[getter]
    #[props(default = 400u16)]
    font_weight: u16,
    /// 斜体，默认 `false`。
    #[getter]
    #[props(default = false)]
    italic: bool,
    /// 颜色，默认 [`text_color`]（核心中性默认色）。
    #[getter]
    color: Color,
    /// 行高倍数，默认 1.4（`None` = 用排版默认）。
    #[getter]
    #[props(default = Some(1.4f32))]
    line_height_multiplier: Option<f32>,
    /// 字距，默认 0。
    #[getter]
    #[props(default = 0.0f32)]
    letter_spacing: f32,
    /// 词距，默认 0。
    #[getter]
    #[props(default = 0.0f32)]
    word_spacing: f32,
    /// 整段片段（**从基类 `omit` 掉的字段，这里重新声明**）。
    ///
    /// 写了就以它为准、`text` 被忽略；没写才由上面的 text + 样式合成一段——
    /// 即 TS 的 `spans: () => spans?.() ?? [singleSpan()]`。
    #[getter]
    spans: Vec<RichTextSpan>,
) -> Fc {
    let text = text.unwrap_or_else(|| always(String::new()));
    let color = color.unwrap_or_else(|| Rc::new(text_color));

    // TS `p.spans() ?? [singleSpan()]`：只有**没给**片段时才由单文本参数合成一段。
    // `spans` 已经从基类 `omit` 掉（见上方 `#[rec]`），所以 `rest.spans` 只有这
    // 一个写入点；调用方写 `spans:` 落在这里，`..rest` 带进来的那份被这里覆盖——
    // 与 `{ ...rest, spans: () => spans?.() ?? .. }` 同序同义。
    let spans: Get<Vec<RichTextSpan>> = spans.unwrap_or_else(|| {
        Rc::new(move || {
            vec![RichTextSpan {
                text: text(),
                style: TextStyle {
                    font_family: font_family(),
                    font_size: font_size(),
                    font_weight: font_weight(),
                    italic: italic(),
                    color: color().to_u32(),
                    letter_spacing: letter_spacing(),
                    word_spacing: word_spacing(),
                    line_height_multiplier: line_height_multiplier(),
                    decoration: TextDecoration::default(),
                },
                url: None,
            }]
        })
    });

    let mut rest = rest;
    rest.spans = Some(spans);

    // 节点身份由 `rich_text_node` 创建：补好 `spans` 的整份参数直接交给它
    // （`RichTextNodeArg::build()` 就是那个函数本身，宏不会做任何额外注入）。
    rest.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::fc::Ref2;
    use crate::canvas_render2::node2::build_root_fc;
    use crate::canvas_render2::selection::as_selectable2;
    use crate::Node2;

    /// 一段远宽于 120px 的文本：折行时会变成多行。
    const LONG: &str = "被继承的超长单行文本，即便盒宽只有 120px 也不折行，只裁剪";

    /// 造一份「父组件传下来的 props」：`..rest` 的载荷。
    ///
    /// `rich_text_node!` 产出的是**节点**（`Fc`），不产 props，所以这类测试只能用
    /// 结构体字面量写一份数据——一次性表达式，不是 `RichTextNodeArg::builder()` 链。
    fn rest_with_spans(text: &'static str) -> RichTextNodeArg {
        let spans: crate::canvas_render2::node2::Get<Vec<RichTextSpan>> =
            Rc::new(move || vec![RichTextSpan::text(text, wy_render::Color::BLACK)]);
        RichTextNodeArg {
            spans: Some(spans),
            ..Default::default()
        }
    }

    /// 建一棵「根 → 单个 wrapped_text」的最小树，返回根与句柄槽。
    fn root_with(no_wrap: bool, slot: &Ref2<Node2>) -> Node2 {
        let slot = slot.clone();
        build_root_fc(node2! {
            width: 200.0,
            height: 200.0,
            children: vec![wrapped_text_node! {
                text: LONG,
                font_size: 14.0,
                // ↓ 以下全是 `rich_text_node` 的字段（`extends = node2` 时写不出来）
                no_wrap: no_wrap,
                max_lines: usize::MAX,
                ellipsis: "…",
                text_align: wy_text::TextAlign::Start,
                auto_width: false,
                width: 120.0,
                node_ref: slot.clone(),
            }],
        })
    }

    /// **继承的是 `rich_text_node`，不是 `node2`**。
    ///
    /// `no_wrap` / `max_lines` / `ellipsis` / `text_align` / `auto_width` /
    /// `node_ref` 这些 key 只存在于 `RichTextNodeArg` 上。本测试既验证它们能被
    /// `wrapped_text_node!` 接受（能编过即证明转发到位），也验证真的生效：
    /// `no_wrap` 为真时同一段文本单行（比折行的那份矮）。
    #[test]
    fn inherits_rich_text_params_not_just_node2() {
        let narrow = Ref2::<Node2>::new();
        let wrapped = Ref2::<Node2>::new();
        root_with(true, &narrow);
        root_with(false, &wrapped);

        let single = narrow.get().height();
        let multi = wrapped.get().height();
        assert!(
            multi > single * 1.5,
            "同一段文本：折行({multi})应明显高于 no_wrap 单行({single})"
        );
    }

    /// `node_ref` 也走继承：拿到的是**树上那个真节点**，不是内部另建的。
    #[test]
    fn inherited_node_ref_points_at_the_tree_node() {
        let slot = Ref2::<Node2>::new();
        let root = root_with(true, &slot);
        let node = slot.get();
        assert_eq!(
            node.parent().map(|p| p.id()),
            Some(root.id()),
            "node_ref 必须指向挂在树上的那个节点"
        );
    }

    /// 没写 `spans` 时由 text + 样式合成一段（TS `p.spans() ?? [singleSpan()]`）。
    #[test]
    fn synthesizes_one_span_when_spans_absent() {
        let slot = Ref2::<Node2>::new();
        root_with(true, &slot);
        let node = slot.get();
        let sel = as_selectable2(&node).expect("富文本节点自带可选能力");
        assert_eq!(sel.text_length(), LONG.len(), "合成片段就是整段 text");
        assert_eq!(sel.text_in_range(0, LONG.len()), LONG);
    }

    /// 写了 `spans` 就以它为准（**同一条继承通道**，不是另开的参数）。
    ///
    /// 这条同时是 `omit(spans)` 的回归门：若 `spans` 没有从基类里省掉，调用处写的
    /// `spans:` 会被塞进 `rest.spans`，而函数体无条件写 `rest.spans = 合成` 会把它
    /// 冲掉 —— 断言就变成「不该出现」了。
    #[test]
    fn explicit_spans_wins_over_text() {
        let slot = Ref2::<Node2>::new();
        let slot_in = slot.clone();
        build_root_fc(node2! {
            width: 200.0,
            height: 100.0,
            children: vec![wrapped_text_node! {
                text: "不该出现",
                spans: vec![RichTextSpan::text("显式片段", wy_render::Color::BLACK)],
                node_ref: slot_in.clone(),
            }],
        });
        let node = slot.get();
        let sel = as_selectable2(&node).expect("富文本节点自带可选能力");
        // 偏移是**字节**空间（与 Parley 一致），别写 `0..4`（会切在汉字中间）
        assert_eq!(sel.text_in_range(0, sel.text_length()), "显式片段");
    }

    /// 自有 `spans` 优先于 `..rest` 带来的那一份（TS `{ ...rest, spans: .. }`）。
    #[test]
    fn own_spans_wins_over_rest_spans() {
        let base = rest_with_spans("来自 rest");
        let slot = Ref2::<Node2>::new();
        let slot_in = slot.clone();
        build_root_fc(node2! {
            width: 200.0,
            height: 100.0,
            children: vec![wrapped_text_node! {
                ..base,
                text: "更不该出现",
                spans: vec![RichTextSpan::text("自有", wy_render::Color::BLACK)],
                node_ref: slot_in.clone(),
            }],
        });
        let sel = as_selectable2(&slot.get()).expect("富文本节点自带可选能力");
        assert_eq!(sel.text_in_range(0, sel.text_length()), "自有");
    }

    /// `Omit` 之后 `rest.spans` 这条车道是**死的**：`..rest` 里哪怕带着片段，
    /// 也以「自己的 spans 或合成的那一段」为准（TS 的类型根本不允许它存在）。
    #[test]
    fn rest_spans_is_ignored_under_omit() {
        let base = rest_with_spans("来自 rest");
        let slot = Ref2::<Node2>::new();
        let slot_in = slot.clone();
        build_root_fc(node2! {
            width: 200.0,
            height: 100.0,
            children: vec![wrapped_text_node! {
                ..base,
                text: "由 text 合成",
                node_ref: slot_in.clone(),
            }],
        });
        let sel = as_selectable2(&slot.get()).expect("富文本节点自带可选能力");
        assert_eq!(sel.text_in_range(0, sel.text_length()), "由 text 合成");
    }

    /// `..rest` 在二级继承下展开成基类 builder 的 `.from_props($r)`。
    ///
    /// `node_ref` 是**没被 omit 掉**的基类字段：它必须照样能被 `..rest` 送进去
    /// （与上一条「被 omit 的那格失效」互为对照——省一个字段不等于关掉整条通道）。
    #[test]
    fn rest_of_wrapped_text_seeds_the_base_args() {
        let slot = Ref2::<Node2>::new();
        let base = RichTextNodeArg {
            node_ref: slot.clone(),
            ..Default::default()
        };
        build_root_fc(node2! {
            width: 200.0,
            height: 100.0,
            children: vec![wrapped_text_node! {
                ..base,
                text: "来自 rest 的句柄",
            }],
        });
        assert!(slot.is_set(), "`..rest` 里的基类字段（node_ref）仍要生效");
        let sel = as_selectable2(&slot.get()).expect("富文本节点自带可选能力");
        assert_eq!(sel.text_in_range(0, sel.text_length()), "来自 rest 的句柄");
    }
}
