# 无 `rsx!` 写法试点 & 组件系统改进清单

> **备注（文件位置已变）**：本文提到的 `examples/todo.rs` / `examples/todo2.rs`
> 在迁移到纯 Node2 运行时后已删除，对应示例现在是 `examples/node2/todo.rs`
> （`cargo run -p wy-app --example node2-todo`）。文中关于写法与组件系统的讨论不受影响。

> **现状勘误（务必先读）**：第 1～4 章是**第一代实验的原始记录**——当时还在
> 「`rsx!` + `#[component]` + `IntoGetter`」的框架里论证"无 `rsx!` 能不能写"。
> 那套结论**已被第 5 章取代**：
>
> - `rsx!` / `#[component]` / `#[fc]` / `#[tfc]` / `.of()` / `IntoGetter` 已**全部删除**，
>   连同它们依赖的 `Node` IR 一起（2026-11）；
> - 现在的唯一声明姿势是 `#[rec]` + `#[getter]` + `node2!` / `flex!`（见第 5 章）；
> - 第 2、3 章里那些 "P0/P1 改进" 也大多已被新体系直接解决（getter 下沉、
>   `flex!` 收容器属性、`btn!` 自定义底板），保留只为还原决策过程。
>
> 要学"现在怎么写"，直接跳到 [第 5 章](#5-第二代体系rec--getter--node2--layoutdir)。

`docs/02` 讲「用 `rsx!` 怎么写」。这份文档做一件相反的事：**完全不写 `rsx!`**，
只用 fc 函数（`node` / `vbox` / `hbox` / `text_field_opts` / `for_each` / `scroll`）
加 `Node::with_*` builder，写一个真能增删勾选的 todo 组件
（`examples/todo.rs`），用它检验「贴近 ooc 写法」时组件系统还差什么。

当时的结论（**已被第 5 章推翻**）：

> **`rsx!` 不能丢，但无 `rsx!` 这条路必须能走通。**
> 本次实验证明无 `rsx!` 能把树建出来、交互也正常；但静态属性的「动态化」
> 在无宏路径上异常 painful——而 `rsx!` 靠 getter 免费解决了。
> 所以改进方向不是「二选一」，而是**把 getter 能力从宏下沉到 builder API**，
> 让两条路共享同一套动态属性机制。

---

## 1. 实验产物

| 文件 | 作用 |
|---|---|
| `examples/todo.rs` | 无 `rsx!` 的完整 todo 组件（增 / 删 / 勾选 / 计数 / 清除已完成） |
| `examples/ooc_preview.rs` | 更小一版的金融看板无 `rsx!` 预览（`#[component]` + `#[getter]` 形态） |
| `docs/images/shot-todo.png` | `todo.rs` 离屏快照 |
| `docs/images/shot-ooc-preview.png` | `ooc_preview.rs` 离屏快照 |

状态模型刻意对齐 `docs/02` 第 5.5 节：**结构信号 `Signal<Vec<u64>>` + 每项独立信号**
（`Signal<String>` / `Signal<bool>`），不用 `Signal<Vec<Todo>>`。勾选只写该项 `done` →
只那一行重绘；增删只写 `ids` → `for_each` 按 key diff。

渲染验证：`cargo run -p wy-app --example todo -- --snapshot docs/images/shot-todo.png`
（1440×900，结构 / 勾选 / 删除线 / 分隔线 / 状态行全部在位）。

---

## 2. 无 `rsx!` 能写，但踩到 6 个框架缺口（FINDING）

每个都来自 `examples/todo.rs` 里的 `// FINDING:` 注释，按痛点排序。

### F1 — 容器要「面板感」只能手绘底板（最高频）
`scroll` / `vbox` 是组件，调用处**不能往它的根节点挂 `draw`**（会覆盖它自己的绘制）。
想要圆角底板只能在外面再套一层 `vbox` 画。每个面板都要重写一遍这段：

```rust
.with_draw(|s| {
    let b = current_box();
    s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 8.0, PANEL);
})
```

**改进**：给 `Node` builder 加一等公民的视觉属性——
`with_background(Color)` / `with_radius(f32)` / `with_border(Color, f32)`，
一行替代上面五行的 `draw` 样板。

### F2 — 根容器默认 `Center` 不铺满（最反直觉）
`Node` 默认 `align_items: Center`（Kotlin 移植遗产），而 ooc / CSS 默认 `Stretch`。
结果每个想铺满窗口的页面都要显式写 `.with_align_items(AlignItem::Stretch)`。

```rust
// todo.rs 根 vbox 必须显式 stretch，否则整页被挤成居中窄条
.with_align_items(AlignItem::Stretch)
```

**改进**：把默认 `align_items` 从 `Center` 改为 `Stretch`，`Center` 留给显式声明。
需评估对既有示例 / 测试的影响（finance 看板、shadcn 卡都依赖当前行为），
**这是个破坏性变更，单独开 PR + 跑全量快照回归**。

### F3 — 动态文本颜色要把整个 `draw` 重写（最说明问题）
`text_styled(text, size, color)` 的 `color` 是静态参数。完成态要变色，
只能整块放弃 `text_styled`、自己 `measure_text` + `draw_text` + 画删除线：

```rust
text_styled(move || text.get(), 13.0, TEXT).with_draw(move |s| {
    let t = text.get();
    let is_done = done.get();
    let (_, th) = measure_text(&t, 13.0);
    let b = current_box();
    let y = (b.height - th) / 2.0;
    s.draw_text(Point::new(0.0, y), &t, 13.0, if is_done { FAINT } else { TEXT });
    if is_done { /* 画删除线 */ }
})
```

而 `rsx!` 版一行就解决：`text_label { color: move || if done.get() { FAINT } else { TEXT } }`
——**getter 在此处的价值暴露无遗**。

**改进（最高杠杆）**：把 `IntoGetter` 下沉到 builder API。
所有视觉属性（`color` / `text_color` / `bg_color` / `width` / `height` / `gap` / 命中相关）
同时提供 `with_x(v)` 与 `with_x_fn(impl Fn() -> T)`，且统一成
`with_x(impl IntoGetter<T>)`——静态值 `COLOR` 和闭包 `move || ..` 同入口。
这样无 `rsx!` 路径免费获得 getter 语义，**F3 / F5 / 部分 F1 一次性闭合**。

### F4 — 主轴没有 `justify_content`，growing spacer 要手搓
「把删除钮推到行右缘」「把清除钮推到状态行右缘」都靠手写 `node(()).with_grow(1.0)`。
框架 `spacer(w, h)` 只是定尺寸空节点（0 尺寸不生长），finance 里也有一个本地
`flex_spacer`——**两个示例各造一遍轮子**。

**改进**：加 `spacer()`（无参、grow 吃掉剩余主轴空间）或一等公民的
`justify_content(Justify)`。前者零成本、立刻消灭重复样板。

### F5 — `text_styled` 缺 `color_fn` 变体
同 F3，文字 / 字号 / 对齐的动态化都应走 getter，不应逼用户绕回裸 `draw`。
（落地见 F3 的 `IntoGetter` 方案。）

### F6 — 自定义按钮每个都要重写「底板 + 文字 + 点击」三件套
框架 `btn` 不吃颜色 / 圆角参数（亮色主题写死），深色 UI 只能自建：

```rust
fn primary_btn(text: &'static str, on_click: impl Fn() + 'static) -> Node {
    hbox([text_styled(move || text.into(), 12.5, BG)])
        .with_pad_x(14.0).with_pad_y(6.0)
        .with_on_click(on_click)
        .with_draw(|s| { /* 画 accent 圆角底板 */ });
}
```

**改进**：加 `btn_opts`（对标 `TextFieldOpts`）——`text` / `text_color` / `bg_color` /
`radius` / `on_click`，深色主题也能一行声明。

---

## 3. 改进清单（按杠杆排序，直接可排期）

| 优先级 | 改进 | 闭合 FINDING | 成本 | 风险 |
|---|---|---|---|---|
| **P0** | `Node::with_*` 接受 `impl IntoGetter<T>`（color/width/height/gap/bg…） | F3 / F5 | 中 | 低（API 扩展） |
| **P0** | 容器视觉属性 `with_background` / `with_radius` / `with_border` | F1 | 低 | 低 |
| **P1** | 默认 `align_items: Stretch`（破坏性，单独 PR + 快照回归） | F2 | 低 | 中（全仓回归） |
| **P1** | `spacer()` growing 空节点 / `justify_content` | F4 | 低 | 低 |
| **P2** | `btn_opts` / `text_field` 主题钩子 | F6 / F3 | 低 | 低 |

**P0 两项是「让无 `rsx!` 路径真正可用」的硬门槛**：没有 getter 下沉，无宏写法
在动态视觉上就是手写 `draw` 地狱，那 `rsx!` 永远没法退居可选。

---

## 4. `rsx!` 去留的结论

> **⚠️ 本节结论已作废**。当时的判断是"`rsx!` 保留为首选、无宏路径作逃生舱"，
> 因为它靠宏免费拿到 getter 语义。第 5 章的 `#[rec]` 把同一份能力做进了
> `!` 宏（属性即表达式），于是"无宏路径样板重"这个前提消失了——现在
> **所有 demo 都是无 `rsx!` 写法**，`rsx!` 反倒成了待删项。以下保留原文，
> 只为记录当时的权衡依据。

**保留 `rsx!` 为首选语法**，理由（这次实验实锤）：

1. **getter 免费**：`color: move || ..` 一类动态属性，宏自动包 `*_fn`；
   无宏路径必须 `with_x_fn` 或退回裸 `draw`——只有 P0 落地后才等价。
2. **分支 / 列表在表达式层**：`if` / `when` / `for_each` / `..spread` 只能活在 `rsx!`。
3. **组件复用**：`#[component]` + `#[getter]` 的 Props 契约无宏路径要手写。

**但无 `rsx!` 路径必须保留作逃生舱**，理由：

- `#[component]` 装不下大闭包 body（`docs/02` 里 `list_row` 的教训）——这类组件
  只能「普通函数 + 内联 `rsx!`」或「fc 函数 + builder」。本次 `todo.rs` 证明
  fc + builder 这条路走得通，只是 P0 落地前样板偏重。

**最终分工（与 `docs/02` 一致，补强无宏路径）**：

| 场景 | 写法 |
|---|---|
| 绝大多数 UI（带动态属性 / 分支 / 复用） | `rsx!` + `#[component]` + `#[getter]` |
| 大闭包 body 的底层组件（`list_row` 类） | fc 函数 + `Node::with_*`（P0 后舒适） |
| 注入型（`provide` / `render_for_each` / `render_when`） | ctx 组件（`cx: &mut NodeContext`） |

---

## 5. 第二代体系：`#[rec]` / `#[getter]` / `node2!` / `LayoutDir`

> **本节已按最终形态重写。** 早期版本的 `#[tfc]` / `#[fc]` 两个组件注解、
> `.of()` 两段式、`layout: Option<Flex>` 都已淘汰。现在**只有一个组件注解 `#[rec]`**：
> 产物类型由函数签名决定，调用处不需要 `.of()`。

### 5.1 `#[rec]`：声明即组件，调用即 `!` 宏

```rust
// 声明
#[rec]
fn btn(text: String, #[getter] font_size: f32, on_click: impl Fn() + 'static) -> Fc { .. }

// 调用（唯一姿势：花括号一次性声明，不走 pipeline，也不写参数结构体）
btn! { text: "保存", font_size: 12.5, on_click: move || save() }
```

`#[rec]` 生成三样东西：

| 产物 | 作用 |
|---|---|
| `BtnArg` | 参数结构体（字段类型按 5.2 的三态规则推导） |
| `BtnArg::builder()` | 链式 setter，`!` 宏的展开目标 |
| `btn!` 宏 | `{ k: v, .. }` → `builder().k(v)..build()` |

**返回类型就是签名上写的那个类型**——`-> Fc` 给 `Fc`，`-> LayoutDir2` 给
`LayoutDir2`（`flex!` 正是这样产出一条布局策略）。没有 `of` / `create` /
`call(node)` 三件套：节点句柄是**普通参数** `node_ref`（不传就内部新建空槽，
调用方拿不到句柄），要提前拿到句柄就写 `node_of(|get| ..)`。

外层只有两个指令：

| 指令 | 作用 |
|---|---|
| `#[rec(arg = XxxProps)]` | 给生成的参数结构体改名（默认 `XxxArg`），用于避开同名手写类型 |
| `#[rec(extends = base)]` | 继承基类的全部 props（基类字段在 `!` 宏里内联转发） |

没有 `defaults(..)` / `required(..)` 这种分组指令——**注解跟着字段走**，读签名
就能看全，不用在两处对照：

### 5.2 `#[getter]`：属性即表达式

标了 `#[getter]` 的字段，`!` 宏把右值**包成 `move || 右值`**，到布局期/绘制期才求值：

| 调用处写法 | 展开 |
|---|---|
| `font_size: abc.get()` | `move || abc.get()`（每次重算） |
| `font_size: 12.5` | `move || 12.5` |
| `text: format!("{}", n.get())` | `move || format!(..)` |
| `text: readout`（`Get<T>` 本身） | ✗ 要写 `readout()`——括号即求值 |

所以**不需要 `always(..)`，也不需要在调用处写 `move \|\|`**。反过来，
"把一个现成 `Get<T>` 往下传"要显式加括号，这是唯一的坑。

字段存储按声明分三态（判定顺序：`children` → `#[getter]` → `impl Fn` → `Option<T>`
→ `Vec<T>` → 必填）：

| 写法 | 存储 | 缺省 |
|---|---|---|
| `#[getter] focus_order: Option<i32>` | `Get<Option<i32>>` | `None`（= Kotlin `Int? = null`） |
| `#[getter] #[props(default = false)] hide: bool` | `Get<bool>` | `false` |
| `#[getter] width: f32` | `Option<Get<f32>>` | `None` = 未声明 |

第一态与第三态别混：`Option<T>` 是**值可空**（`focus_order` 本来就可以没有顺序），
无默认值的裸类型是**声明可省**（`width` 不写 = 引擎走自动尺寸）。
能用默认值就别用 `Option`——这是对齐 Kotlin 结论后的取舍。

三条字段级注解彼此正交，各管一件事：

| 注解 | 管什么 | 对存储的影响 |
|---|---|---|
| `#[getter]` | **怎么读**：每次求值重算 | `Get<T>` / `Option<Get<T>>` / `Get<Option<T>>` |
| `#[props(default = ..)]` | **缺省值** | 有值即 optional |
| `#[props(required)]` | **强制必填**（把按类型本可省略的拉回必填） | 剥掉外层 `Option` |

`#[props(required)]` 只用在"类型本身带可省略语义、但业务上必须由调用方给出"的
字段上（`text: String` 的 getter、`on_click: impl Fn()` 这类）。与
`#[props(default = ..)]` 互斥，同时写是编译错误。

### 5.3 children 收 `Vec<Fc>`

TS `Node2Arg.children?: readonly Fc[]`，Rust 逐字对应 `children: Vec<Fc>`：

```rust
node2! {
    children: vec![
        label! { text: "行一" },              // 自定义 #[rec] 组件，返回 Fc，直接进 vec
        node_of(|get| { .. }),                // 需要自身句柄时
        for_each_node(ids, |id, cx| ..),      // 结构组件，本来就是 Fc
        if_node(|| show(), |cx| .., None),    // 同上
    ],
}
```

**没有 `children!` 之类的转换宏，也没有 `.of()`**：`node2!`、各 `#[rec]` 组件、
`scroll_content!` / `scroll_bar!`、结构组件**全部直接产出 `Fc`**，元素类型只有
一种，`vec![..]` 直接写。需要提前拿句柄才写 `node_of(|get| ..)`。

### 5.4 `Node2` 本体：读取式属性 + `Flex` + `exts`

旧的 `Node` 是**引擎 IR**：`width: f32` + `width_fn: Option<Fn>` 成对出现，
flex 相关字段（`grow` / `align_items` / `gap` / `justify_content`）直接长在节点上。
那是给布局器吃的扁平结构，**不该是声明层**。所以第二代另起一个 `Node2`：

```rust
pub struct Node2(Rc<Node2Inner>);   // Clone = 浅拷贝，所有副本共享同一节点
```

三段式里那个 `{} as Node2` 就是这个 `Node2`（`Node2::empty()`）。

**① 属性全部是读取式 `Get<T> = Rc<dyn Fn() -> T>`**

TS 侧把 `width` / `height` / `padding` / `hide` / `cursor` / `focusable` …
统一成 `GetValue<T>`（大部分是 lambda），Rust 逐字对应：

| TS `Node2Arg` | `!` 宏里写什么 | 存储 |
|---|---|---|
| `width?: GetValue<number>` | `width: 220.0` / `width: w.get()` | `Option<Get<f32>>` |
| `height?: GetValue<number>` | 同上 | `Option<Get<f32>>` |
| `x?()` / `y?(): number` | `x: 0.0` / `x: goff.get()` | `Option<Get<f32>>` |
| `size?(s)` / `position?(s)` | 低层逐轴覆盖，罕用 | `Option<Rc<dyn Fn(Axis) -> _>>` |
| `paddingInlineStart` …（四向） | `padding_inline_start: 12.0` | `Option<Get<f32>>` |
| `hide` / `notInLayout` / `skipDraw` / `selectionEnabled` | `hide: flag.get()` | `Option<Get<bool>>` |
| `focusable` / `focusOrder` / `focusTrap` | `focus_order: Some(3)` | `Get<Option<i32>>` 等 |
| `cursor?: GetValue<CursorType>` | `cursor: CursorType::Pointer` | `Get<CursorType>` |
| `draw?(ctx2, n)` | `draw: \|s, n\| { .. }` | `Option<Rc<dyn Fn(&mut Scene, &Node2)>>` |
| `mouseClick` … / `handleKey` | `mouse_click: move \|e\| { .. }` | 按值持有的回调 |

语义要点：**没有静态/动态两套槽位**，也不需要记「这个字段有没有 `_fn` 变体」——
写表达式即可，宏负责包 `move ||`（见 5.2）。`220.0` 是常量表达式，
`w.get()` 是响应式表达式，两者在同一入口。

`padding` 是**简写**：四向各自未声明时回退到它（`padding_inline_start
.unwrap_or(padding)`），对应 TS `getPadding(node, dir, side)` 的回落顺序。

**② 容器布局是 `LayoutDir` 策略，子节点伸展收进 `exts`**

`layout` **不是** `Option<Flex>`：它对齐 Kotlin `interface LayoutDirection { layoutX; layoutY }`，
是一条**布局策略句柄**，`Flex` 只是其中一种实现：

```rust
pub trait LayoutDir {                        // Kotlin interface LayoutDirection
    fn layout_x(&self) -> Rc<dyn LayoutFun2>;
    fn layout_y(&self) -> Rc<dyn LayoutFun2>;
    fn dir(&self) -> Layout { Layout::Column }   // 默认实现，非 flex 给中性值
    fn gap(&self) -> f32 { 0.0 }
    fn flex(&self) -> Option<&Flex> { None }
}
pub struct LayoutDir2(Rc<dyn LayoutDir>);    // 声明处传的句柄
```

实现方两个：`Flex`（`flex! { .. }` 的产物）与 `AbsoluteLayoutDir`（`absolute()`）。
`LayoutDir2::default()` **就地是绝对定位**——所以"没写 layout"不是一个空 `None`，
而是"选了绝对定位这条策略"，不用调用方先判空再分支。

```rust
// 老：flex 字段散在节点上
Node { layout: Some(Layout::Column), align_items: AlignItem::Stretch, gap: 14.0, grow: 1.0, .. }

// 新：容器属性走 flex!，子节点身份走 exts
node2! {
    layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, gap: 14.0 },
    exts: vec![wy_mve::grow(1.0)],          // TS grow(1)
}
```

`Flex` 对应 TS `flex({..})`：`direction` / `align_item` / `align_fix` /
`direction_justify` / `gap` / `reverse` / `direction_fix_between_when_one`。
每个参数都在**字段上**带默认值（`#[props(default = ..)]`），省略即取默认：

```rust
#[rec]
pub fn flex(
    #[getter] #[props(default = Layout::Column)] direction: Layout,
    #[getter] #[props(default = AlignItem::Center)] align_item: AlignItem,
    #[getter] #[props(default = 0.0f32)] gap: f32,
    /* align_fix / direction_justify / reverse / direction_fix_between_when_one */
) -> LayoutDir2 { .. }
```

`exts: Vec<Ext>`（`Ext = Rc<dyn Any>`）
对应 TS `exts?: readonly any[]`，内置三种，用 `find_ext::<T>()` 按类型取
（= TS `findExt`）：

| ext | TS | 读取方式 |
|---|---|---|
| `grow(n)` / `grow_xy(x, y)` / `grow_dyn(..)` | `grow` | `GrowChild::grow(axis)` |
| `align(a)` / `align_xy(x, y)` | `align` | `AlignChild::align(axis)` |
| `ignore_flex()` / `ignore_flex_dyn(..)` | `ignoreFlex` | `IgnoreFlexChild` 存在即生效 |

于是布局器需要的扁平字段（`grow` / `align_self` / `ignore`）由
`Node2::to_node()` **在交界处算出来**，不是让调用方写两遍。

**③ `to_node()` 是两层唯一交界**

```rust
pub fn to_node(&self) -> Node {          // 声明层 → 引擎 IR
    let axis = axis_of(self.layout.dir());          // LayoutDir 默认实现给中性值
    let grow = find_ext::<GrowChild>(&self.exts).map_or(0.0, |g| g.grow(axis));
    let align_self = find_ext::<AlignChild>(&self.exts).and_then(|a| a.align(axis.opposite()));
    Node { width_fn: /* 读取式闭包 */, height_fn: .., pad_left_fn: .., grow, .. }
}
```

读取式属性落到引擎的 `*_fn` 槽，所以**布局期每次重算都重新求值**：
信号变了下一帧几何就变，不存在「构造期快照。」

### 5.5 焦点 / 光标也是读取式的

TS 这轮把 focus 与 cursor 一族全改成 lambda，Rust 侧：

```rust
node2! {
    focusable: is_active(),                            // 表达式，宏包 move ||（下同）
    focus_order: Some(3),                              // Get<Option<i32>>，省略即 None
    focus_trap: false,
    on_focus: move || { .. }, on_blur: move || { .. }, // 回调按值持有，不包
    handle_key: move |e: &mut KeyEvent| -> bool { .. },// true = 已消费
    cursor: CursorType::Pointer,
}
```

焦点状态存 `thread_local! Signal<u64>`（`canvas_render2/focus.rs`），值是
**节点 id**（0 = 无焦点）——不存 `Rc<Node2>`，避免信号让节点永不释放。
节点侧 `is_focused()` / `request_focus()` 按自己的 `id` 比对。

### 5.6 语义与 TS 一致：不做保护

几何读取是**直读**（`Node2::geom()` / `NodeRef` 读同一个 `Rc<Cell<NodeGeom>>`，
引擎在布局盒分配后、绘制子树前写入）。**没有**常量快照、**没有**一帧延迟、
**没有**重入检测——在 `width()` 里读 `node.height()` 会像 TS 一样栈溢出，
写错是写的人的锅。

### 5.7 与 TS `commonNode.draw` 的三处逐字对齐

`Node2::to_node()` 是声明层→引擎 IR 的唯一交界，以下三处已对齐 TS 验证过的逻辑：

1. **默认命中盒**（`accept_hit`）。没显式给 `accept_hit` 时，TS `commonNode.draw`
   的补默认是 `x>0 && y>0 && x<width && y<height`（严格在盒内、不含负坐标）。
   原先 `to_node` 不落 `hit_test_fn`，落到引擎 `Node::default()` 的 `|_,_| true`
   （全空间可命中）——已改为 `to_node` 在无显式 `accept_hit` 时回落盒子命中
   （读节点 geom 的 width/height）。`Node2::accept_hit` 方法层也实现了同一默认，
   两层一致。
2. **`draw_at_parent` 真正接入绘制循环**。TS `commonNode.draw` 对每个 child 先
   `child.drawAtParent(ctx2)`（父坐标系下）、再 `translate(child.x(), child.y())`、
   再 `child.draw`。引擎 `draw_node` 之前从没调用过 `run_draw_at_parent`
   （它只是个死字段）——已在两个子节点循环分支里、递归 `draw_node`（即
   `translate`）**之前**、`!child.skip_draw` 时调用 `child.run_draw_at_parent(scene)`，
   与 TS 顺序逐字一致。
3. **`flex({ directionFixBetweenWhenOne })` 贯通**。TS `FlexLayout` 有该字段、
   引擎 `wy-layout` 也有对应枚举与分支，但 Rust `Flex` / `FlexCfg` / `Node` 之前
   都没接。现已加 `Flex::direction_fix_between_when_one(..)`，经
   `to_node` 落到引擎 `Node.direction_fix_between_when_one`，再经 `FlexCfg`
   实现 `FlexObject::direction_fix_between_when_one()` 驱动 `wy-layout`
   单子节点 `Between` 落位。无 flex 时回落 `Center`（与 `FlexObject` trait 默认一致）。

回归测试见 `crates/wy-engine/src/mve_integration.rs`：
`node2_default_accept_hit_is_box_only` / `node2_explicit_accept_hit_overrides_default` /
`node2_draw_at_parent_called_in_parent_space` /
`node2_direction_fix_between_when_one_passes_through`。

### 5.8 `scroll` 迁进新体系：删掉最后一个"桥回旧 `Node`"的 hack

在此之前，`canvas_render2` 里唯一还连着旧体系的地方是：

```rust
// crates/wy-mve/src/canvas_render2/nodes.rs
pub use crate::scroll as scroll_node;   // 旧 `#[component]` + Signal + Node
```

TS 的 `Scroll.ts` / `ScrollBar.ts` 本来就是 `canvasRender2` 的一部分（`createScroll` /
`scrollContent` / `scrollBar` 全是 `node2.create` + `tfc`），所以 Rust 侧也必须用
`Node2` 写，而不是桥接。现已新增 `crates/wy-mve/src/canvas_render2/scroll.rs`（纯
`Node2`），并**删除**上面那行重导出——新旧体系只剩 `Node2::to_node()` 一个交界。

对外三段与 TS 同名同形：`Scroll2::new` / `scroll_content` / `scroll_bar`。
后两者都是 `#[rec(extends = node2)]` 组件，直接产出 `Fc`；句柄走普通参数
`node_ref`（要让外部拿到视口/滑块节点时才传）。`examples/node2/scroll.rs` 里的
`scroll_view` 只是把这几段组合起来：

```rust
#[rec]
fn scroll_view(
    #[props(default = 4.0f32)] gap: f32,
    #[props(default = 8.0f32)] pad: f32,
    children: Vec<Fc>,
) -> Fc {
    node_of(move |get: Ref2<Node2>| {
        let scroll = create_scroll(&get.get(), Axis::Y);
        scroll_content! { scroll: scroll.clone(), layout: flex! { .. } }
        // .. 再挂 scroll_bar!、组装视口容器
    })
}

// 调用处：一行
scroll_view! { children: rows }
```

`scroll` 的便捷封装（`ScrollOpts`）属于**旧体系**（`crates/wy-mve/src/scroll.rs`），
新层不提供"一行搞定"的隐藏默认值——容器的方向 / 内边距 / 间距属于业务声明，
留在 `scroll_view` 的参数里，全在明面上。

三处**机制映射**（不是妥协，是引擎原语差异，代码里逐条注释）：

| 关注点 | TS | Rust |
|---|---|---|
| 滚动偏移 | `position(d)` 整体覆盖布局位置 | 容器布局方向取滚动方向的**反向**，偏移落**交叉轴** `y = -value`（引擎 `x_fn`/`y_fn` 是叠加在布局盒上的偏移，且只在交叉轴抑制自动对齐）；布局盒已含 padding，故 `-value` 与 TS 的 `pad - value` 等价 |
| 溢出裁剪 | `scrollContent.drawAtParent` 里 `clip()` | 视口 `clip: true`（引擎 `push_clip`/`pop_clip` 包裹整棵子树，且 `hit_test_node` 同步裁剪）；引擎对 `run_draw_at_parent` 无 save/restore，直接 `clip()` 会污染后续绘制 |
| 滚动条定位 | track 作普通 flex 子节点（位置未指定） | `not_in_layout` + 显式 `x`/`y` 贴视口内缘，浮层覆盖 |
| 内容尺寸 | 扫 `container.children()` 找 `isScrollContent` | `Scroll2::attach_content` 直接持句柄（`Node2::children()` 在**挂载期**才填，构建期扫不到）；`isScrollContent` 标记保留供对照 |

**"尺寸写死、缩窗口不滚"的根因**：旧 `scroll` 的 `w`/`h` 是名义自然尺寸，viewport 默认
没有 `grow`，所以除非调用处显式给 `grow: 1.0`，它就是个固定 footprint；而 `grow` 又只在
"父容器主轴尺寸确定"时才分配剩余空间。新写法在 `scroll_view` 里用
`exts: vec![grow(1.0)]` 贯穿两级容器，滚轮 / 裁剪 / 滚动条一个不少。

像素级验证（`docs/images/shot-scroll2-*.png`）：视口高 820 时滑块 ≈343px、高 360 时 ≈39px、
`todo2` 压到 170px 高时滑块重新出现且图元数 107→108——即"窗口变矮立刻可滚"。
单元测试见 `canvas_render2::scroll::tests`（6 条：钳制、滑块几何、`len` 参数、
双向换算互逆、无内容不滚）。

---

## 6. 现状与后续

**已落地**（本文第 5 章即最终形态）：

- 第二代声明层：`#[rec]`（唯一组件注解）+ `#[getter]` 三态 + `node2!` / `flex!` +
  `node_of`；`counter` / `todo` / `abs` / `scroll` / `canvas2_text` 五个 demo 全部
  只写 `!` 宏，零 `XxxArg{..}.of()`、零 `current_box()`。
- `LayoutDir` trait 取代 `Option<Flex>`：缺省即绝对定位，不再有"空 layout"语义。
- `Node2` 的读取式属性落引擎 `*_fn` 槽，布局期逐帧重求值（引擎测试
  `node2_reading_props_reevaluate_each_frame` 锁定）。
- 事件层补上 `Click` 合成（宿主在按下时派发），Node2 demo 的按钮才真正可点。

**后续**：

- **#57** 让 demo 彻底不依赖旧体系：只剩 `text_field_opts` / `TextFieldOpts`
  （旧 `text_field.rs`）与 `text_styled`（`components.rs`）。下一步按
  TS `EditableTextNode.ts` 复刻 `canvas_render2::editable_text`，再把 `text_styled`
  换成 `wrapped_text_node` / `rich_text_node`。
- **#56** 把 `Node2` 的读取式属性接通事件层（5.7 的三处已对齐，剩下）：
  `cursor` 真正切换系统光标、`focus_order` 参与 Tab 遍历排序
  （当前焦点只存 id，遍历顺序还是文档序）；`activeEditor` 编辑器路由缺失
  （TS `Focus.ts` `requestFocus` 会登记活跃编辑器，Rust 侧无此概念）。
- **库级组件已收口**：`wrapped_text_node` / `editable_text_node` / `scroll_content` /
  `scroll_bar` / `rect_node` / `image_node` / `rich_text_node` 全部迁到
  `#[rec(extends = node2)]`，签名不再是 `XxxArg { rest: .., .. }`，调用处零 `.of()`。
- **`Node2Arg` 已并入 `Node2Props`**：节点参数只有一层，`Node2::build` 直接吃
  `node2` 组件的 props。`extends = node2` 注入的 `rest` 就是一份 `Node2Props`，
  组件体内写 `node2! { ..rest, 覆盖项 }` —— 这是 TS 的
  `interface XxxArg extends Node2Arg` + `...rest` 的**逐字对应**，不再需要
  "声明层 props → 引擎 IR" 的转接。布尔开关也从 `Option<Get<bool>>` 收敛成
  `Get<bool>`（默认值已在声明层，再区分"没写"没有语义）。
- 库内唯一残留的 `required` 是 `editable_text_node` 的 `text`（文本源没有合理
  缺省，TS 兜空串属偏离）。
- **#53**（已完成）`examples/shadcn` 整块迁到 canvasRender2，v1 版本删除；
  快照、行数、断言方式的对照见 `docs/07-旧体系迁移实测.md`。
- **#54** 门禁脚本化（build 全 example + test + clippy `-D warnings` + fmt），沙箱 `.exe` 写入对策写进脚本。
- 旧 `#[component]` / `rsx!` / `IntoGetter` 待所有 demo 迁移后删除。
