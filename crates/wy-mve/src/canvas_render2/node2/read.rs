//! `Node2` 的读取式 API：尺寸 / 位置 / 子节点 / 命中 / 绘制（每次调用重新求值）。

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use wy_render::{Point, Rect, Scene};

use crate::canvas_render2::focus;
use crate::canvas_render2::focus::KeyEvent2;
use crate::canvas_render2::protocol::MouseEvent2;
use crate::{primitives::CursorType, NodeGeom, NodeRef};

use super::*;

impl Node2 {
    /// 空壳节点（TS `{} as Node2`）：只有身份与几何 cell，属性由 [`Node2Props`] 填。
    #[must_use]
    pub fn empty() -> Self {
        Self(Rc::new(Node2Inner {
            id: next_id(),
            geom: Rc::new(Cell::new(NodeGeom::ZERO)),
            data: RefCell::new(Node2Data::default()),
            children: RefCell::new(Vec::new()),
            parent: RefCell::new(None),
            index: Cell::new(0),
            layout_index: Cell::new(0),
        }))
    }

    /// 节点身份（焦点信号、`PartialEq` 比对都靠它）。
    #[must_use]
    pub fn id(&self) -> NodeId2 {
        self.0.id
    }

    /// 共享几何 cell（交给引擎 IR 的 `Node::geom`，两边看到同一份）。
    #[must_use]
    pub fn geom_cell(&self) -> Rc<Cell<NodeGeom>> {
        Rc::clone(&self.0.geom)
    }

    /// 几何句柄（构造期释放给 `node_of` / `draw` 引用最终生效值）。
    #[must_use]
    pub fn node_ref(&self) -> NodeRef {
        NodeRef::from_cell(self.geom_cell())
    }

    /// 本帧生效几何。
    #[must_use]
    pub fn geom(&self) -> NodeGeom {
        self.0.geom.get()
    }

    /// 取 IME 光标盒（TS `runImeCursorRect`）：节点**局部**坐标 `x, y, w, h`。
    ///
    /// 宿主拿它加上节点绝对位置去摆输入法候选框（Kotlin `requestInputOverlay`）。
    /// 回调从 `RefCell` 里 clone 出来再调用——不能"借着重调"，否则回调里若再碰
    /// 同一格就是 `RefCell already borrowed`。
    #[must_use]
    pub fn ime_cursor_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let f = self.data().ime_cursor.clone()?;
        Some(f())
    }

    /// 是否带了某类扩展（TS `n.exts.some(x => x instanceof C)` 的判定）。
    ///
    /// 供"类型标记"用法：`scrollContent` 在 TS 上给节点打 `isScrollContent`，
    /// 这里用 `ScrollContentMarker` 作为 ext 打标，`contentSize` 据此从容器
    /// 子节点里认出内容节点——与 TS 的结构完全一致。
    #[must_use]
    pub fn has_ext<T: Any>(&self) -> bool {
        find_ext::<T>(&self.0.data.borrow().exts).is_some()
    }

    fn data(&self) -> std::cell::Ref<'_, Node2Data> {
        self.0.data.borrow()
    }

    // ---- 尺寸 / 位置（TS `width()` / `height()` / `x()` / `y()`）----

    /// 外包宽：TS `width()` = `outerSize('x', this)`。
    ///
    /// 显式 `width` → 显式 `size`（加 padding 包成外包）→ 落到 `defaultSize`
    /// （实时几何，由父布局 / flex 测量）。**不再**回落到 v1 `geom`——
    /// 那是旧模式写入的缓存，新模式以 Node2 自己的几何为唯一真相。
    #[must_use]
    pub fn width(&self) -> f32 {
        let d = self.data();
        if let Some(f) = &d.width {
            return f();
        }
        if let Some(s) = &d.size {
            return self.to_outer(Axis::X, s(Axis::X));
        }
        drop(d);
        self.to_outer(
            Axis::X,
            crate::canvas_render2::tree::geometry::default_size(self, Axis::X),
        )
    }

    /// 外包高：TS `height()` = `outerSize('y', this)`。
    #[must_use]
    pub fn height(&self) -> f32 {
        let d = self.data();
        if let Some(f) = &d.height {
            return f();
        }
        if let Some(s) = &d.size {
            return self.to_outer(Axis::Y, s(Axis::Y));
        }
        drop(d);
        self.to_outer(
            Axis::Y,
            crate::canvas_render2::tree::geometry::default_size(self, Axis::Y),
        )
    }

    /// 只读**显式声明**的尺寸（`width` / `height` 字段本身），不含 `size` 闭包兜底。
    ///
    /// 与 [`declared`](Self::declared) 的区别：`declared` 在没有显式宽/高时会回落到
    /// 组件自己的 `size` 闭包——而 `rich_text_node` 的 `size` 闭包会反问段落宽度，
    /// 在"按声明宽排版段落"这一步里读它就会自递归。所以需要"只看声明字段"的入口。
    #[must_use]
    pub fn explicit(&self, axis: Axis) -> Option<f32> {
        let d = self.data();
        match axis {
            Axis::X => d.width.as_ref().map(|f| f()),
            Axis::Y => d.height.as_ref().map(|f| f()),
        }
    }

    /// 只读**显式声明**的位置（`x`/`y` 字段或 `position` 闭包）；`None` = 交给父布局。
    ///
    /// 绝对坐标求到**根节点**时没有父布局可问，`x()` 会落到
    /// `default_position` 的 `parent().expect("未找到父节点")` 上 panic。根节点
    /// 就是画布原点，没声明位置时取 0（TS `renderCanvas` 给根 `x/y = 0` 同理）。
    #[must_use]
    pub fn explicit_position(&self, axis: Axis) -> Option<f32> {
        let d = self.data();
        let explicit = match axis {
            Axis::X => d.x.as_ref().map(|f| f()),
            Axis::Y => d.y.as_ref().map(|f| f()),
        };
        explicit.or_else(|| d.position.as_ref().map(|p| p(axis)))
    }

    /// 已声明尺寸时取值（`None` = 交给父布局分配）。布局期用。
    #[must_use]
    pub fn declared(&self, axis: Axis) -> Option<f32> {
        let d = self.data();
        match axis {
            Axis::X => d.width.as_ref().map(|f| f()),
            Axis::Y => d.height.as_ref().map(|f| f()),
        }
        .or_else(|| d.size.as_ref().map(|s| self.to_outer(axis, s(axis))))
    }

    /// TS `toOuterSize`：`fromInside` 的声明值要加上两侧 padding。
    fn to_outer(&self, axis: Axis, size: LayoutSize) -> f32 {
        if !size.from_inside {
            return size.value;
        }
        match axis {
            Axis::X => size.value + self.padding_inline_start() + self.padding_inline_end(),
            Axis::Y => size.value + self.padding_block_start() + self.padding_block_end(),
        }
        .max(0.0)
    }

    /// 相对父节点的横向位置（TS `x()` = `position('x')`）。
    ///
    /// 显式 `x` → 显式 `position` → 落到 `defaultPosition`（父布局分配 + 父
    /// padding start）。**不再**用 v1 `geom` 反推——新模式以 Node2 几何为真相。
    #[must_use]
    pub fn x(&self) -> f32 {
        let d = self.data();
        if let Some(f) = &d.x {
            return f();
        }
        if let Some(p) = &d.position {
            return p(Axis::X);
        }
        drop(d);
        crate::canvas_render2::tree::geometry::default_position(self, Axis::X)
    }

    /// 相对父节点的纵向位置（TS `y()` = `position('y')`）。
    #[must_use]
    pub fn y(&self) -> f32 {
        let d = self.data();
        if let Some(f) = &d.y {
            return f();
        }
        if let Some(p) = &d.position {
            return p(Axis::Y);
        }
        drop(d);
        crate::canvas_render2::tree::geometry::default_position(self, Axis::Y)
    }

    /// 绝对（画布）横坐标（TS `absoluteX`）。
    ///
    /// 沿父链累加 `x()`（TS `absolutePosition`）——不依赖 v1 `geom` 回填。
    /// 根节点若无显式 `x`/`position`，沿链到顶即取其 `x()`（通常显式 0）。
    #[must_use]
    pub fn abs_x(&self) -> f32 {
        crate::canvas_render2::tree::geometry::absolute_x(self)
    }

    /// 绝对纵坐标（TS `absoluteY`）。
    #[must_use]
    pub fn abs_y(&self) -> f32 {
        crate::canvas_render2::tree::geometry::absolute_y(self)
    }

    /// 绝对坐标对。
    #[must_use]
    pub fn abs_pos(&self) -> (f32, f32) {
        let g = self.0.geom.get();
        (g.abs_x, g.abs_y)
    }

    /// Node2 原生递归绘制（TS `nodeShell.draw`）。
    ///
    /// 与 TS 同构：先画自身（`draw` 消息），再对每个可见子节点
    /// `push_transform(child.x(), child.y())` → `drawAtParent`（裁剪等父坐标系钩子）
    /// → 递归 → `pop_transform`。命中测试走 [`crate::canvas_render2::tree::geometry::hit_path`]，
    /// 二者都只用本节点的实时几何（`x()/width()`），**不依赖 v1 引擎回填的 `geom`**——
    /// 这是「新模式」脱离旧管线、让 Node2 成为真正渲染载体的绘制入口。
    ///
    /// 注意：TS 用 `ctx.save()/restore()` 把变换与裁剪一并回退；Rust `Scene` 把二者分开
    /// 管理，这里 `push_transform`/`pop_transform` 管变换，`clip` 字段为真时额外
    /// `push_clip`(inner 盒)/`pop_clip` 管裁剪（对应 v1 引擎 `draw_node` 的同名逻辑）。
    pub fn render_node2(&self, scene: &mut Scene) {
        // 自身绘制（TS `messages.draw(ctx2, this)`）
        //
        // ⚠️ 顺序讲究：`clip` 只夹**子节点**，不夹自己。TS 里裁剪来自子节点的
        // `drawAtParent`（`scrollContent` 在"父的绘制循环里、translate 之前"裁剪），
        // 容器的背景/边框在自己的 `draw` 里画、发生在那一刻**之前**，所以永远
        // 完整可见。早期这里把 `push_clip` 放在了父循环里（包住 `render_node2`），
        // 于是带 `clip` 的卡片自己的圆角背景与外框被自己的内盒裁掉——画面上就是
        // "卡片不见了、条目直接浮在页面上"。
        if let Some(draw) = &self.data().draw {
            draw(scene, self);
        }
        // 自身 clip 从这一刻起生效于后代。
        let clipped = self.data().clip;
        if clipped {
            let pt = self.padding_block_start();
            let ih = (self.height() - pt - self.padding_block_end()).max(0.0);
            let pl = self.padding_inline_start();
            let iw = (self.width() - pl - self.padding_inline_end()).max(0.0);
            if iw > 0.0 && ih > 0.0 {
                scene.push_clip(Rect::new(pl, pt, iw, ih));
            }
        }
        for child in self.children() {
            if child.skip_draw() {
                continue;
            }
            // 子节点要求「裁到父内盒」（TS `ScrollContent.drawAtParent`）：
            // 压栈必须在 `push_transform` **之前**、用**父的**内盒——TS 的
            // `save → drawAtParent(剪) → translate → draw → restore` 就是这么排的。
            // 放进 translate 之后就会用子节点自己的坐标去剪，等于剪错框。
            let clip_to_parent = child.data().clip_to_parent;
            let mut clip_pushed = false;
            if clip_to_parent {
                let pl = self.padding_inline_start();
                let pt = self.padding_block_start();
                let iw = (self.width() - pl - self.padding_inline_end()).max(0.0);
                let ih = (self.height() - pt - self.padding_block_end()).max(0.0);
                if iw > 0.0 && ih > 0.0 {
                    scene.push_clip(Rect::new(pl, pt, iw, ih));
                    clip_pushed = true;
                }
            }
            scene.push_transform(Point::new(child.x(), child.y()));
            if let Some(dap) = &child.data().draw_at_parent {
                dap(scene, &child);
            }
            child.render_node2(scene);
            scene.pop_transform();
            if clip_pushed {
                scene.pop_clip();
            }
        }
        if clipped {
            scene.pop_clip();
        }
    }

    // ---- 内边距（TS `paddingInlineStart()` …）----

    /// 起始横向内边距（左）。
    #[must_use]
    pub fn padding_inline_start(&self) -> f32 {
        self.data()
            .padding_inline_start
            .as_ref()
            .map_or(0.0, |f| f().max(0.0))
    }

    /// 结束横向内边距（右）。
    #[must_use]
    pub fn padding_inline_end(&self) -> f32 {
        self.data()
            .padding_inline_end
            .as_ref()
            .map_or(0.0, |f| f().max(0.0))
    }

    /// 起始纵向内边距（上）。
    #[must_use]
    pub fn padding_block_start(&self) -> f32 {
        self.data()
            .padding_block_start
            .as_ref()
            .map_or(0.0, |f| f().max(0.0))
    }

    /// 结束纵向内边距（下）。
    #[must_use]
    pub fn padding_block_end(&self) -> f32 {
        self.data()
            .padding_block_end
            .as_ref()
            .map_or(0.0, |f| f().max(0.0))
    }

    /// 指定轴上的起/止内边距（TS `getPadding(node, d, side)`）。
    #[must_use]
    pub fn padding(&self, d: Axis, side: StartEnd) -> f32 {
        match (d, side) {
            (Axis::X, StartEnd::Start) => self.padding_inline_start(),
            (Axis::X, StartEnd::End) => self.padding_inline_end(),
            (Axis::Y, StartEnd::Start) => self.padding_block_start(),
            (Axis::Y, StartEnd::End) => self.padding_block_end(),
        }
    }

    /// 内容区尺寸（TS `innerSize(node, d)`）。
    #[must_use]
    pub fn inner_size(&self, d: Axis) -> f32 {
        let size = match d {
            Axis::X => self.width(),
            Axis::Y => self.height(),
        };
        (size - self.padding(d, StartEnd::Start) - self.padding(d, StartEnd::End)).max(0.0)
    }

    // ---- 布局 / 树 ----

    /// 容器布局策略（TS `arg.layout ?? absoluteLayoutDirection`）。
    ///
    /// 没声明就是[`absolute`]（绝对定位）——
    /// 不是"没有布局"，而是**一条具体的布局策略**，所以这里没有 `Option`。
    #[must_use]
    pub fn layout(&self) -> crate::canvas_render2::node2::LayoutDir2 {
        self.data().layout.clone()
    }

    /// 子节点（TS `children()`）。
    ///
    /// **已按 `hide()` 过滤**——TS 侧过滤发生在构建期：`nodeConfig.purifyList`
    /// → `canvasCore/List.ts` 的 `flatten` 展平嵌套列表并剔除 `hide()` 为真的节点，
    /// 随后 `after: collectIndex` 回填 `_index`。
    ///
    /// Rust 侧挂载（`attach_to`）时嵌套列表已经展平，因此这里只保留等价的两步：
    /// 过滤 hide + 回填文档序索引。放在读取时做，与 TS「列表随信号重建」的观效一致
    /// （`hide()` 翻转后两边都立刻从 `children()` 里消失）。
    #[must_use]
    pub fn children(&self) -> Vec<Node2> {
        let kids: Vec<Node2> = self
            .0
            .children
            .borrow()
            .iter()
            .filter(|c| !c.hide())
            .cloned()
            .collect();
        // TS `collectIndex(list)`
        for (i, c) in kids.iter().enumerate() {
            c.0.index.set(i);
        }
        kids
    }

    /// 全部已挂载的子节点（含隐藏的），仅供桥接 / 诊断使用。
    ///
    /// TS 没有这条消息——`children()` 本身就是过滤后的结果，隐藏节点根本不在列表里。
    /// 只给需要"原始挂载列表"的场景（如 v1 桥接）留一个出口，树算法一律走
    /// [`Node2::children`]。
    #[must_use]
    pub fn raw_children(&self) -> Vec<Node2> {
        self.0.children.borrow().clone()
    }

    /// 文档序索引（TS `index()`）。
    ///
    /// TS 原文：
    /// ```ts
    /// if (this.hide()) throw new Error('已经隐藏不再显示');
    /// if (parent) { parent.children(); }
    /// return this._index;
    /// ```
    /// 隐藏节点不在 `children()` 里，索引对它没有意义——这里用 `debug_assert`
    /// 表达（release 下退化为返回上次回填值，不因此打断渲染）。
    #[must_use]
    pub fn index(&self) -> usize {
        debug_assert!(!self.hide(), "已经隐藏不再显示");
        // 先确保父节点已回填索引（TS `parent.children()`）
        if let Some(p) = self.parent() {
            let _ = p.children();
        }
        self.0.index.get()
    }

    /// 参与布局的子节点（TS `layoutChildren()`）。
    ///
    /// TS 原文是 `this.children().filter(n => !n.notInLayout())`——**只**过滤
    /// `notInLayout`：`hide` 已经由 `children()` 过滤掉了，这里再滤一次就重复了。
    ///
    /// 与 TS 一样**顺带回填**每个子节点的布局序索引（`_layoutIndex`）——
    /// [`Node2::layout_index`] 只负责读。
    #[must_use]
    pub fn layout_children(&self) -> Vec<Node2> {
        let kids: Vec<Node2> = self
            .children()
            .into_iter()
            .filter(|c| !c.not_in_layout())
            .collect();
        for (i, c) in kids.iter().enumerate() {
            c.0.layout_index.set(i);
        }
        kids
    }

    /// 父节点（`Weak` 向上，避免父子循环引用）。
    #[must_use]
    pub fn parent(&self) -> Option<Node2> {
        self.0
            .parent
            .borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(Node2)
    }

    /// 布局序索引（TS `layoutIndex()`）。
    ///
    /// TS 原文：
    /// ```ts
    /// if (this.hide()) throw new Error('已经隐藏不再显示');
    /// if (this.notInLayout()) throw new Error('当前节点不在Layout中');
    /// this.parent?.layoutChildren();
    /// return this._layoutIndex;
    /// ```
    /// 两个前置条件这里用 `debug_assert` 表达（宿主不该在隐藏/不参与布局的节点上
    /// 问索引；release 下退化为返回上次回填值，不因此打断渲染）。
    #[must_use]
    pub fn layout_index(&self) -> usize {
        debug_assert!(!self.hide(), "已经隐藏不再显示");
        debug_assert!(!self.not_in_layout(), "当前节点不在Layout中");
        // 先确保父节点已回填索引
        if let Some(p) = self.parent() {
            let _ = p.layout_children();
        }
        self.0.layout_index.get()
    }

    /// 横向布局对象（TS `layoutX()` = `createLayout(this, 'x')()`）。
    #[must_use]
    pub fn layout_x(&self) -> Rc<dyn wy_layout::Layout> {
        crate::canvas_render2::tree::geometry::create_layout(self, Axis::X)
    }

    /// 纵向布局对象（TS `layoutY()` = `createLayout(this, 'y')()`）。
    #[must_use]
    pub fn layout_y(&self) -> Rc<dyn wy_layout::Layout> {
        crate::canvas_render2::tree::geometry::create_layout(self, Axis::Y)
    }

    /// 扩展对象列表（TS `exts`）。
    #[must_use]
    pub fn exts(&self) -> Vec<Ext> {
        self.data().exts.clone()
    }

    /// 按类型找扩展（TS `findExt`）。
    #[must_use]
    pub fn find_ext<T>(&self) -> Option<T>
    where
        T: Any + Clone,
    {
        find_ext::<T>(&self.data().exts).cloned()
    }

    /// 按类型取扩展的**共享句柄**（TS `findExt` 的 `Rc` 版）。
    ///
    /// 布局转换（`grow` / `ignoreFlex`）要在闭包里读扩展，借用版 [`Node2::find_ext`]
    /// 拿不出来，故取 `Rc`。
    #[must_use]
    pub fn find_ext_rc<T: Any>(&self) -> Option<Rc<T>> {
        self.data()
            .exts
            .iter()
            .find_map(|e| Rc::clone(e).downcast::<T>().ok())
    }

    /// 诊断用可读名（TS `debugName()`）；`None` 对应 TS 的 `null`。
    #[must_use]
    pub fn debug_name(&self) -> Option<String> {
        self.data().debug_name.as_ref().map(|f| f())
    }

    // ---- 读取式开关（TS `hide()` / `notInLayout()` / …）----

    /// 是否隐藏（不参与布局与命中）。
    #[must_use]
    pub fn hide(&self) -> bool {
        (self.data().hide)()
    }

    /// 是否不参与父布局。
    ///
    /// 两个来源任一为真即成立：节点自己的 `not_in_layout`，以及 `exts` 里的
    /// [`IgnoreFlexChild`]（`ignore_flex(true)`）。合并成同一个判据是为了让
    /// [`Self::layout_children`] 与 flex 排布看到一致的结果——早期版本把这层
    /// 合并放在 `to_node` 里，IR 层删除后收回到这里。
    #[must_use]
    pub fn not_in_layout(&self) -> bool {
        (self.data().not_in_layout)()
            || self
                .find_ext_rc::<IgnoreFlexChild>()
                .is_some_and(|it| (it.ignore)())
    }

    /// 是否参与文本选择。
    #[must_use]
    pub fn selection_enabled(&self) -> bool {
        (self.data().selection_enabled)()
    }

    /// 是否裁剪溢出（`data.clip`，静态字段；滚动容器靠它在绘制时开裁剪）。
    #[must_use]
    pub fn data_clip(&self) -> bool {
        self.data().clip
    }

    /// 是否把子树裁到父节点的内盒（TS `ScrollContent.drawAtParent`）。
    #[must_use]
    pub fn data_clip_to_parent(&self) -> bool {
        self.data().clip_to_parent
    }

    /// 是否跳过绘制。
    ///
    /// ⚠️ 语义是**整棵子树**（[`Node2::render_node2`] 对这类子节点是 `continue`，
    /// 在递归之前就跳过），不是"只跳过自身、子节点照画"。所以带子节点的容器
    /// 绝不能写它——写了那批子节点会凭空消失，而树上的几何查起来一切正常。
    /// 想"自己不画"就别给 `draw`，那才是只影响自身。
    #[must_use]
    pub fn skip_draw(&self) -> bool {
        (self.data().skip_draw)()
    }

    /// 是否可被 Tab 焦点遍历拾取。
    #[must_use]
    pub fn focusable(&self) -> bool {
        (self.data().focusable)()
    }

    /// Tab 遍历显式顺序（`None` = 文档序）。
    #[must_use]
    pub fn focus_order(&self) -> Option<i32> {
        (self.data().focus_order)()
    }

    /// 是否焦点陷阱（模态层）。
    #[must_use]
    pub fn focus_trap(&self) -> bool {
        (self.data().focus_trap)()
    }

    /// 当前是否持有焦点（读全局焦点信号，响应式：焦点变化会自动重绘）。
    #[must_use]
    pub fn is_focused(&self) -> bool {
        focus::is_focused(self)
    }

    /// 请求获得焦点。
    pub fn request_focus(&self) {
        focus::request_focus(self);
    }

    /// 键盘事件处理；返回 `true` 表示已消费。
    #[must_use]
    pub fn handle_key(&self, e: &KeyEvent2) -> bool {
        match &self.data().handle_key {
            Some(f) => f(e),
            None => false,
        }
    }

    // ---- 命中 ----

    /// 是否命中（本地坐标）。默认判据与 TS 一致：严格在盒内。
    #[must_use]
    pub fn accept_hit(&self, x: f32, y: f32) -> bool {
        if let Some(f) = &self.data().accept_hit {
            return f(x, y);
        }
        x > 0.0 && y > 0.0 && x < self.width() && y < self.height()
    }

    /// 是否允许子树接收命中（本地坐标）。默认接受。
    #[must_use]
    pub fn accept_clip(&self, x: f32, y: f32) -> bool {
        match &self.data().accept_clip {
            Some(f) => f(x, y),
            None => true,
        }
    }

    /// 鼠标光标（`None` = 由引擎派生）。
    #[must_use]
    pub fn cursor(&self) -> Option<CursorType> {
        self.data().cursor.as_ref().map(|f| f())
    }

    // ---- 指针消息（TS `mouseClick` / `mouseDown` / … 十四条）----
    //
    // 协议里它们是**必答消息**：TS 侧缺省由 `nodeMessageDefaults` 给空实现，
    // 所以派发可以直接 `target.mouseClick(event)`，不存在"钩子有没有给"的判断
    // （TS `tree/dispatch.ts` 的注释明确写了：不能检查成员是否存在）。
    // Rust 侧由这十四个方法承担：内部 `Option` 为空时就是空实现。

    /// 点击（TS `mouseClick`）。
    pub fn mouse_click(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_click.as_ref(), event)
    }
    /// 点击捕获相位（TS `mouseClickCapture`）。
    pub fn mouse_click_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_click_capture.as_ref(), event)
    }
    /// 按下（TS `mouseDown`）。
    pub fn mouse_down(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_down.as_ref(), event)
    }
    /// 按下捕获相位（TS `mouseDownCapture`）。
    pub fn mouse_down_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_down_capture.as_ref(), event)
    }
    /// 抬起（TS `mouseUp`）；`cancel` 也走这条（对标 Kotlin Cancel → onPointerUp）。
    pub fn mouse_up(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_up.as_ref(), event)
    }
    /// 抬起捕获相位（TS `mouseUpCapture`）。
    pub fn mouse_up_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_up_capture.as_ref(), event)
    }
    /// 移动（TS `mouseMove`）。
    pub fn mouse_move(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_move.as_ref(), event)
    }
    /// 移动捕获相位（TS `mouseMoveCapture`）。
    pub fn mouse_move_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_move_capture.as_ref(), event)
    }
    /// 滚轮（TS `mouseWheel`）。
    pub fn mouse_wheel(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_wheel.as_ref(), event)
    }
    /// 滚轮捕获相位（TS `mouseWheelCapture`）。
    pub fn mouse_wheel_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_wheel_capture.as_ref(), event)
    }
    /// 右键上下文菜单（TS `mouseContextMenu`）。
    pub fn mouse_context_menu(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_context_menu.as_ref(), event)
    }
    /// 右键菜单捕获相位（TS `mouseContextMenuCapture`）。
    pub fn mouse_context_menu_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_context_menu_capture.as_ref(), event)
    }
    /// 双击（TS `mouseDoubleClick`）。
    pub fn mouse_double_click(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_double_click.as_ref(), event)
    }
    /// 双击捕获相位（TS `mouseDoubleClickCapture`）。
    pub fn mouse_double_click_capture(&self, event: &mut MouseEvent2) {
        self.send_mouse(|d| d.mouse_double_click_capture.as_ref(), event)
    }

    /// 十四条指针消息的共同实现：取钩子（没给就是空实现）→ 发消息。
    ///
    /// 闭包取 `&Node2Data` 而不是直接取值，是因为 `data()` 的 `Ref` 必须在
    /// 调用期间一直活着——先把 `Rc` 克隆出来再解锁，避免钩子里读同一个节点时
    /// 触发重复借用。
    fn send_mouse(
        &self,
        pick: impl FnOnce(&Node2Data) -> Option<&Mouse2>,
        event: &mut MouseEvent2,
    ) {
        let hook = {
            let d = self.0.data.borrow();
            pick(&d).map(Rc::clone)
        };
        if let Some(f) = hook {
            f(event);
        }
    }

    // ---- 挂载 ----

    /// 把自己挂到父节点下（构建期由 `Node2::builder` 调用）。
    pub(super) fn attach_to(&self, parent: &Node2) {
        *self.0.parent.borrow_mut() = Some(Rc::downgrade(&parent.0));
        parent.0.children.borrow_mut().push(self.clone());
    }

    /// 显式挂一个子节点（构建期之外用：单测、动态追加）。
    ///
    /// 与 `Self::attach_to` 等价，只是方向反过来，且是公开的——正常声明式
    /// 组装走 `Node2Props::children`，这里给需要"先建父、再挂子"的场景用。
    pub fn push_child(&self, child: &Node2) {
        child.attach_to(self);
    }

    /// 把子节点挂到 `index` 位置（越界即追加）。
    ///
    /// 动态区域（`for_each_node` / `if_node`）靠它把本轮新建的节点插回
    /// **区域原本所在的位置**，而不是粗暴地追加到末尾——否则区域后面的兄弟节点
    /// 会被挤到它前面（TS 的 `ctx.addNode` 是按声明序插入的）。
    pub fn insert_child_at(&self, index: usize, child: &Node2) {
        *child.0.parent.borrow_mut() = Some(Rc::downgrade(&self.0));
        let mut kids = self.0.children.borrow_mut();
        let at = index.min(kids.len());
        kids.insert(at, child.clone());
    }

    /// 从父节点摘除自己（父不存在时什么也不做）。
    ///
    /// 对应 TS 动态区域重算时"销毁旧节点"那一步：先从 `children()` 里消失，
    /// 再轮到引用计数归零。
    pub fn detach(&self) {
        let Some(parent) = self.parent() else {
            return;
        };
        parent
            .0
            .children
            .borrow_mut()
            .retain(|c| !Rc::ptr_eq(&c.0, &self.0));
        *self.0.parent.borrow_mut() = None;
    }
}
