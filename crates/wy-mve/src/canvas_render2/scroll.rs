//! 滚动（纯 `Node2`，逐句对标 TS `canvasRender2/Scroll.ts` + `ScrollBar.ts` + `OverlayScrollBar.ts`）。
//!
//! TS 只有三个原语（外加内容节点自带的 `scrollWheel`），**没有**"一行搞定滚动
//! 容器"的封装，也**没有**单独的滚轮注册器——`scrollContent` / `scrollBar` 自己
//! 把 `scrollWheel(scroll)` 挂成 `mouseWheel` 消息，沿命中链派发（内层先滚、
//! 被 `stopPropagation` 拦住则外层不再滚）：
//!
//! | TS | Rust |
//! |---|---|
//! | `createScroll(container, direction[, value])` | [`create_scroll`] |
//! | `scrollWheel(scroll)`（挂在 `scrollContent` / `scrollBar` 的 `mouseWheel`） | [`scroll_wheel`] |
//! | `scrollContent({ scroll, ...rest })` | [`scroll_content`] |
//! | `scrollBar({ scroll, ...rest })` | [`scroll_bar`] |
//! | `contentSize` / `maxScroll` | [`content_size`] / [`max_scroll`] |
//!
//! 容器由调用方自己拼——这正是 TS 的做法，也是"滚动容器方向/内边距/间距"
//! 这些**业务属性**该由业务声明的地方。
//!
//! # 组成方式（与 TS 示例同构）
//!
//! ```ignore
//! // 容器：纵向滚动 → 布局取 Row（滚动轴落在交叉轴，见下）
//! node2(Node2Arg {
//!     layout: Some(flex().direction(Layout::Row).align_item(AlignItem::Stretch)),
//!     clip: true,                       // 子节点溢出裁到 inner 区
//!     children: vec![content, bar],
//!     ..
//! })
//! ```
//!
//! # 为什么滚动偏移落在**交叉轴**
//!
//! 引擎的 `x_fn`/`y_fn` 是**叠加在布局盒上的偏移**，而 TS 的 `position(d)`
//! 是**整体覆盖**布局位置；两者只在"该轴没有其它布局偏移"时等价。纵向滚动时
//! 把容器布局取 `Row`（主轴 X），内容沿 Y 的布局位置恒为 `padding_start`，
//! 于是 `y = -value` 就等价于 TS 的 `position('y') = pad - value`。
//! TS 侧的真实用法（`apps/mve-vite-demo/.../canvas-v2-demo`）也是这么写的：
//! `layout: flex({ direction: 'x' })` + `y: () => -scroll.value()`。
//!
//! # 裁剪
//!
//! TS 在 `scrollContent.drawAtParent` 里 `clip()` 到**父的 inner 区**。
//! 引擎里等价物是容器上的 `clip`：绘制期先画容器自身、再 `push_clip(inner)`
//! 夹住子节点、出栈（`mve_integration::draw_node`），命中测试同样按 inner 区
//! 门控（`in_layout_box`）——对应 TS 的 `acceptClip`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wy_render::{Color, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::node2::{always, get, Axis, Ext, Node2};
use crate::canvas_render2::protocol::MouseEvent2;
use crate::canvas_render2::renderer::drag_mouse;
use crate::{rec, Node2Props};

/// 滑块最短长度（TS `Math.max(20, ..)`）。
const MIN_THUMB: f32 = 20.0;
/// 滚动条交叉轴厚度默认值（TS `thickness = () => 10`）。
const DEFAULT_THICKNESS: f32 = 10.0;
/// 滑块圆角默认值（TS `thumbRadius = () => 5`）。
const DEFAULT_THUMB_RADIUS: f32 = 5.0;
/// 轨道底色默认值（TS `trackColor = () => rgba(0, 0, 0, 15)`）。
const TRACK_COLOR: Color = Color::rgba(0, 0, 0, 15);
/// 滑块底色默认值（TS `thumbColor = () => rgba(120, 120, 120, 180)`）。
const THUMB_COLOR: Color = Color::rgba(120, 120, 120, 180);

/// 滚动内容扩展标记（TS `ScrollContentExt`）。
///
/// 它与 `GrowChild / AlignChild / IgnoreFlexChild` 走同一套 `exts` 机制，不在
/// `Node2` 上增加 `isScrollContent` 字段。
pub struct ScrollContentExt;

thread_local! {
    /// TS `scrollContentExt` 的共享单例。Rust 的 UI/响应式模型同样是单线程，
    /// 因此用线程局部 `Rc` 保持每个滚动内容节点复用同一实例。
    static SCROLL_CONTENT_EXT: Ext = Rc::new(ScrollContentExt);
}

/// 取共享滚动内容标记（TS `scrollContentExt`）。
#[must_use]
pub fn scroll_content_ext() -> Ext {
    SCROLL_CONTENT_EXT.with(Rc::clone)
}

/// 节点是否为滚动内容（TS `isScrollContentNode`）。
#[must_use]
pub fn is_scroll_content_node(node: &Node2) -> bool {
    node.has_ext::<ScrollContentExt>()
}

// ============================================================================
// 几何：ScrollBarCalculate2 / contentSize / maxScroll
// ============================================================================

/// 滚动条几何（TS `ScrollBarCalculate2`）。
#[derive(Clone, Copy, Debug)]
pub struct ScrollBarCalc {
    /// 滑块尺寸（沿滚动方向）。
    pub size: f32,
    /// 滑块偏移（沿滚动方向）。
    pub offset: f32,
    /// 最大滚动距离。
    pub max_scroll: f32,
    /// 滑块最大偏移（轨道长 − 滑块长）。
    pub max_offset: f32,
}

impl ScrollBarCalc {
    /// 指针位移 → 滚动值增量（TS `moveToScroll`）。
    #[must_use]
    pub fn move_to_scroll(&self, delta: f32) -> f32 {
        if self.max_offset > 0.0 {
            delta * self.max_scroll / self.max_offset
        } else {
            0.0
        }
    }

    /// 滚动值增量 → 指针位移（TS `scrollToMove`）。
    #[must_use]
    pub fn scroll_to_move(&self, delta: f32) -> f32 {
        if self.max_scroll > 0.0 {
            delta * self.max_offset / self.max_scroll
        } else {
            0.0
        }
    }
}

/// 纯几何：`scrollBarSize` 的计算核（不依赖节点树，便于单测）。
///
/// 逐行对应 TS：
/// ```text
/// len = length > 0 ? length : innerSize(container, d)
/// v   = innerSize(container, d)
/// c   = contentSize(container, d)
/// m   = max(0, c - v)
/// if (m > 0 && c > 0) { thumb = max(20, len*v/c); maxOffset = max(0, len - thumb) }
/// ```
#[must_use]
fn compute_scroll_bar(len: f32, v: f32, c: f32, value: f32) -> Option<ScrollBarCalc> {
    let m = (c - v).max(0.0);
    if m > 0.0 && c > 0.0 {
        let thumb = (len * v / c).max(MIN_THUMB);
        let max_offset = (len - thumb).max(0.0);
        Some(ScrollBarCalc {
            size: thumb,
            offset: max_offset * value.clamp(0.0, m) / m,
            max_scroll: m,
            max_offset,
        })
    } else {
        None
    }
}

/// 内容区尺寸：容器里第一个滚动内容子节点的外尺寸（TS `contentSize`）。
///
/// 用 [`crate::canvas_render2::node2::size`] 而不是 `child.geom()`：TS 的
/// `size(child, d)` 是读取式 getter，返回**内容撑出来的自然长度**；布局盒在
/// 纵向滚动里恰好等于视口高（交叉轴 stretch），拿它算 `maxScroll` 永远是 0。
#[must_use]
pub fn content_size(container: &Node2, direction: Axis) -> f32 {
    for child in container.children() {
        if is_scroll_content_node(&child) {
            return crate::canvas_render2::node2::size(&child, direction);
        }
    }
    0.0
}

/// 最大可滚动距离 = 内容尺寸 − 容器 inner 尺寸（负值归零，TS `maxScroll`）。
#[must_use]
pub fn max_scroll(container: &Node2, direction: Axis) -> f32 {
    (content_size(container, direction) - container.inner_size(direction)).max(0.0)
}

// ============================================================================
// 滚动状态（TS `Scroll2` + `createScroll`）
// ============================================================================

/// 活动监听者名单（编号 + 回调）。编号用于注销，回调是副作用方。
type ActivityList = Rc<RefCell<Vec<(u64, Rc<dyn Fn()>)>>>;

/// 滚动状态（TS `Scroll2`）：值钳制在 `[0, maxScroll]`，写入经信号触发重绘。
///
/// 除滚动值之外全是纯派生（内容尺寸 / 滑块几何都现算），唯一另外的可变状态是
/// **活动监听者名单**——那是给浮层滚动条播浮现动画用的副作用通道（TS 原文：
/// "与 `value` 信号的区别是**不参与响应式派生**"），所以不放进信号。
#[derive(Clone)]
pub struct Scroll2 {
    /// 滚动容器（内容与轨道的父节点）。
    container: Node2,
    /// 滚动方向。
    direction: Axis,
    /// 滚动值（写入不钳制，读取时钳制，对应 TS 的 `clamp`）。
    value: Signal<f32>,
    /// 外层滚动（同向嵌套时由内层滚到底后接续消费剩余量，TS `parentScroll`）。
    parent: Option<Rc<Scroll2>>,
    /// 活动监听者（[`Scroll2::add_activity_listener`]）。
    activity: ActivityList,
    /// 下一个监听者编号（用来做注销）。
    next_activity_id: Rc<Cell<u64>>,
}

/// 活动监听句柄：丢弃即注销（TS 里 `addActivityListener` 返回的那个 `EmptyFun`）。
///
/// Rust 用 RAII 而不是返回闭包：漏调注销函数是这类 API 最常见的泄漏来源，
/// `Drop` 让它变成"只要不忘持有"就行。
pub struct ActivityListener {
    list: ActivityList,
    id: u64,
}

impl Drop for ActivityListener {
    fn drop(&mut self) {
        self.list.borrow_mut().retain(|(id, _)| *id != self.id);
    }
}

/// 创建滚动状态（TS `createScroll(container, direction = 'y', value = late(0))`）。
#[must_use]
pub fn create_scroll(container: &Node2, direction: Axis) -> Scroll2 {
    Scroll2 {
        container: container.clone(),
        direction,
        value: Signal::new(0.0),
        parent: None,
        activity: Rc::new(RefCell::new(Vec::new())),
        next_activity_id: Rc::new(Cell::new(0)),
    }
}

impl Scroll2 {
    /// [`create_scroll`] 的带值版本（TS 第三个参数 `value`）。
    #[must_use]
    pub fn with_signal(container: &Node2, direction: Axis, value: Signal<f32>) -> Self {
        Self {
            container: container.clone(),
            direction,
            value,
            parent: None,
            activity: Rc::new(RefCell::new(Vec::new())),
            next_activity_id: Rc::new(Cell::new(0)),
        }
    }

    /// 挂上外层滚动（TS `createScroll(.., parentScroll)` 的第四参）。
    ///
    /// 嵌套滚动：内层滚到边界后，剩余量转交外层，所以滚到底会"自然接力"
    /// 带动外层，而不是卡住不动。
    #[must_use]
    pub fn with_parent(mut self, parent: Scroll2) -> Self {
        self.parent = Some(Rc::new(parent));
        self
    }

    /// 外层滚动（没有则为 `None`）。
    #[must_use]
    pub fn parent_scroll(&self) -> Option<&Scroll2> {
        self.parent.as_deref()
    }

    /// 订阅「滚动活动」：任何来源（滚轮 / 拖滑块 / 程序写值）改变位置后回调一次。
    ///
    /// TS 原文强调这**不是**响应式通道——监听者通常是副作用方（浮层滚动条收到
    /// 通知后浮现并重新计时消隐），不该在渲染过程中被同步读取。
    pub fn add_activity_listener(&self, listener: Rc<dyn Fn()>) -> ActivityListener {
        let id = self.next_activity_id.get();
        self.next_activity_id.set(id + 1);
        self.activity.borrow_mut().push((id, listener));
        ActivityListener {
            list: Rc::clone(&self.activity),
            id,
        }
    }

    /// 通知全部监听者。**先复制一份再遍历**：监听者可能在回调里同步注销自己
    /// （TS 注释里那句 "监听者可能在回调里同步取消订阅"）。
    fn notify_activity(&self) {
        let snapshot: Vec<Rc<dyn Fn()>> = self
            .activity
            .borrow()
            .iter()
            .map(|(_, f)| Rc::clone(f))
            .collect();
        for f in snapshot {
            f();
        }
    }

    /// 滚动方向。
    #[must_use]
    pub fn direction(&self) -> Axis {
        self.direction
    }

    /// 滚动容器。
    #[must_use]
    pub fn container(&self) -> &Node2 {
        &self.container
    }

    /// 当前滚动值（读时钳制，TS `value()`）。
    #[must_use]
    pub fn value(&self) -> f32 {
        self.value.get().clamp(0.0, self.max_scroll(self.direction))
    }

    /// 直接设置（不钳制；读取时钳制，TS `setValue`）。值真的变了才通知活动。
    pub fn set_value(&self, v: f32) {
        let changed = v != self.value.get();
        self.value.set(v);
        if changed {
            self.notify_activity();
        }
    }

    /// 相对滚动，返回**本层**真实滚动量（TS `scroll(delta)`）。
    ///
    /// 本层吃不完的剩余量交给外层。注意通知要自己补：这里绕过 `set_value`
    /// （钳制只算一次），而滚轮走的正是这条路——漏了它浮层滚动条就不会在
    /// 滚动时浮现（TS 源码里专门有一行注释记着这个坑）。
    pub fn scroll(&self, delta: f32) -> f32 {
        let cur = self.value();
        let next = (cur + delta).clamp(0.0, self.max_scroll(self.direction));
        let real = next - cur;
        self.value.set(next);
        if real != 0.0 {
            self.notify_activity();
        }
        let rest = delta - real;
        if rest != 0.0 {
            if let Some(parent) = &self.parent {
                parent.scroll(rest);
            }
        }
        real
    }

    /// 内容尺寸（TS `contentSize(container, d)`）。
    #[must_use]
    pub fn content_size(&self, d: Axis) -> f32 {
        content_size(&self.container, d)
    }

    /// 最大滚动距离（TS `maxScroll(container, d)`）。
    #[must_use]
    pub fn max_scroll(&self, d: Axis) -> f32 {
        max_scroll(&self.container, d)
    }

    /// 滚动条几何；不可滚动时 `None`（TS `scrollBarSize(d, length = 0)`）。
    ///
    /// `length <= 0` 时轨道长取容器 inner 尺寸；显式传值可让轨道与视口不等长
    /// （TS 里 `length` 参数正是为此存在）。
    #[must_use]
    pub fn scroll_bar_size(&self, d: Axis, length: f32) -> Option<ScrollBarCalc> {
        let len = if length > 0.0 {
            length
        } else {
            self.container.inner_size(d)
        };
        let v = self.container.inner_size(d);
        let c = self.content_size(d);
        compute_scroll_bar(len, v, c, self.value())
    }
}

// ============================================================================
// 滚轮消费（TS `scrollWheel`）
// ============================================================================

/// 滚轮沿命中链到达滚动内容/滚动条时由本节点消费（TS `scrollWheel`）。
///
/// 与原 v1 的全局 `registerScroll` 不同：这里**不**自行判定指针是否落在 inner 区——
/// 命中链派发保证只有指针真正落在内容区内的滚动节点才会收到事件，故直接按方向
/// 消费 `wheelDeltaY`/`wheelDeltaX` 原值（`event.stopPropagation()` 拦住外层，
/// 实现"内层优先、滚到底才交给外层"的嵌套滚动语义）。
///
/// 返回 `Rc<dyn Fn(&mut MouseEvent2)>`：作为 `Node2Props.mouse_wheel` 挂到节点上，
/// 经 `to_node` 桥接进 v1 引擎的 `on_wheel_fn`，由 `dispatch_wheel_chain` 沿命中链派发。
#[must_use]
pub fn scroll_wheel(scroll: &Scroll2) -> Rc<dyn Fn(&mut MouseEvent2)> {
    let s = scroll.clone();
    Rc::new(move |event: &mut MouseEvent2| {
        let delta = match s.direction {
            Axis::Y => event.wheel_delta_y,
            Axis::X => event.wheel_delta_x,
        };
        if delta == 0.0 {
            return;
        }
        s.scroll(delta);
        // 拦住外层：否则页面/父容器会同时滚动，形成"滚一格跳两格"
        event.stop_propagation();
    })
}

// ============================================================================
// 滚动内容（TS `scrollContent`）
// ============================================================================

/// 滚动内容节点（TS `scrollContent`）。
///
/// `#[rec(extends = node2)]`：除 `scroll` 句柄外的全部节点参数（`layout` / `exts` /
/// `children` / `size` / `padding` …）直接复用 `node2` 的 props，宏内联转发进自动
/// 注入的 `rest: Node2Props`；调用处用 `scroll_content! { scroll, .. }` 同构写法
/// 写基类字段，不再手写 `ScrollContentArg { rest: .. }`。
///
/// 四件事，与 TS 注释一一对应：
/// 1. 打 `isScrollContent` 标记（这里用 [`ScrollContentExt`] ext）；
/// 2. 滚动轴位置 = 容器 padding 起点 `- value`——引擎的 `x_fn`/`y_fn` 是叠加在
///    布局盒上的偏移，而布局盒在该轴上恒为 `padding_start`（滚动轴是反向容器的
///    交叉轴，位置为 0），所以只写 `-value` 即等价于 TS 的 `pad - value`；
/// 3. **自己**把子树裁到容器内盒（TS `drawAtParent` → `Node2Props::clip_to_parent`），
///    并把命中门控在视口内（TS `acceptClip`）。两件事都由内容节点做，
///    所以容器**不需要**写 `clip`——这正是 `todo-list.ts` / `todo-demos.ts` 里
///    滚动容器一个 `clip` 都没有的原因；
/// 4. 其余参数原样透传。
#[rec(extends = node2)]
#[must_use]
pub fn scroll_content(
    scroll: Scroll2,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）：**普通参数**，不传则内部新建、
    /// 调用方拿不到该节点；需要句柄时显式传一个 `Ref2<Node2>`。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    // 滚动轴位置 = `-value`（见上方第 2 条）。用结构体更新语法表达 TS 的
    // `{ ...rest, x: .. }`——"在原参数上覆盖一个轴"，不是手搓节点。
    let s = scroll.clone();
    let rest = match scroll.direction {
        Axis::Y => Node2Props {
            y: Some(get(move || -s.value())),
            ..rest
        },
        Axis::X => Node2Props {
            x: Some(get(move || -s.value())),
            ..rest
        },
    };

    // 滚轮沿命中链到达内容区时由本节点消费，并拦住外层（嵌套滚动"内层优先"）。
    let wheel = scroll_wheel(&scroll);
    // TS `acceptClip`：只有容器 inner 区内才算命中（滚出视口的条目不该响应事件）。
    // 引擎 `hitest` 传进来的坐标**相对父（容器）**，与 TS 的 `acceptClip(x, y)` 同义。
    let hit_ref = node_ref.clone();
    let accept_clip = move |x: f32, y: f32| {
        let Some(node) = hit_ref.try_get() else {
            return false;
        };
        let Some(container) = node.parent() else {
            return false;
        };
        let left = container.padding_inline_start();
        let top = container.padding_block_start();
        let iw = (container.width() - left - container.padding_inline_end()).max(0.0);
        let ih = (container.height() - top - container.padding_block_end()).max(0.0);
        x > left && x < left + iw && y > top && y < top + ih
    };
    node2! {
        ..rest,
        node_ref: node_ref.clone(),
        // 与 TS `exts: [...(exts ?? []), scrollContentExt]` 一致：`..rest` 带回
        // 调用方的扩展，`exts_push` 再追加共享滚动内容标记。
        exts_push: scroll_content_ext(),
        mouse_wheel: move |e: &mut MouseEvent2| wheel(e),
        // TS `drawAtParent` 的裁剪：把本支裁到**容器**内盒，容器自己不必写 `clip`
        clip_to_parent: true,
        accept_clip: accept_clip,
    }
}

// ============================================================================
// 滚动条（TS `scrollBar`）
// ============================================================================

/// 滚动条（TS `scrollBar`）。
///
/// `#[rec(extends = node2)]`：除 `scroll` 句柄外的全部节点参数（`width` / `height` /
/// `layout` / `exts` / `draw` / `children` …）复用 `node2` 的 props，宏内联转发进
/// 自动注入的 `rest: Node2Props`。`thickness` / `track_color` / `thumb_color` /
/// `thumb_radius` 是滚动条专属样式，沿用 TS「未写即回落默认」，保持为自有字段
/// （`Option<Get<..>>`，缺省 `None`）。
///
/// - 轨道：交叉轴固定厚度，滚动轴尺寸交给父布局（Row 容器里 = stretch）；
/// - 滑块：尺寸与偏移由 [`Scroll2::scroll_bar_size`] 纯派生，不可滚动时尺寸为 0
///   自然隐身（TS 同样不给 `hide`）；
/// - 拖拽：`mouseDown` 起手，随后交给全局鼠标注册表的 [`drag_mouse`]，
///   指针移出滑块/画布仍持续拖动（与节点 `mouseDown` 冒泡派发解耦）。
///
/// 轨道**不设 layout**：TS 里 `node2.create({...})` 没给 layout，`commonNode`
/// 就落 `absoluteLayoutFun`，于是滑块按自身 `x`/`y`/`width`/`height` 绝对定位。
/// Rust 引擎对无 `layout` 的节点的处理与之相同（`layout_boxes` 返回零盒、
/// 尺寸取自身 `node_size`），所以这里也保持"不给 layout"。
#[rec(extends = node2)]
#[must_use]
pub fn scroll_bar(
    /// 滚动状态句柄。
    scroll: Scroll2,
    /// 轨道厚度（交叉轴外尺寸），默认 10。
    thickness: Option<crate::canvas_render2::node2::Get<f32>>,
    /// 轨道背景，`None` 着色 / `Some(None)` 不画；默认 `rgba(0,0,0,15)`。
    track_color: Option<crate::canvas_render2::node2::Get<Option<Color>>>,
    /// 滑块背景，默认 `rgba(120,120,120,180)`。
    thumb_color: Option<crate::canvas_render2::node2::Get<Color>>,
    /// 滑块圆角（自动钳到半宽/半高），默认 5。
    thumb_radius: Option<crate::canvas_render2::node2::Get<f32>>,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）：**普通参数**，不传则内部新建、
    /// 调用方拿不到该节点；需要句柄时显式传一个 `Ref2<Node2>`。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    let direction = scroll.direction;
    let thickness = thickness.unwrap_or_else(|| always(DEFAULT_THICKNESS));
    let track_color = track_color.unwrap_or_else(|| always(Some(TRACK_COLOR)));
    let thumb_color = thumb_color.unwrap_or_else(|| always(THUMB_COLOR));
    let thumb_radius = thumb_radius.unwrap_or_else(|| always(DEFAULT_THUMB_RADIUS));

    // 轨道：交叉轴固定厚度，滚动轴不声明（交给父布局分配）。
    // 结构体更新语法 = TS 的 `{ ...rest, width: thickness() }`。
    let rest = match direction {
        Axis::Y => Node2Props {
            width: Some(Rc::clone(&thickness)),
            ..rest
        },
        Axis::X => Node2Props {
            height: Some(Rc::clone(&thickness)),
            ..rest
        },
    };

    // 轨道自身：先调继承来的 draw，再画底色。
    let rest_draw = rest.draw.clone();
    let track_fill = Rc::clone(&track_color);
    let track_width = Rc::clone(&thickness);
    let track_draw = move |scene: &mut Scene, n: &Node2| {
        if let Some(f) = &rest_draw {
            f(scene, n);
        }
        let Some(color) = track_fill() else { return };
        let (w, h) = (n.width(), n.height());
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        // TS: `const r = Math.min(thickness() / 2, 4);` 圆角 0 时走直角。
        let r = (track_width() / 2.0).min(4.0);
        if r > 0.0 {
            scene.fill_round_rect(Rect::new(0.0, 0.0, w, h), r, color);
        } else {
            scene.fill_rect(Rect::new(0.0, 0.0, w, h), color);
        }
    };

    // 滑块：滚动轴尺寸由 `scrollBarSize` 派生；交叉轴撑满**轨道** inner
    // （TS `innerSize(node, d)` 里的 `node` 是轨道自己）。轨道此刻还没造出来，
    // 所以读句柄槽——`width()` / `height()` 都是布局期求值。
    let s_w = scroll.clone();
    let s_h = scroll.clone();
    let track_x = node_ref.clone();
    let track_y = node_ref.clone();
    let s_off = scroll.clone();
    let s_thumb = scroll.clone();
    let thumb_fill = Rc::clone(&thumb_color);
    let thumb_corner = Rc::clone(&thumb_radius);
    let thumb = node2! {
        width: match direction {
            Axis::X => s_w.scroll_bar_size(Axis::X, 0.0).map_or(0.0, |c| c.size),
            Axis::Y => track_x.get().inner_size(Axis::X),
        },
        height: match direction {
            Axis::Y => s_h.scroll_bar_size(Axis::Y, 0.0).map_or(0.0, |c| c.size),
            Axis::X => track_y.get().inner_size(Axis::Y),
        },
        // 只沿滚动轴偏移，另一轴恒 0（TS 同样只写 `x` 或 `y` 之一）。
        position: move |d| {
            if d != direction {
                return 0.0;
            }
            s_off
                .scroll_bar_size(direction, 0.0)
                .map_or(0.0, |c| c.offset)
        },
        draw: move |scene: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            // TS: `Math.min(thumbRadius(), Math.min(w, h) / 2)`
            let r = thumb_corner().min(w.min(h) / 2.0);
            if r > 0.0 {
                scene.fill_round_rect(Rect::new(0.0, 0.0, w, h), r, thumb_fill());
            } else {
                scene.fill_rect(Rect::new(0.0, 0.0, w, h), thumb_fill());
            }
        },
        // 拖拽：TS `mouseDown(e) { ..; dragMouse(ctx, (x, y) => ..) }`。
        mouse_down: move |ev: &mut MouseEvent2| {
            let Some(c) = s_thumb.scroll_bar_size(direction, 0.0) else {
                return;
            };
            let start_value = s_thumb.value();
            let start_pointer = match direction {
                Axis::Y => ev.global_y,
                Axis::X => ev.global_x,
            };
            let drag_scroll = s_thumb.clone();
            let _ = drag_mouse(move |x, y| {
                let pointer = match direction {
                    Axis::Y => y,
                    Axis::X => x,
                };
                drag_scroll.set_value(start_value + c.move_to_scroll(pointer - start_pointer));
            });
        },
    };

    // 滚轮同样沿命中链到达滚动条时由本节点消费（TS `OverlayScrollBar` 同形）。
    let wheel = scroll_wheel(&scroll);
    node2! {
        ..rest,
        node_ref: node_ref.clone(),
        draw: track_draw,
        mouse_wheel: move |e: &mut MouseEvent2| wheel(e),
        children: vec![thumb],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_absent_when_content_fits() {
        assert!(
            compute_scroll_bar(100.0, 100.0, 100.0, 0.0).is_none(),
            "正好等长"
        );
        assert!(
            compute_scroll_bar(100.0, 100.0, 80.0, 0.0).is_none(),
            "内容更短"
        );
    }

    #[test]
    fn bar_math_matches_ts() {
        let c = compute_scroll_bar(100.0, 100.0, 400.0, 0.0).expect("可滚动");
        assert!((c.max_scroll - 300.0).abs() < 1e-5, "m = 内容 − 视口");
        assert!((c.size - 25.0).abs() < 1e-5, "thumb = len*v/c");
        assert!(
            (c.max_offset - 75.0).abs() < 1e-5,
            "maxOffset = len − thumb"
        );
        assert!(c.offset.abs() < 1e-5, "起点偏移 0");

        // 滚到半程 → 偏移半程
        let half = compute_scroll_bar(100.0, 100.0, 400.0, 150.0).unwrap();
        assert!((half.offset - half.max_offset / 2.0).abs() < 1e-5);

        // 双向换算互为逆
        let back = half.scroll_to_move(half.move_to_scroll(10.0));
        assert!((back - 10.0).abs() < 1e-4);
    }

    #[test]
    fn thumb_clamped_to_min_length() {
        let c = compute_scroll_bar(100.0, 100.0, 100_000.0, 0.0).unwrap();
        assert!((c.size - MIN_THUMB).abs() < 1e-5, "极长内容压到下限");
        assert!(c.max_offset >= 0.0);
    }

    #[test]
    fn explicit_length_is_used_as_track() {
        // track 100、视口 100、内容 400 → thumb 25；track 200 时 thumb 50
        let a = compute_scroll_bar(100.0, 100.0, 400.0, 0.0).unwrap();
        let b = compute_scroll_bar(200.0, 100.0, 400.0, 0.0).unwrap();
        assert!((b.size - 50.0).abs() < 1e-5);
        assert!(b.max_offset > a.max_offset);
    }

    #[test]
    fn value_clamped_to_max_scroll() {
        let container = Node2::empty().build(Node2Props {
            height: Some(always(100.0)),
            ..Node2Props::default()
        });
        let content = Node2::empty().build(Node2Props {
            height: Some(always(300.0)),
            exts: vec![scroll_content_ext()],
            ..Node2Props::default()
        });
        // 内容节点要挂进容器，`contentSize` 才扫得到（TS 同样扫 children）。
        container.push_child(&content);
        let scroll = create_scroll(&container, Axis::Y);

        assert!((scroll.max_scroll(Axis::Y) - 200.0).abs() < 1e-5);
        scroll.set_value(1000.0);
        assert!((scroll.value() - 200.0).abs() < 1e-5, "读时钳制到 max");
    }

    #[test]
    fn scroll_delta_returns_real_amount() {
        let container = Node2::empty().build(Node2Props {
            height: Some(always(100.0)),
            ..Node2Props::default()
        });
        let content = Node2::empty().build(Node2Props {
            height: Some(always(300.0)),
            exts: vec![scroll_content_ext()],
            ..Node2Props::default()
        });
        container.push_child(&content);
        let scroll = create_scroll(&container, Axis::Y);

        assert!((scroll.scroll(50.0) - 50.0).abs() < 1e-5);
        assert!((scroll.value() - 50.0).abs() < 1e-5);
        // 越过边界只回真实滚动量
        assert!((scroll.scroll(500.0) - 150.0).abs() < 1e-5);
        assert!((scroll.value() - 200.0).abs() < 1e-5);
        // 往回滚到 0 以下：真实滚动量同样被钳制
        assert!((scroll.scroll(-500.0) + 200.0).abs() < 1e-5);
        assert!(scroll.value().abs() < 1e-5);
    }

    /// 竖滚动容器 + 可滚内容（`viewport` 视口、`content` 内容长）。
    fn scroll_pair(viewport: f32, content: f32) -> Scroll2 {
        let container = Node2::empty().build(Node2Props {
            height: Some(always(viewport)),
            ..Node2Props::default()
        });
        let inner = Node2::empty().build(Node2Props {
            height: Some(always(content)),
            exts: vec![scroll_content_ext()],
            ..Node2Props::default()
        });
        container.push_child(&inner);
        create_scroll(&container, Axis::Y)
    }

    #[test]
    fn activity_fires_on_scroll_and_on_set_value() {
        use std::cell::Cell;

        let scroll = scroll_pair(100.0, 300.0);
        let hits = std::rc::Rc::new(Cell::new(0));

        let h = std::rc::Rc::clone(&hits);
        let listener = scroll.add_activity_listener(std::rc::Rc::new(move || h.set(h.get() + 1)));

        // 滚轮走的是 `scroll()`：它绕过 `set_value`，通知必须自己补
        scroll.scroll(30.0);
        assert_eq!(hits.get(), 1, "滚一下要通知一次（浮层滚动条靠它浮现）");

        scroll.set_value(80.0);
        assert_eq!(hits.get(), 2, "程序写值同样算活动");

        // 值没变就不通知（否则静止时也在闪）
        scroll.set_value(80.0);
        assert_eq!(hits.get(), 2);

        // 到边界再滚：真实滚动量为 0，也不该通知
        scroll.scroll(-10_000.0);
        assert_eq!(hits.get(), 3, "回滚到顶算一次活动");
        scroll.scroll(-100.0);
        assert_eq!(hits.get(), 3, "已经在顶了，再滚不算活动");

        drop(listener);
        scroll.scroll(50.0);
        assert_eq!(hits.get(), 3, "句柄 drop 后必须退订（RAII 而非漏调注销）");
    }

    #[test]
    fn nested_scroll_forwards_remainder_to_parent() {
        let parent = scroll_pair(100.0, 300.0);
        let child = scroll_pair(100.0, 200.0).with_parent(parent.clone());

        // 内层吃不满：全部由内层消费，外层不动
        let real = child.scroll(60.0);
        assert!((real - 60.0).abs() < 1e-5);
        assert!(parent.value().abs() < 1e-5);

        // 内层只剩 40 可滚：滚动量 100 → 内层吃 40、外层吃 60
        let real = child.scroll(100.0);
        assert!((real - 40.0).abs() < 1e-5, "返回的只是本层真实滚动量");
        assert!((child.value() - 100.0).abs() < 1e-5);
        assert!(
            (parent.value() - 60.0).abs() < 1e-5,
            "剩余量接续给外层（TS `parentScroll` 的嵌套滚动）"
        );
    }
}
