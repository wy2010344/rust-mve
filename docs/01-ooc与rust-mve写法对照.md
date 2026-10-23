# ooc 看板 vs Rust MVE 看板：写法对照

对照对象：

- ooc 版：`C:\github\ooc\packages\example\src\ooc\pages\finance-dashboard.ooc`（402 行，拆成 `dashboard/*.ooc`）
- Rust 版：`crates/wy-app/examples/finance/`（`main.rs` + `ui/*.rs`，约 6500 行，含行情接入、图表/盘口几何算法与 38 条测试）

两边都是「树只建一次、信号驱动重绘」的 MVE。本文记录**写法层面**能不能对上、差在哪、差距是框架的还是语言的。

---

## 0. 一句话结论

目标写法（组件声明式装配 + 事件自路由 + 布局交给容器）**已经能对上**；剩下的是**样式表达力**的差距——
ooc 有 Tailwind 的 className，Rust 没有 CSS，所以每个组件仍要写一段 `draw_fn`。
但那段 `draw_fn` 现在只画自己那一小块（`current_box()` 给尺寸），不再是「全窗统一绘制 + 手算矩形 + 透明热区」。

改之前与改之后的差别，用同一件事说明：

| | 改之前 | 改之后 |
|---|---|---|
| 三列宽度 | `layout(w, h) -> [Rect; 7]` 手算 | `width_of(left, 286.0)` / `fill(center)` |
| 点自选行 | 面板整体绘制，点哪行靠 `y / ROW_H` 反算 | 每行是一个节点，`on_click` 直接知道自己是哪只标的 |
| 悬停高亮 | 引擎外记 `hover_row: Option<usize>`，全窗重绘 | 行自己持 `Signal<bool>`，`on_hover` 翻转 |
| 面板标题栏 | 手算 `y` 偏移，内容从 `y + 27` 开始画 | 标题栏节点固定高 35 + 内容 `grow: 1.0` |
| 拖拽列宽 | 引擎外记拖动状态、改 `layout()` 参数 | `splitter::gutter` 是流里的 6px 节点，拖它改相邻列的 `width_fn`/`grow_fn` |

---

## 1. 页面入口

ooc——`FinanceDashboard = fc $ [ctx => ...]`，页面就是一个函数组件，返回 `dom div`：

```ooc
FinanceDashboard = fc $ [ctx =>
    symbol = bindSignal $ 'BTCUSDT';
    ...
    (dom div { className = 'flex h-screen flex-col ...' }
        (dom header { ... })
        (dom main { ... }))
]
```

Rust——`dashboard(model, ctl) -> Node`，普通函数返回 `Node`：

```rust
pub fn dashboard(model: Rc<Model>, ctl: Rc<Ctl>) -> Node {
    let bar = topbar::topbar(Rc::clone(&model), Rc::clone(&ctl));

    // 每列内部再上下分割：stacked() = vbox[上, gutter(6px), 下]，比例由 grow_fn 给
    let left = width_of(stacked(watchlist(...), stats_panel(...), 0.52), left_w);
    let center = fill(stacked(chart_panel(...), depth_panel(...), 0.64));
    let right = width_of(stacked(book_panel(...), trades_panel(...), 0.62), right_w);

    // 列之间也是 flex：两条 6px 分割条夹在中间，拖它改 left_w / right_w
    let mut body = wy_mve::hbox_gap(PAD, vec![
        Slot::One(left),
        Slot::One(splitter::gutter(left_w, DragSpec::width(1.0, 176.0, 420.0, SIDE_W, ...), probe)),
        Slot::One(center),
        Slot::One(splitter::gutter(right_w, DragSpec::width(-1.0, 224.0, 520.0, RIGHT_W, ...), probe)),
        Slot::One(right),
    ]);
    body.align_items = AlignItem::Stretch;

    let mut root = wy_mve::vbox_gap(PAD, vec![Slot::One(bar), Slot::One(fill(body))]);
    root.padding = PAD;
    root.align_items = AlignItem::Stretch;
    root
}
```

`main.rs` 里就一句话装配：

```rust
cx.child(ui::dashboard(Rc::clone(&ui_model), Rc::clone(&ui_ctl)));
```

差距：ooc 用 `fc` 把上下文通过 `ctx` 隐式传递，Rust 里 `model`/`ctl` 显式当参数往下传（`Rc` 克隆）。
这是 `fc` 宏能做而普通函数做不到的部分；`#[component]` + `rsx!` 已经能写得更像，但示例里刻意用普通函数——
扁平函数更容易按「一个文件一个面板」拆开看。

---

## 2. 组件定义

ooc 的 `fc`，组件就是闭包，第一个参数是 `ctx`，其余是 props：

```ooc
Tab = fc $ [ctx, label, current, onPick =>
    (dom button {
        type = 'button',
        className => 'rounded-sm border px-2.5 py-1 ... ${tabClass $ ((current $) == label)}',
        onClick(e){ onPick $ label }
    } (text $ label)) $ ctx
];
```

Rust 侧有两条路，都可用：

**(a) `#[component]` + `rsx!`**——最接近 ooc 的形态：

```rust
let slot = rsx! {
    hbox {
        gap: 8.0,
        grow: 1.0,
        padding: 4.0,
        align_items: AlignItem::Stretch,
        on_click: move || {},
        on_hover: move |_entered: bool| {},
        text(|| String::from("x"))
    }
};
```

**(b) 普通构造函数返回 `Node`**——示例里用的是这条：

```rust
pub fn tab(
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    on_pick: impl Fn() + 'static,
) -> Node {
    let hover = Signal::new(false);
    Node {
        width: w,
        height: 21.0,
        on_hover_fn: Some(Rc::new(move |entered| hover_in.set(entered))),
        on_click_fn: Some(Rc::new(move |_ev| click())),
        draw_fn: Rc::new(move |any| { /* 见第 4 节 */ }),
        ..Node::default()
    }
}
```

差距：ooc 一个 `dom button` 就拿到了盒子 + 边框 + 内边距 + 文字居中 + hover 色。
Rust 里这些是**结构属性（`width`/`height`/`grow`/`padding`）+ 绘制（`draw_fn`）**两件事，必须分开写。

**这就是"退回了绘制"的残余**——但注意：现在绘制的只是**自己的 40×21 小盒子**，不是整窗。
组件不需要知道自己在屏幕哪、父容器多宽，这些都是布局给它的。

---

## 3. 桩件（dom div）与属性

| ooc | Rust（结构属性） | Rust（`rsx!` 属性） |
|---|---|---|
| `dom div` | `Node::default()` | `hbox` / `vbox` 标签 |
| `className='flex gap-2'` | `hbox_gap(8.0, children)` | `hbox { gap: 8.0 }` |
| `className='flex flex-col'` | `vbox_gap(gap, children)` | `vbox { gap: ... }` |
| `flex-1` | `node.grow = 1.0` | `grow: 1.0` |
| `w-[286px]` | `node.width = 286.0` | `width: 286.0` |
| `items-stretch` | `node.align_items = AlignItem::Stretch` | `align_items: AlignItem::Stretch` |
| `self-start` | `node.align_self = Some(AlignItem::Start)` | `align_self: ...` |
| `justify-end` | `node.justify = DirectionJustify::End` | `justify: ...` |
| `p-2` | `node.padding = 8.0` | `padding: 8.0` |
| `onClick={...}` | `node.on_click_fn = Some(Rc::new(...))` | `on_click: move ...` |
| `onMouseEnter/Leave` | `node.on_hover_fn` | `on_hover: move \|entered: bool\| ...` |
| `onMouseMove`（悬停期） | `node.on_hover_move_fn` | `on_move: move \|x: f32, y: f32\| ...` |
| `onWheel` | `node.on_wheel_fn`（`ev.wheel`） | `on_wheel: move \|delta: f32\| ...` |
| `flex-1` 但比例会变 | `node.grow_fn = Some(Rc::new(...))` | —（写函数体里） |
| `hidden` / 条件挂载 | `node.hide_fn = Some(...)` | — |
| `(text $ [...])` | `wy_mve::text(|| ...)` / 自绘 | `text(|| ...)` |

注意 `on_click` / `on_hover` 在 `rsx!` 里是**容器上也能挂**的（不是只有 `button`），
这一点和 ooc 的 `dom div { onClick }` 一致——之前的宏只认 `button`。

---

## 4. 样式：这是唯一的实打实差距

ooc：

```ooc
(dom div { className = 'rounded-sm border border-white/10 px-2.5 py-1 text-[10px] hover:bg-white/5' } 'x')
```

Rust：

```rust
draw_fn: Rc::new(move |any: &mut dyn std::any::Any| {
    let Some(scene) = any.downcast_mut::<Scene>() else { return };
    let b = current_box();                       // ← 布局分给我的尺寸
    scene.fill_round_rect(Rect::new(0.0, 0.0, b.width, 21.0), 3.0, theme::ACCENT_SOFT);
    scene.stroke_round_rect(Rect::new(0.5, 0.5, b.width - 1.0, 20.0), 3.0, theme::ACCENT, 1.0);
    theme::text_center(scene, b.width / 2.0, 5.5, label, 9.5, theme::TEXT);
}),
```

`current_box()` 是这一轮加的关键设施：thread-local 里存「当前节点分到的盒」，`draw` 期可读。
没有它，组件只能靠外部传宽度，就必然退回手算。

缺的东西（按对写法的影响排序）：

1. **没有样式表**：颜色/圆角/字号散在 `draw_fn` 与 `theme::*` 常量里。ooc 有 `tabClass`/`changeClass` 这类样式函数，Rust 只能用函数返回 `Color` 打平。
2. **没有伪类**：`hover:` / `active:` 必须自己拿 `Signal<bool>` 模拟（本仓库 `tab`/`icon_btn`/`watch_row`/`book_row` 都是这个套路）。好在套路固定，四处一致。
3. **没有文本换行/截断**：`theme::text` 是单行定宽定高，长文本溢出。ooc 靠 CSS 自动处理。
4. **没有 `position: absolute` + `z-index`**：弹层（ooc 的免责声明 modal）在 Rust 侧得自己算偏移并放到根的最后。
5. **没有百分比宽度**：`width: 62%` 要么换算成 `b.width * 0.62`（`watch_row` 的中间价就是这么算的），要么用 `grow` 权重近似。

前 4 条是**框架/渲染层**的欠缺，不是 MVE 模型的问题，可以后续补（样式表 / 伪类状态机 / 文本溢出策略 / 绝对定位层）。

---

## 5. 批量：`for_each` 对上 ooc 的 map

ooc 侧列表是数据驱动的（`(Array of ...)` 后交组件展开），Rust 侧对应 `for_each`，**keyed diff**：

```rust
let rows = for_each(
    move |emit| {                                  // 1) 声明有哪些 key（读信号，自动追踪）
        let list = m_rows.watch.get();
        for t in list.iter() {
            emit(t.symbol.clone(), t.clone());     // key, payload
        }
    },
    move |_key, ticker, cx| {                       // 2) 一个 key 一个组件（key 不变则复用节点）
        cx.child(watch_row(ticker, Rc::clone(&m_ctl)));
    },
);
```

盘口用的就是 keyed 复用该有的样子：

```rust
for (i, lv) in d.asks.iter().take(n).enumerate().rev() {
    emit(format!("a{i}"), *lv);   // key 是"第 i 档卖价"，档位不变则节点不重建
}
```

差别：ooc 那边列表是 DOM 节点数组，React 式 diff 由框架负责；
Rust 这边 `for_each` 也是 keyed diff，但**payload 是 `Rc<RefCell<T>>`**，行内更新靠 `borrow()` 读最新值——
所以行组件不必随数据重建，只有 `draw` 重跑。

---

## 6. 条件渲染

ooc 用 `#guard` 分支 + `hidden` className：

```ooc
hideUnless = [visible => #guard visible; '' #else; 'hidden'];
```

Rust 两条：
- 结构性条件（挂不挂这个节点）：`when(cond_fn, |on, cx| { if on { cx.child(node); } })`
  （= `render_when`，`cx` 上直接调，节点不挂时连构建都不做）
- 视觉性条件（挂着但不占位/不显示）：`node.hide_fn = Some(Rc::new(move || !cond()))`

`hide_fn` 为真时：不绘制、不参与布局占位、**不可命中**（命中链里直接跳过），行为等同 ooc 的 `hidden`。

---

## 7. 事件：路由方式两边一致

这条是「组件化真正的好处」，也是改这次的重点。

ooc 靠 DOM 冒泡；Rust 靠引擎沿节点树：

```text
CursorMoved ──▶ hit_test_node 递归（子优先、倒序）
                 └─ 每个节点先查 in_layout_box（点必须在盒内）
                    └─ 命中链 叶→根：[row, list, panel, root]
                       ├─ update_hover：比对链差异，派 pointerleave / pointerenter（各一次）
                       ├─ 未按下：dispatch_hover_move_chain（十字线、悬停读数）
                       ├─ 按下中：dispatch_move_chain（拖拽——分割条、K 线平移）
                       ├─ MouseInput：dispatch_click_chain
                       │    ├─ 第一轮 run_on_down（叶→根），stopped 即止
                       │    └─ 第二轮 run_on_click（叶→根），stopped 即止
                       ├─ MouseWheel：沿命中链派 run_wheel（缩放）
                       └─ apply_focus：焦点路径变化 → run_blur / run_focus
```

`on_hover` / `on_hover_move` / `on_move` / `on_wheel` 是四个不同的钩子，对应浏览器里
`pointerenter` / `pointermove` / 拖拽中的 `pointermove` / `wheel`：

| 钩子 | 什么时候派 | ooc 里的对应物 |
|---|---|---|
| `on_hover(bool)` | 进出节点各一次（链差分） | `onPointerEnter/Leave` |
| `on_hover_move(x, y)` | 未按下时每次移动 | `onPointerMove`（十字线） |
| `on_move(x, y)` | 本节点被按下后（capture 链） | 拖拽期的 `onPointerMove` |
| `on_wheel(delta)` | 指针在其上滚轮，沿链冒泡 | `onWheel` |

要点：

- 命中用**同一份 `layout_boxes`**，与 `draw_node` 完全一致，所以「所见即可点」。
- `in_layout_box` 里盒尺寸为 0 时**不可命中**（不是放行）——早期"点顶栏却触发右侧盘口"就是缺了这层。
- 显式设了 `x`/`y` 的节点不参与父容器交叉轴自动对齐（`argPosition` 覆盖 `align` 的语义，与 Kotlin 版一致）。

---

## 8. 数据接入

ooc——`createMarketSession(sym, itv, handlers)`，回调里写信号：

```ooc
handlers = {
    onSnapshot(snapshot){ ticker $ (snapshot ticker); klines $ (snapshot klines); ... },
    onTicker(t){ ticker $ t },
    onKline(k){ klines $ (upsertKline $ (klines $) k) }
};
```

Rust——`ureq` 后台线程 + `mpsc` + `on_wake` 泵进 UI 线程，一次 `batch` 写多个信号：

```text
后台线程（binance.rs） ──Msg──▶ mpsc ──▶ on_wake（UI 线程）
                                          ├─ batch(|| 写信号)      ← 一帧只重绘一次
                                          └─ 信号变 → RedrawTracker 重录 Scene
```

`batch` 对应 ooc 侧「一次回调里连写四个信号只触发一次渲染」的效果。
数据源默认本机伪接口 `http://localhost:8787/api/v3`，`WY_MARKET_API` 覆盖为真实币安。

---

## 9. 这一轮为「贴近 ooc」改的框架代码

| 位置 | 改动 | 为什么必须动框架 |
|---|---|---|
| `wy-mve/src/node.rs` | `layout_boxes(node, avail, children)` 真 flex（grow / justify / align / padding） | 没有它，容器无法替子节点定尺寸，只能手算 |
| 同上 | `current_box()` / `with_box()`（thread-local 当前盒） | 组件在 `draw` 期必须知道自己多大，否则只能由外部传宽度 |
| 同上 | `on_hover_fn` + `run_hover(bool)` + `has_handler` 纳入 | ooc 的 `hover:` 伪类要有落点 |
| 同上 | `node_size` 让显式 `width/height` **覆盖**布局容器自然尺寸 | 否则 `width: 286.0` 的列会塌成 0 |
| `wy-mve-macros` | `grow`/`padding`/`justify`/`align_items`/`align_self`/`on_click`/`on_hover` | `rsx!` 要能一行写完 ooc 的 className 意图 |
| `wy-engine/src/mve_integration.rs` | `draw_node` 传**相对**偏移（父已 `push_transform`） | 之前传绝对坐标，与 transform 叠加成双重偏移 |
| 同上 | `in_layout_box`（盒外不可命中，盒为 0 也不可命中） | 修「点 A 触发 B」 |
| 同上 | `viewport: Cell<(f32,f32)>` + `update_hover` + `find_identity` | 指针事件拿不到视口就无法算布局；hover 进出要去重 |
| 同上 | `hit_test_node` 带 `avail`，与 `draw` 共用 `layout_boxes` | 保证「所见即可点」 |
| 同上 | `dispatch_wheel_chain` + `MouseWheel` 分支（LineDelta 原值、PixelDelta / 50） | K 线/深度图缩放需要滚轮，之前引擎直接丢弃 |
| 同上 | `dispatch_hover_move_chain` + `hover_chain()` | 十字线要在"不按下"时也拿到坐标 |
| 同上 | `MveApp::root()` | 自检用：不开窗也能拿 `root.nodes()` 复核布局盒（等价 `getBoundingClientRect`） |
| `wy-mve/src/node.rs` | `on_wheel_fn` / `on_hover_move_fn` / `grow_fn` + `PointerEvent.wheel` | 滚轮、悬停移动、**动态** grow 权重（拖拽时改比例） |
| 同上 | `effective_grow()` 进入 `FlexCfg` | 上一条的落地：flex 每帧读动态权重 |
| `wy-mve-macros` | `on_wheel`（`Fn(f32)`）、`on_move`（`Fn(f32,f32)`） | `rsx!` 里一行写完 |

---

## 10. 功能面：ooc 有的，这边现在也有

| 功能 | ooc | Rust（MVE 写法） |
|---|---|---|
| 三列可拖拽 + 双击复位 | `dragSplit` + `saveLayout` + `onDblClick` | `ui/splitter.rs`：分割条是 **flex 流里的 6px 节点**，拖动改相邻面板的 `width_fn`/`grow_fn`；双击 320ms 内复位。位置仍由容器分配，分割条自己不接管坐标 |
| K 线缩放（锚定光标） | `onWheel` + `zoomAt` | `body.on_wheel_fn` + `chartview::zoom_by_wheel`，锚点用 `current_box()` + `chart_plot` 反算 |
| K 线平移 / 双击回最新 / LATEST | 拖拽 + 按钮 | `on_down_fn` / `on_move_fn`（capture 链）+ `chartview::pan_by_pixels` |
| 十字线 + OHLC 读数 + 双轴 | 绘制 | `on_hover_move_fn` 写 `chart_hover`，面板 `draw_fn` 画（`chartview::hover` 负责几何） |
| 深度图缩放 / 悬停读数 / FULL | `zoomDepth` + tooltip + 展开 | `ui/depth_view.rs`：滚轮 `exp(-wheel*0.22)`、`geometry()` 复用同一套坐标、`depth_full` 走 `when()` 换布局 |
| 盘口三列 / 模式 / 聚合步长 / 累计 / 比例条 / Avg&Sum | `orderbook.ooc` | `ui/book.rs` + `bookview.rs`：每档一个节点，`grow: 1` 均分行高，聚合与格式化是纯函数（有 6 条测试） |
| 点价 | `pickedPrice` | 点某档写 `picked_price`，顶栏/统计/中间价行都读它 |

样式全部按原 `examples/panes` 的视觉语言重建（8px 圆角面板、35px 标题栏、斑马纹行、
FAINT 小字表头、HEADER 顶栏），迁移完成后 `examples/panes` 已删除——`finance` 就是它的继任者。

**写法上没有一处回到"整体绘制"**：所有位置来自 flex，所有点击来自节点树命中，
`panels.rs` 里的绘制函数只接收「我的盒子多大 + 数据」，不接收窗口尺寸。

### 布局自检（代替肉眼看截图）

组件化之后错误的表现形式变了：不再是"画歪了"，而是**某个盒子塌成 0**
（忘了 `grow` / `Stretch`）或**溢出视口**。`main.rs` 里两条测试直接兜这一类：

```rust
layout_audit_keeps_panels_on_screen  // 所有容器盒为正、在视口内、有限值（3 种窗口尺寸）
columns_do_not_overlap               // 三列按 x 排开、互不重叠
```

实现就是把引擎的布局路径再走一遍（`children_nodes` + `layout_boxes`，与 `draw_node` 完全一致），
拿到每个可见节点的绝对盒——等价于浏览器里一次性 `getBoundingClientRect()` 全量快照。
本轮它们抓到了真 bug：盘口/成交面板外面多包了一层 `vbox`，那一层只有自然高度，
把下面的 `grow` 行全挤成 0（盘口 20 行行高 0、成交面板整块空白）。

---

## 11. 写一个新面板的套路（照抄即可）

1. **数据**：在 `Model` 里加 `Signal<T>`；`Ctl` 里加一个改它的方法。
2. **行组件**：一个函数返回 `Node`，设 `height`/`grow`，挂 `on_hover_fn`（自带 `Signal<bool>`）+ `on_click_fn`，`draw_fn` 用 `current_box()` 取宽高。
3. **列表**：`for_each(声明 key, 造行)`；外层容器 `align_items = AlignItem::Stretch`（否则行宽 0，画不出也点不中）。
4. **外壳**：`panel(title, badge, vec![Slot::One(list)])`。内容区**本身就是** `grow: 1.0` + `Stretch` 的纵向 flex，
   所以**把子节点直接给它**，不要再包一层 `vbox`——多包的那层只有自然高度，下面所有 `grow` 行都会塌成 0。
   需要固定高 + 弹性高的混排时，两者并列传进去即可（盘口就是 `表头 / 卖盘(grow) / 中间价 / 买盘(grow) / 比例条`）。
5. **装配**：在 `ui/mod.rs` 的 `dashboard()` 里塞进某一列，给 `grow` 权重或 `width_of(_, 286.0)`。

不需要：算坐标、注册热区、维护 `hover_row` 索引、改 `layout(w, h)`。

---

## 12. 还没对上的

- 下单面板（`ticket.ooc`）与免责声明弹层（`panel.ooc` 的 DEMO 常驻文案，合规相关，真产品必须有）
- 文本溢出与换行（`draw_fn` 里没有 ellipsis，长数字会把列撑破）
- 样式表化（`theme::*` 目前是常量，改主题要改代码；ooc 侧是 Tailwind class 拼接）
- 布局存盘（ooc 的 `saveLayout` 把列宽写 localStorage，Rust 侧还没有持久化层）
- 无头截图（GPU 管线在 `runner.rs` 里，还没有"离屏渲染到 PNG"的入口；目前靠 §10 的布局自检顶）

前三项是框架能力，第四项是宿主侧功能。

