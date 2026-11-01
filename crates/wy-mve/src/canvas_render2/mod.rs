//! canvasRender2：复刻 TS `mve-dom-helper/canvasRender2` 的 2 代节点渲染框架。
//!
//! 节点类型（rect / wrappedText / richText / forEach / if / tabRender）、顶层入口
//! `render_canvas2` 与跨组件选区管理器 `SelectionManager2` 均已落地，全部复用
//! `wy-mve` 现有引擎（`Node` 渲染/事件管线、`wy_layout` flex、`wy_render` 绘制
//! 后端），而非另起一套平行渲染器。
//!
//! 设计原则：节点声明走 [`node2::Node2`]（对标 TS `Node2` / `Node2Arg`）——
//! 几何与焦点属性全是读取式，布局配置走 `layout: flex({..})`，伸展/对齐走
//! `exts: [grow(..), align(..)]`；`renderCanvas` 映射到 [`render_canvas2`]。

/// 基础元素组件 `node2`（对标 TS `Node.ts` 的 `node2` / `Node2Arg`）。
///
/// 这是新体系唯一的"万能元素"：所有容器、文本、自绘节点都从它派生。
// `#[macro_use]`：`node2!` 由 `#[rec]` 展开产生，**宏展开出来的 `#[macro_export]`
// 宏不能按绝对路径引用**（`use crate::node2;` / `crate::node2!` 都会被 rustc 拒绝），
// 所以本 crate 内部只能靠文本作用域把它提上来。`element` 必须排在**所有**使用
// `node2!` 的组件（editable_text_node / rect / rich_text / scroll / popover …）之前。
#[macro_use]
pub mod element;
/// 帧时钟与宿主唤醒登记（对标 TS 里 `requestAnimationFrame` 那一小块）。
pub mod animation;
pub mod clipboard;
pub mod editable_text_node;
pub mod fc;
/// 焦点真相源 / 键盘事件 / 编辑器句柄（对标 TS `Focus.ts`）。
pub mod focus;
pub mod geometry;
/// 词边界（对标 TS `Graphemes.ts` 的 `Words`；字素簇边界由编辑内核
/// `wy_text::EditCore` 提供，组件层不再留第二份）。
pub mod graphemes;
/// 图片节点（对标 TS `ImageNode.ts`）。
pub mod image_node;
/// 节点模型：`Node2` / `Node2Props` / `exts` / `flex`（对标 TS `Node.ts`）。
///
/// 同样 `#[macro_use]`：`flex!` 也是 `#[rec]` 产物，本 crate 内部只能用文本作用域。
#[macro_use]
pub mod node2;
pub mod nodes;
/// macOS 风格浮层滚动条（对标 TS `OverlayScrollBar.ts`）。
pub mod overlay_scroll_bar;
/// 浮层放置（对标 TS `canvasCore/Placement.ts`）：纯几何，锚矩形 → 面板坐标，
/// 带空间不足自动翻转与贴边。
pub mod placement;
/// 浮层组件 `dialog` / `dropdown`（对标 TS `helper/Popover.ts`  + 旧 v1 `overlay.rs`
/// 的两种形态）。
pub mod popover;
/// 节点消息协议（对标 TS `tree/protocol.ts`）：`MouseEvent2` / `MouseType2` /
/// `PointerDevice` / `KeyEvent2` / `EditorHandleExt` 等三侧共用的契约载体。
pub mod protocol;
/// 矩形容器（对标 TS `RectNode.ts`）。
pub mod rect;
pub mod renderer;
/// 富文本节点（对标 TS `RichTextNode.ts`）。
pub mod rich_text;
pub mod scroll;
pub mod selection;
pub mod tab;
pub mod text_defaults;
pub mod text_metric;
/// 树引擎（逐文件对标 TS `canvasRender2/tree/`）。
pub mod tree;
pub mod url_opener;
pub mod util;
/// 单片段文本节点（对标 TS `WrappedTextNode.ts`）。
pub mod wrapped_text;

pub use animation::{now_ms, take_due_frame, time_until_next_frame, Fade, FrameTick, FRAME_MS};
pub use editable_text_node::{editable_text_node, EditableTextNodeArg, CARET_BLINK_MS};
pub use element::node2;
// `Node2Props` **就是**节点参数（TS `Node2Arg`）：`node2!` 组装出来的那一份，
// `Node2::build` 直接吃它，`extends = node2` 的组件内部也靠它做
// `node2! { ..rest, .. }` 展开。没有第二层 IR，所以它不是"中间件"、不必藏。
pub use element::{Node2Props, Node2PropsBuilder};
pub use fc::{build_children, Fc, NodeApply, Ref2};
pub use focus::{
    active_editor, active_editor_ext, apply_modifiers, clear_focus, clear_modifiers,
    editor_handle_ext, focused_id, focused_node, is_focused, is_pressed, modifiers,
    notify_key_press, pressed_node, register_key_press, request_focus, set_pressed,
    EditableTextHandle, InputOverlayRect, KeyCode2, KeyEvent2, Modifiers,
};
pub use geometry::{
    append_rounded_rect, fill_inner_rect, fill_outer_rect, union_rect, Path2, PathCommand2, PointF,
    RectF,
};
pub use graphemes::Words;
pub use image_node::{image_node, ImageNodeArg};
pub use node2::{
    absolute_in_inner, align, align_xy, always, axis_of, build_root_fc, build_root_node2,
    expand_node2, find_ext, flex, get, grow, grow_dyn, grow_xy, ignore_flex, ignore_flex_dyn,
    layout_size_direction, size, AlignChild, Axis, Ext, Flex, GrowChild, IgnoreFlexChild, ImeExt,
    LayoutSize, LayoutSizeDirection, Node2, Node2Weak, StartEnd,
};
pub use nodes::{for_each_node, if_node, render_canvas2};
pub use overlay_scroll_bar::{overlay_scroll_bar, OverlayScrollBarArg, ScrollBarStyle2};
pub use placement::{resolve_placement, Placement, PlacementRect, PlacementSize};
pub use popover::{dialog, dropdown};
pub use protocol::{
    create_mouse_event2, pointer_button_mask, EditorHandleExt, MouseEvent2, MouseEventInit2,
    MouseType2, PointerDevice, POINTER_BUTTON_LEFT, POINTER_BUTTON_MIDDLE, POINTER_BUTTON_RIGHT,
    POINTER_LEFT_DOWN_MASK, POINTER_MIDDLE_DOWN_MASK, POINTER_RIGHT_DOWN_MASK,
};
pub use rect::{rect_node, RectNodeArg};
pub use renderer::{
    create_click_counter, dispatch_global_mouse_down, dispatch_global_mouse_move,
    dispatch_global_mouse_up, drag_mouse, register_down, register_move, register_up,
};
pub use rich_text::{rich_text_node, RichTextNodeArg, RichTextSpan};
pub use scroll::{
    content_size, create_scroll, is_scroll_content_node, max_scroll, scroll_bar, scroll_content,
    scroll_content_ext, scroll_wheel, ActivityListener, Scroll2, ScrollBarArg, ScrollBarCalc,
    ScrollContentArg, ScrollContentExt,
};
pub use selection::{
    expand_word_selection, HitestItem2, HitestResult2, PointerSelect2, Rect2, SelPair2, SelPoint2,
    Selectable2, SelectionManager2, SelectionSource2,
};
pub use tab::{expand_tabs, SPACES_PER_TAB};
pub use text_defaults::{placeholder_color, set_placeholder_color, set_text_color, text_color};
pub use text_metric::ink_shift_y_of;
pub use url_opener::{open_url, set_url_opener};
pub use util::MAX_LAYOUT_WIDTH;
pub use wrapped_text::{wrapped_text_node, WrappedTextNodeArg};
