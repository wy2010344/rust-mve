//! `Node2` 的构建：一份 [`Node2Props`] → 数据槽，以及把子节点挂进父容器。

use std::rc::Rc;

use crate::canvas_render2::element::Node2Props;
use crate::{Layout, NodeContext};

use super::mount::{current_parent, ParentScope};
use super::*;

impl Node2 {
    /// 用声明参数填充节点：一份 [`Node2Props`] → 数据槽。
    ///
    /// 对应 TS `node2 = tfc((ctx, node, arg) => { ... })` 里 `Object.assign` 那步。
    ///
    /// 参数就是 `node2` 组件的 props（`node2!` 组装出来的那一份），不再是某个
    /// "内部 IR"：只有一层，`extends = node2` 的组件写 `node2! { ..rest, .. }`
    /// 就能把继承来的字段原样传进来再覆盖。
    ///
    /// 三件"加工"收在这里（声明层因此一个都不用写）：
    /// - `padding` 是四向简写：某一向没单独声明时回落到它；
    /// - `ime` / `ime_cursor` 不在 props 里，从 `exts` 的 [`ImeExt`] 取；
    /// - `node_ref` 是组件槽、`children` 是子节点，都不属于节点字段。
    #[must_use]
    pub fn build(self, props: Node2Props) -> Self {
        let Node2Props {
            node_ref: _,
            children,
            width,
            height,
            size,
            x,
            y,
            position,
            padding,
            padding_inline_start,
            padding_inline_end,
            padding_block_start,
            padding_block_end,
            layout,
            exts,
            hide,
            not_in_layout,
            selection_enabled,
            skip_draw,
            focusable,
            focus_order,
            focus_trap,
            on_focus,
            on_blur,
            on_hover,
            accept_hit,
            accept_clip,
            cursor,
            draw,
            draw_at_parent,
            handle_key,
            mouse_click,
            mouse_click_capture,
            mouse_down,
            mouse_down_capture,
            mouse_up,
            mouse_up_capture,
            mouse_move,
            mouse_move_capture,
            mouse_wheel,
            mouse_wheel_capture,
            mouse_double_click,
            mouse_double_click_capture,
            mouse_context_menu,
            mouse_context_menu_capture,
            clip,
            clip_to_parent,
            debug_name,
        } = props;

        // 四向简写：某一向没单独声明时回落到 `padding`（TS 里
        // `paddingInlineStart ?? padding` 的逐向展开）。
        let fill = |side: Option<Get<f32>>| side.or_else(|| padding.clone());
        let ime_ext = find_ext::<ImeExt>(&exts);
        let ime = ime_ext.map(|e| Rc::clone(&e.on_ime));
        let ime_cursor = ime_ext.map(|e| Rc::clone(&e.cursor));

        {
            let mut d = self.0.data.borrow_mut();
            d.width = width;
            d.height = height;
            d.size = size;
            d.x = x;
            d.y = y;
            d.position = position;
            d.padding_inline_start = fill(padding_inline_start);
            d.padding_inline_end = fill(padding_inline_end);
            d.padding_block_start = fill(padding_block_start);
            d.padding_block_end = fill(padding_block_end);
            d.layout = layout;
            d.hide = hide;
            d.not_in_layout = not_in_layout;
            d.selection_enabled = selection_enabled;
            d.skip_draw = skip_draw;
            d.focusable = focusable;
            d.focus_order = focus_order;
            d.focus_trap = focus_trap;
            d.accept_hit = accept_hit;
            d.accept_clip = accept_clip;
            d.cursor = cursor;
            d.draw = draw;
            d.draw_at_parent = draw_at_parent;
            d.handle_key = handle_key;
            d.ime = ime;
            d.ime_cursor = ime_cursor;
            d.mouse_click = mouse_click;
            d.mouse_click_capture = mouse_click_capture;
            d.mouse_down = mouse_down;
            d.mouse_down_capture = mouse_down_capture;
            d.mouse_up = mouse_up;
            d.mouse_up_capture = mouse_up_capture;
            d.mouse_move = mouse_move;
            d.mouse_move_capture = mouse_move_capture;
            d.mouse_wheel = mouse_wheel;
            d.mouse_wheel_capture = mouse_wheel_capture;
            d.mouse_double_click = mouse_double_click;
            d.mouse_double_click_capture = mouse_double_click_capture;
            d.mouse_context_menu = mouse_context_menu;
            d.mouse_context_menu_capture = mouse_context_menu_capture;
            d.on_focus = on_focus;
            d.on_blur = on_blur;
            d.on_hover = on_hover;
            d.exts = exts;
            d.clip = clip;
            d.clip_to_parent = clip_to_parent;
            d.debug_name = debug_name;
            d.child_fcs = children;
        }
        // # 认父为什么**不**在这里做
        //
        // TS `node2` 体里那句 `const parent = ctx.getParent(); node.parent = parent`
        // 是在**拿到 ctx 的那一刻**执行的，也就是挂载期。Rust 这边 `build` 只是
        // "把 Node2Props 灌进空壳"，真正挂载是 [`Self::mount`]——两件事必须分开：
        //
        // `rect_node(..)` 这类工厂会在**父节点的 `children` 表达式里**就把
        // 节点造出来（工厂体立即执行、直接返回 `Fc`）。那一刻父节点自己还没进入
        // `ParentScope`，若这里就地认父，取到的是**祖父**，子节点会被挂到错误的
        // 层级上（按钮里的文字跑到整行里，就是这条）。把认父留给 `mount`，
        // 时序就与 TS 一致：谁在挂载、谁就是父。
        self
    }

    /// 展开子节点（在父上下文中），并把它们登记进 `children`。
    pub(super) fn mount_children(&self, cx: &mut NodeContext) {
        let fcs = {
            let mut d = self.0.data.borrow_mut();
            std::mem::take(&mut d.child_fcs)
        };
        if fcs.is_empty() {
            return;
        }
        let _scope = ParentScope::push(self.clone());
        // 同步把父指针挂进上下文：动态区域（for_each / when）的 creater 在
        // 惰性求值时才跑，线程局部栈那时已弹空——creater 的子上下文靠
        // `cx.node2_parent` 认父（见 `NodeContext::node2_parent`）。
        let prev_parent = cx.node2_parent.replace(self.clone());
        for fc in fcs {
            fc.mount(cx);
        }
        cx.node2_parent = prev_parent;
    }

    /// 挂到 `cx` 当前父节点下（TS `node2` 体里那两句
    /// `node.parent = ctx.getParent(); ctx.addNode(node)`）。
    ///
    /// 认父**不能**在 [`Self::build`] 里做：`rect_node(..)` 这类工厂会在父节点的
    /// `children` 表达式里就把节点造出来，那一刻父节点自己还没进入 `ParentScope`，
    /// 就地认父会取到**祖父**，子节点被挂到错误的层级上。挂载期才是"谁在挂、谁
    /// 就是父"。
    ///
    /// 优先取上下文携带的父（动态区域 creater 走这条），退回线程局部栈
    /// （`build` 在父作用域内直接调用时走这条）。
    pub fn mount(self, cx: &mut NodeContext) {
        if self.parent().is_none() {
            let parent = cx.node2_parent.clone().or_else(current_parent);
            if let Some(parent) = parent {
                self.attach_to(&parent);
            }
        }
    }
}

/// `Layout`（主轴方向）→ [`Axis`]：`Row` = x 主轴，`Column` = y 主轴。
#[must_use]
pub fn axis_of(layout: Layout) -> Axis {
    match layout {
        Layout::Row => Axis::X,
        Layout::Column => Axis::Y,
    }
}
