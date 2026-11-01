# 05 · canvasRender2 对标审计（逐文件）

> 结论先行：**不是 1:1**。22 个 TS 文件里，8 个真正对齐、7 个部分对齐（含
> 明显偏离）、7 个缺失。最严重的问题不在细节，而在**基础组件的主构建路径
> 整体绕开了 `Node2`**。

审计时间：2026-10-26。TS 源：`packages/mve/dom-helper/src/canvasRender2`。

---

## 1. 逐文件对照

| TS 文件 | 行 | Rust 对应 | 判定 |
|---|---:|---|---|
| `Node.ts` | 616 | `node2.rs` 1301 + `fc.rs` 216 + `element.rs` 249 | ⚠️ 拆三份可接受，但有 TS 没有的扩展字段 |
| `Renderer.ts` | 867 | `nodes.rs::render_canvas2` + `renderer.rs` | ⚠️ 声明根(≈60行)+全局鼠标注册表已落地；事件/重绘/IME/键盘 由引擎统一提供（§5.1） |
| `SelectionManager.ts` | 358 | `selection.rs` 108 | ❌ **模型不同**（见 §3.5） |
| `Geometry.ts` | 346 | `geometry.rs` 449 | ✅ |
| `EditableTextNode.ts` | 983 | — | ❌ **完全缺失** |
| `RichTextNode.ts` | 253 | `nodes.rs::text_node` | ⚠️ 走旧 `Node`（§3.1） |
| `Scroll.ts` | 231 | `scroll.rs` 619 | ⚠️ 三处机制映射（§3.6） |
| `Focus.ts` | 126 | `focus.rs` 71 | ❌ 缺 `EditableTextHandle` / `activeEditor` |
| `ImageNode.ts` | 98 | — | ❌ 缺失 |
| `ScrollBar.ts` | 98 | `scroll.rs` 内 | ⚠️ 合并 |
| `WrappedTextNode.ts` | 81 | `nodes.rs::wrapped_text_node` | ⚠️ 走旧 `Node` |
| `RectNode.ts` | 79 | `nodes.rs::rect_node` | ✅ **本轮已对齐结构**（§4） |
| `ForEachNode.ts` | 57 | `nodes.rs::for_each_node` | ✅ |
| `TabRender.ts` | 44 | `tab.rs` 47 | ✅ |
| `TextDefaults.ts` | 31 | `text_defaults.rs` 37 | ✅ |
| `index.ts` | 31 | `mod.rs` 48 | ✅ |
| `UrlOpener.ts` | 17 | `url_opener.rs` 24 | ✅ |
| `flex.ts` | 13 | `node2.rs::flex()` | ✅ |
| `util.ts` | 25 | `geometry.rs` 内 | ⚠️ 位置不同 |
| `Graphemes.ts` | 5 | — | ❌ 纯 re-export |
| `UndoRedo.ts` | 5 | — | ❌ 纯 re-export |
| `Clipboard.ts` | 7 | — | ❌ 纯 re-export |

**8 对齐 / 7 部分 / 7 缺失。**

---

## 2. 一句话结论

`canvas_render2/` 里的**模型层**（`Node2` / `Fc` / `Ref2` / `flex` / `exts`）
是认真对标的；但**组件层**（`nodes.rs`）长期是"旧引擎 IR 的伪装"——
自己造 `Node` 塞进父上下文，而不是像 TS 那样用 `node2({...rest, draw})`
组合出来。

---

## 3. 五类妥协（附代码位置）

> **阅读提示**：本节记录的是**审计当时**的差距快照。P0/P1 已在同一轮修完
> （§4.3 / §4.4），文中 `RectNodeOpts` / `TextNodeOpts` / `text_node` /
> `measure_fn` 均已从新体系删除；仅 `render_canvas2` 的 `make_container`
> 与 `SelectionManager` 仍待 P2 / P5。

### 3.1 基础组件绕开 `Node2`（最严重）

`nodes.rs:566` 的注释是自认的：

> 构造一个基础容器节点（**等价于 `node2` 的内部逻辑，但不走 `node2!` 宏，
> 以规避 `macro_export` 绝对路径 lint**）

受影响：`rect_node`（已修）、`wrapped_text_node`、`rich_text_node`、
`render_canvas2`。它们全部走 `make_container` / `text_node` → 直接造 `Node`。

TS 里这四者都只有一种写法：

```ts
export const rectNode = tfc((ctx, node, arg) =>
  node2.create({ ...rest, draw(ctx2, n) { .. } })(node)(ctx));
```

### 3.2 属性不是读取式

| TS | Rust |
|---|---|
| `background?: GetValue<ColorInt \| null>` | `background: Option<Color>` |
| `borderColor?: GetValue<...>` | `border_color: Option<Color>` |
| `borderWidth?: GetValue<number>` | `border_width: f32` |
| `radius?: GetValue<number>` | `radius: f32` |
| `text?: GetValue<string>` | `get_text: impl Fn() -> String` ✅ |

`RectNodeOpts`（`nodes.rs:514`）和 `TextNodeOpts`（`nodes.rs:71`）都是**静态值**，
主题切换 / 状态驱动的颜色无法响应。TS 全部是 getter。

> **状态**：§3 整节记录的是**审计当时**的差距；P0/P1 已修，见 §4.3 / §4.4。
> 上述两个类型已删除，改为 `RectNodeArg` / `RichTextNodeArg` / `WrappedTextNodeArg`。

### 3.3 尺寸机制不同

- TS：`Node2Arg.size(d) -> LayoutSize{value, fromInside}`
- Rust：`Node.measure_fn`（旧引擎槽位）

而 `to_node()` 曾经把 `size` 的 `from_inside` **丢掉**（只取 `.value`），
导致走 `Node2Arg.size` 的节点在带 padding 时外包宽少算两倍 padding。
这就是当初绕开 `Node2` 的直接原因。**本轮已修**（§4.1）。

### 3.4 过渡 `From` impl（自认临时）

`fc.rs` 里原有五处，注释写着"不是鼓励的写法——等复刻完就删"。现状只剩两处：

- `From<crate::Node> for Fc` —— 旧构造器（`make_container` 等）直接产 `Node`
- `From<Slot> for Fc` —— 旧组件的产物（`scroll` / `text_field` 等）

已删除的三条（`From<TFc> for Slot` / `NodeSink for Slot` / `NodeSink for crate::Node`）
随 `TFc` 一起消失：整套 `tfc` 三段式（`TFc` / `.of()` / `.create()` / `NodeSink` /
`FcSink`）已从代码里清空，`children` 只剩 `Fc` 一种元素类型。

### 3.5 模型级偏离

| 子系统 | TS | Rust |
|---|---|---|
| `SelectionManager` | 从渲染树**纯派生**：`pointerSelect` / `moveHitTest` 信号 → `rangeOf(node)` 按文档序分配，天然支持跨组件连续拖选 | **注册贡献者**：节点创建时登记 `get`/`clear`，`recompute()` 把各节点选中文本拼接。没有 `rangeOf`，跨组件连续拖选是"各自拖拽 + 最后聚合" |
| `Focus` | 信号存 `Node2` 引用；`EditableTextHandle` 定义编辑器句柄；`activeEditorSignal` 路由键盘 / IME / 剪贴板 | 信号存 `u64` id；无编辑器句柄、无活跃编辑器路由 |

### 3.6 Rust 独有扩展（TS 没有）

| 字段 | 位置 | TS 对应做法 |
|---|---|---|
| `clip` | `Node2Arg.clip` | `drawAtParent` 里自己 `clip()` |
| `tag` | `Node2Arg.tag` | 无 |
| `on_hover` | `Node2Arg.on_hover` | 由 `cursor` 派生，无独立回调 |
| `mouse_wheel` / `mouse_wheel_capture` | `Node2Arg` | `Renderer` 的 `globalMouse2.registerMouseWheel` |

这些"扩展"本身不算错，但它们**改变了节点契约**，让"1:1"不成立。
处理方式：保留字段但明确标注为扩展，或者在 P6 一起收敛。

### 3.7 IME 空串预编辑：跟 Kotlin，不跟 TS

| | 空串 `onComposing('')` 的语义 |
|---|---|
| TS `editable/editor.ts` | `commitComposition()`：清组合态但**保留**窗口文本，光标推到窗口尾 |
| Kotlin `EditableTextNode.onComposing` | `endComposition(false)`：**移除**窗口，光标回基座 |
| rust-mve | 同 **Kotlin**（`on_composing` 的 `t.is_empty()` 分支） |

理由：winit 在 Windows 上提交输入法结果是「先 `Ime::Preedit("", None)` 清窗、
再 `Ime::Commit(text)`」。走 TS 分支会把拼音串留在正文里（得到 `nihao你好`），
Kotlin 侧对此有明确约定：「空串上报（提交完成）仅移除窗口——已确认文本由平台
经常规插入路径另行写入」（`EditableComposingTest`）。

两条提交路径都验过：Windows 序列 = 清窗 → `Commit` 插入；Linux/macOS 不给空串
= `Commit` 直接替换窗口。回归见 `editable_text_node.rs::tests` 的
`composing_frames_replace_window_and_empty_preedit_removes_it` /
`winit_clear_then_commit_writes_final_text`。

---

## 4. 本轮已修（2 处）

### 4.1 `to_node()` 的 `size` 语义

`node2.rs` 的 `to_node`：

- **修 `from_inside`**：外包 = `value + padding_start + padding_end`（TS `toOuterSize`）
- **修优先级**：显式 `width`/`height` 覆盖 `size`（TS `if (!node.width)` 只在
  未声明 width 时才用 `size`）

这是 §3.1 的**根因**。修完之后基础组件才有资格走 `Node2` 路径。

### 4.2 `rect_node` 改成真正的 `node2` 组合

`nodes.rs::rect_node` 不再调 `make_container`，而是构造 `Node2Arg`
（`padding_*` / `layout: flex()` / `exts: [grow]` / `clip` / `draw` / `children`）
交给 `Node2::build`。

顺带修正一处绘制语义：TS 里背景矩形是**边框内缩 `bw/2`** 后填充
（`[bw/2, bw/2, w-bw, h-bw]`），旧 Rust 是铺满 `(0,0,w,h)`。
`canvas2_text` 快照因此有 1px 边缘差（1941 px，只在轮廓），图元数仍 44。

### 4.3 P0：参数类型对齐 TS（`extends Node2Arg` + 全 getter）

TS 的三个基础组件参数都是**接口继承 `Node2Arg`**、视觉属性全是 `GetValue<T>`。
Rust 没有接口继承，用 `#[rec(extends = node2)]` 注入 `rest: Node2Props` 承载
TS 的 `...rest`，组件体内再用 `node2! { ..rest, 覆盖项 }` 展开（`..rest` 是
`!` 宏的一条分支，语义就是对象展开）：

| TS | Rust |
|---|---|
| `RectNodeArg extends Node2Arg` | `RectNodeArg { rest: Node2Arg, .. }`（`rect.rs`） |
| `RichTextNodeArg extends Node2Arg` | `RichTextNodeArg { rest: Node2Arg, .. }`（`rich_text.rs`） |
| `WrappedTextNodeArg extends RichTextNodeArg` | `WrappedTextNodeArg { rest, + 透传字段 }`（`wrapped_text.rs`） |
| `background?: GetValue<ColorInt\|null>` | `background: Option<Get<Option<Color>>>` |
| `borderWidth?: GetValue<number>`（默认 1） | `border_width: Option<Get<f32>>`（`None` → `always(1.0)`） |
| `spans?: GetValue<RichTextSpan[]>` | `spans: Option<Get<Vec<RichTextSpan>>>` |
| `autoWidth / maxLines / ellipsis / textAlign` | 同名 `Option<Get<..>>` |

旧的 `RectNodeOpts` / `TextNodeOpts`（静态 `Color` / `f32`）**已删除**，不留别名。

同时按 TS 文件拆开，不再全堆在 `nodes.rs`：

```
canvas_render2/rect.rs         ← RectNode.ts
canvas_render2/rich_text.rs    ← RichTextNode.ts
canvas_render2/wrapped_text.rs ← WrappedTextNode.ts
canvas_render2/nodes.rs        ← ifNode / ForEachNode / renderCanvas
```

`tab_render_node` **已删除**：TS 的 `TabRender.ts` 只有 `expandTabs` 等纯函数，
没有"tab 渲染节点"；tab 展开本来就在 `RichTextNode` 内部做（现已对齐）。

### 4.4 P1：文本节点改走 `Node2Arg.size` + `draw` + `mouseClick`

旧的 `text_node()` 直接造 `Node { measure_fn, draw_fn, on_down_fn, .. }`——那是
旧引擎槽位，TS 里没有。现在逐句对照 TS `richTextNode`：

| TS | Rust |
|---|---|
| `node2.create({ ...rest, mouseClick, size, draw })(node)(ctx)` | `node.build(Node2Arg { size, draw, mouse_click, ..rest })` |
| `size(d)`：`autoWidth` 取段落宽；`y` 取段落高（空则 `maxFontSize*1.4`） | 同（`LayoutSize::inside`，`from_inside` 由 `to_node` 加 padding） |
| `size('x')` 且 `!autoWidth` → `defaultSize(node,'x')` | `min(段落宽, 已分配内容区宽)`（见下"一处机制映射"） |
| `draw`：先 `rest.draw?.(..)`，再选区高亮，再按 **`paddingInlineStart/BlockStart`** 偏移画正文 | 同（旧实现漏了 padding 偏移，正文贴在边框上，已修） |
| `mouseClick`：先 `rest.mouseClick?.(e)`，命中 link 再 `openUrl` | 同（旧实现把链接判断塞在 `mouse_down` 里，已移回 click） |
| `paragraph = memo(..)` | `ParaCache`（键：显示文本 + 样式 + 排版宽 + 最大行数 + 省略号 + 对齐） |

**`maxLines` / `ellipsis` 现在真的生效了**：此前 `build_paragraph` 的
`_max_lines` 是"预留参数"，白拿不干活。现在 `wy-text` 里按 TS
`buildParagraph(spans, width, maxLines, ellipsis, align)` 实现——先排完整段落，
超行则取「第 `maxLines` 行起点」为上界，二分出最长的可见前缀拼省略号重排
（`log2(字节数)` 次，结果被 `ParaCache` 持有）。

**一处机制映射（不是打折）**：TS 的 `size('x')` 在 `!autoWidth` 时返回
`defaultSize`，即 `layoutValue(node,'x').allowSizeFromChildren() ? sizeFromChildren()
: layoutValue(parent,'x').childSize(layoutIndex)`。Rust 引擎的 `layout_boxes`
把这一整套（含 `Layout::childSize`）在引擎内部一次算完，没有向声明层暴露
`childSize`；因此 Rust 取**等价结果** `min(段落宽, 已分配内容区宽)`——父给多宽就
换行到多宽，父还没给（首帧）时退回段落自然宽，与旧 `measure_fn` 的取值逐像素一致。

**`SelectionManager` 仍是 P5**：TS 从 `ctx.consume(selectionManagerContext)` 拿
管理器，Rust 的组件函数体拿不到 `ctx`（只有 `node_ref` / 参数），暂由
`RichTextNodeArg::selection_manager` 显式注入；无管理器时节点不装
`mouse_down/move/up`（= TS 里 `rangeOf` 恒 `null` 的效果）。

### 4.5 IME 组合态两处修复（2026-10-31）

1. **一打拼音就 panic（`RefCell already borrowed`）**：`on_composing` 曾在
   `if let Some(base) = self.composition_base.borrow().clone()` 的条件里取快照，
   edition 2021 下那个临时 `Ref` 活到整个 `if let`（含 `else` 分支）结束，
   `else` 里存 `composition_base` 的 `borrow_mut()` 撞上自己。改成先
   `let base_start = ...borrow()...;` 单条语句取快照，再分支。
   `composing_start` 同款写法一并拆开（它当时没炸只是因为 `cursor()` 不碰这格）。
2. **空串预编辑语义**按 §3.7 改跟 Kotlin（`end_composition(false)`）。

同类隐患顺手清了两处：`wy-engine/src/node2_app.rs` 的 `redraw` 回调
（`if let Some(f) = self.redraw.borrow().clone() { f() }`）——`Ref` 会被带进回调，
回调里一旦重设 redraw 就是同一个 panic；改成先 clone 出 `Rc` 再调。

### 4.6 文本垂直居中 + IME 候选框定位（2026-10-31）

1. **单行墨迹补偿补上了**（`text_metric::ink_shift_y_of`，对标 TS
   `canvasCore/TextMetric.ts::inkShiftYOf` / Kotlin `RichTextNode.inkShiftY`）。
   之前绘制一律从 `paddingBlockStart` 起笔，而 Parley 的行高留白堆在字形上方：
   `height: 34` 的输入框里字形只占最上面 12px（实测 gap 上 3 / 下 19），看着就是
   "文字贴顶"。现在 `editable_text_node` 与 `rich_text`（→ `wrapped_text`）的
   **绘制 / 选区 / 光标 / 命中** 共用一个文本原点 `padding + inkShift`，实测
   输入框变成上 11 / 下 11、按钮 12 / 11。
2. **IME 候选框定位补上了**：`Node2App` 之前没有覆写 `WyApp::ime_cursor_area`，
   落到默认实现返回 `None` → winit 收不到 `set_ime_cursor_area`，候选框不跟随光标。
   现在 `Node2::ime_cursor_rect()`（节点局部光标盒）+ `abs_x/abs_y` 合成全局矩形，
   等价于 TS `inputOverlay` / Kotlin `requestInputOverlay`。
3. 示例侧：shadcn 按钮把写死的 `draw_text(.., 10.0)` 先改成按 `measure_text`
   行高居中（`draw_text` 的 `point` 是**行盒原点**，不是基线）。

### 4.7 文本组件化 + `alignFix` 的第二个用途（2026-10-31）

1. **示例里不再有"手绘文本"**：`button` / `badge` / `tabs` / `avatar` /
   `stat_card`（标题 / 数值 / 变化）/ `sidebar` 导航项 / 「更多菜单」/ 下拉菜单项，
   文案统统换成 `text_label!` 子节点。理由不是审美，是**契约**：
   `Scene::draw_text` 不进布局，父容器拿不到这段字多高多宽，于是"居中 / 徽章宽度 /
   撑高"只能 `measure_text` + 手算坐标。换成文本节点后 `rich_text` 的
   `size(d)`（X=段落自然宽、Y=段落高）就是"文本撑起来的尺寸"，父布局直接吃。
   按钮因此回到 Kotlin `ButtonBase` 的结构：**容器 + 文本子节点**
   （`directionJustify: center` + `alignItem: center` + `alignFix = true`）。
2. **`alignFix` 的第二个用途**：除了"交叉轴尺寸取父给的"，它同时决定
   `StackLayout` 居中/靠尾的**基准**——`alignFix ? innerSize() : max(child.outerSize())`
   （TS `wy-helper/src/layout/stack.ts`、Kotlin `StackLayout.kt` 完全同款）。
   所以「自己声明了 `height` 的行要 `Center` 子节点」必须写 `align_fix: true`，
   否则基准退化成最高子节点：待办行（30 高、最高子节点 24、checkbox 18）会整体
   偏上 3px。三边都有这个坑，Kotlin 组件里 `override val alignFix get() = true`
   遍布就是这条约定。回归：`geometry::tests::stack_align_fix_decides_centering_reference`。

### 4.8 中文输入的三个门禁 + 光标闪烁驱动（2026-10-31）

现象：打拼音时字母与"选词空格"跑进正文；点输入框有时看不到光标。

1. **翻译层：`KeyEvent::text` 才是"实际产生的文本"**。`winit_translate` 之前只看
   `logical_key`（"这枚键代表什么字符"），被输入法消费的按键照样变成 `Key::Char`
   ——拼音字母、空格确认就是这么进正文的。现在字符键与空格必须有
   `KeyEvent::text`（IME 吃掉 `WM_CHAR` 时它是 `None`），带 Ctrl/Cmd/Alt 的组合键
   回退到逻辑键（保证 Ctrl+A/C/X/V/Z 不被误吞）。回归 4 条：
   `ime_composing_letter_produces_no_key` / `ime_space_confirm_is_not_text` /
   `accel_key_survives_missing_text` / `dead_key_multichar_text_is_ignored`。
2. **应用层：组合态 + 提交守护窗口**。`Node2App::handle_ime_event` 曾对任何
   `Preedit` 都置 `composing = true`，空串也不例外——而 winit 恰恰用
   `Preedit("", None)` 表示"组合结束 / 清空"，组合态于是一路卡住，键盘看起来
   "失灵"。现在非空才 true，空串与 `Disabled` 置 false。另外 Windows 的"空格选词"
   是两条消息：先 `Ime::Commit`，随后那次按键的 `WM_CHAR` 才作为 `Character(" ")`
   到达，此时组合态已结束——用 30ms 守护窗口丢掉紧随其后的字符键
   （`crate::ime_guard::ImeCommitGuard`）。
   这条与运行时不绑定：`Node2App` 与 v1 的 `MveApp` 共用同一个 `ImeCommitGuard`
   （`arm()` 在 `Ime::Commit`、`swallow()` 在字符键），v1 之前只修了翻译层，
   空格选词仍会多出一个空格。回归：`ime_guard::tests::*`、
   `mve_integration::tests::ime_commit_arms_guard_for_the_trailing_space_key`。
3. **光标闪烁要宿主驱动**。`cursor_visible` 按相位算，而绘制只发生在 `draw` 里：
   没人定时重绘，画面就停在某个相位上，正好是"灭"就无法解释地看不见光标。
   `Node2App` 现在覆写 `WyApp::next_wake` / `on_wake`，编辑器聚焦期间每
   `CARET_BLINK_MS`（530ms）唤醒一次（对齐 TS `repeatEvery`）；编辑器把相位锚点
   在"获得焦点 / 光标移动"时归零，点进去的第一帧必然是亮相。
   回归：`caret_blink_schedules_wakes_only_while_editing`、
   `caret_is_drawn_on_the_first_frame_after_focus`、`caret_phase_flips_every_period`。
   注：v1 的 `text_field` 光标是**常亮**（没有相位），所以这一条只对 v2 成立，
   v1 不需要补 `next_wake`。
4. **输入框的命中区 = 整块矩形**。命中测试只认节点自己的矩形：内边距留在外层
   容器时，靠边 10px 属于"容器空白"，命中链里没有可聚焦节点 → 焦点被清空。
   shadcn `input_field` 把 `padding_inline_*` 移到编辑器自身，点哪儿都能聚焦
   （详见 docs/07 第 6 条）。

### 4.9 逐子节点对齐（`AlignChild`）接线（2026-10-31）

三边比对里 Rust 最后一块缺口：`FlexCross::align(child)` 恒返回 `None`，子节点只能
跟着容器 `alignItem` 走，没法单独指定交叉轴对齐。TS 是
`cross.align(n) { const a = n.findExt(AlignChild); if (a) return direction()=='x' ? a.alignY() : a.alignX() }`，
Kotlin 是 `if (n is AlignChild) return when(direction) { x -> n.alignY; y -> n.alignX }`。
两处都特意注明了"这里必须 return，否则 when 只是语句、值被丢弃 → AlignChild 自对齐永远失效"。

接线分两层：

| 层 | 改动 |
|---|---|
| `wy-layout/src/stack.rs` | `Align::size` 由 `f32` 改为 `Option<f32>`；新增 `AlignItem::as_align()`（TS `alignSelf(item)`），四种取值各返回一个 `&'static` 无状态对象 |
| `wy-mve/.../tree/layout_direction.rs` | `FlexCross::align` 读 `find_ext_rc::<AlignChild>()`，按交叉轴取值 |

`Align::size` 改 `Option` 的理由：TS `alignSelf().size` 只在 `stretch` 时返回尺寸，
其余分支直接 `throw 'you should make your own size'`；Kotlin 的 `fun size(size: Float): Float`
没法表达"我不管尺寸"，只能由实现方硬凑一个数。`None` 让 `StackLayout` 把
`child_sizes[i]` 留空，`child_size()` 报"无自身尺寸"，与 TS 的"抛错"等价但可诊断。

用法（与 TS 同一套 ext 机制）：

```rust
node2! {
    exts: vec![align(AlignItem::End)],      // 两轴都设
    // 或 align_xy(Some(AlignItem::End), None) —— 只设 x
}
```

注意轴向：**横向容器读 `alignY`，纵向容器读 `alignX`**（交叉轴那一条）。只写
`align_xy(Some(..), None)` 的 ext 在 Row 容器里不生效，这是 TS 的原样行为。

回归：`wy_layout::stack::tests::fixed_align_item_matches_ts_align_self`、
`per_child_align_overrides_item_and_leaves_size_calc`、
`wy_mve::tree::geometry::tests::per_child_align_overrides_container_item`
（含"只给 alignX 不生效"的反例）。后者做过反向验证：把 `FlexCross::align` 改回
`None`，`End: 100 - 20` 的断言立即失败。

---

## 5. 剩余复刻顺序（按依赖，不按大小）
| 阶段 | 内容 | 前置 | 判据 |
|---|---|---|---|
| ~~**P0**~~ ✅ | `RectNodeOpts` / `TextNodeOpts` → `RectNodeArg` / `RichTextNodeArg` / `WrappedTextNodeArg`（`rest` + 全 getter） | — | 已完成（§4.3） |
| ~~**P1**~~ ✅ | `rich_text_node` / `wrapped_text_node` 走 `Node2Arg.size` + `draw` + `mouse_click` | P0 | 已完成（§4.4）；`measure_fn` 已从新体系消失 |
| ~~**P2**~~ ✅ | `render_canvas2` 对齐 `Renderer.ts` | P1 | 根走 `Node2`（删 `make_container`）+ 全局鼠标注册表 `drag_mouse`/`createClickCounter`；焦点/IME/键盘路由由引擎统一提供（§5.1） |
| **P3** | `ImageNode.ts` | P1 | 新文件 `image_node.rs` |
| **P4** | `EditableTextNode.ts`（983 行） | P1、P2、§3.5 的 Focus | `todo2` 去掉 `text_field` |
| **P5** | `SelectionManager` 重写为派生模型 | P4 | 跨组件连续拖选可用 |
| **P6** | 删 `make_container` + `fc.rs` 五处过渡 impl + Rust 独有扩展收敛 | P1-P3 | `fc.rs` 无 `From<Node>` / `From<Slot>` |

P0 + P1 已完成：`rect` / `rich_text` / `wrapped_text` 三个基础组件里不再有
"自建 `Node`"，也不再碰 `measure_fn`。`make_container` 现在只剩
`render_canvas2` 一个调用点（P2 一并解决）。

---

## 5.1 P2 已落地（2026-10-26）

`Renderer.ts` 在 TS 里**同时**是「声明根 + DOM 事件/重绘/IME 桥」；Rust 引擎
（`wy-engine/src/mve_integration.rs` 的 `MveApp`）已经拥有绘制循环、命中测试、
捕获+冒泡派发、焦点（`tab_navigate` / `click_focus_path`）、IME
（`handle_ime_event` / `ime_cursor_area`）与键盘路由（`handle_key_event`）。
因此 P2 按**架构边界**拆成两层，不把 TS 的事件循环原样抄进 `canvas_render2`
（那会重蹈"杂糅旧实现"的覆辙）：

- **声明层（`canvas_render2/`）**：
  - 删除 `make_container`，[`render_canvas2`] 改走 `Node2::build`（`nodes.rs`），
    与 TS `renderCanvas` 的 `node = {...rest, width, height}` + `commonNode` 同构；
    根背景由 `draw` 画在根上，子节点走 `children`。新体系**已无** `let mut n = Node`。
  - 新增 `canvas_render2/renderer.rs`：
    - [`GlobalMouse2`] 注册表（线程局部）：`down` / `move` / `up` / `wheel` 四套；
    - [`drag_mouse`]（对标 TS `dragMouse`）：按下接管全局 move，松手以最后坐标
      再回调一次并自动摘除；
    - [`create_click_counter`]（对标 TS `createClickCounter`）：同节点 400ms/5px
      内递增，三击选段、双击选词、单击复位（P5 选区会话消费）。
  - 引擎在事件循环里消费 `dispatch_global_mouse_down/move/up/wheel`
    （`mve_integration.rs` 的 `CursorMoved` / `MouseInput` / `MouseWheel`），默认
    空注册表即无操作，不改变任何既有行为。
- **引擎层（`mve_integration.rs`，已存在、未动）**：
  `focusableList` / `moveFocus` / `assignFocus` / `cursorOf` / IME 同步 / 键盘路由
  等价于 TS `Renderer.ts` 的同名闭包，只是遍历的是保留模式 `Node` 树而非
  `Node2` 树（`Node2` 字段经 `to_node` 落到 `Node`）。**不在 `canvas_render2`
  里重复实现**，避免两套焦点/IME 逻辑并存。

判据：P2 的"全局鼠标注册表"已可用（有单测锁行为）；`make_container` 已删除；
焦点/IME/键盘路由由引擎统一提供，canvas_render2 不再重复。

---

## 6. 判定规则（后续 review 用）

新增/修改 `canvas_render2/` 下的组件时，按这三条自检：

1. **是否 `node2.create` 组合？** 组件里出现 `let mut n = Node {..}` 就是偏离。
2. **属性是否 `Get<T>`？** 静态 `Color` / `f32` 字段在 TS 里都是 getter。
3. **尺寸是否走 `size(d)`？** 出现 `measure_fn` 就是走了旧槽位。
