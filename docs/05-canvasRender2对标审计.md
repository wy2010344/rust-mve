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

---

## 7. 本轮增量（2026-11-01）：浮层滚动条 · 裁剪顺序 · 帧唤醒

本轮以「整页移植 `apps/mve-vite-demo/src/pages/canvas-render2/`」为驱动，把对齐
从"逐个组件"推进到"整页跑起来"。结论按**谁的错改谁**记：

### 7.1 新增 `OverlayScrollBar.ts` → `overlay_scroll_bar.rs`（✅ 结构对齐）

| TS | Rust | 说明 |
|---|---|---|
| `ScrollBarStyle2` / `overlayScrollBarStyle` | `ScrollBarStyle2` / `ScrollBarStyle2::macos()` | 13 个字段逐一对应 |
| `ThumbGeo2` | `ThumbGeo` | 私有结构，四字段同名 |
| `inBand` / `geoOf` / `positionOf` | 同名局部函数 | 全部从容器的 `inner_size` / `content_size` / `max_scroll` 派生 |
| `alphaOf` + `fade/dragging/hovering` | 同形 | `dragging/hovering` 用 `Rc<Cell<bool>>`（事件驱动重绘，无需信号） |
| `acceptHit` / `mouseDown` / `draw` / `focusable` / `selectionEnabled` | 同名 props | — |

两处**不得不**结构差异（已在文件头注明）：

1. **动画**：TS 用 `requestAnimationFrame` 逐帧写信号；Rust 没有帧回调。落点是
   新增 `animation.rs`：`Fade` 把「浮现 → 保持 → 淡出」写成**读时按墙钟求值**的
   纯函数（跳过帧不错位、重复调用幂等），并用 `FrameTick` 向宿主登记"下一帧几点"；
   引擎 `Node2App::next_wake/on_wake` 把该截止时刻与光标闪烁取最小值、到点重绘。
   这不是新增一套动画框架，而是复用了光标闪烁**已有**的定时唤醒通道。
2. **注销**：TS 在 `ctx.addDestroy` 摘全局监听器；Rust 组件没有析构钩子。为此刻意
   给 `Node2` 加了 `downgrade() -> Node2Weak`：长生命周期的全局回调只捕获弱引用，
   节点消失后第一次被调用就地注销自己（否则注册表只增不减，`Fade` 还会一直排唤醒）。

### 7.2 `Scroll.ts` 补齐两处（Rust 侧漏的实现）

| TS | 本轮补齐 |
|---|---|
| `addActivityListener(listener): EmptyFun` | `Scroll2::add_activity_listener(Rc<dyn Fn()>) -> ActivityListener`（RAII，`Drop` 即退订） |
| `parentScroll` + `scroll()` 的剩余量转交 | `Scroll2::with_parent` / `parent_scroll`，`scroll()` 把 `delta - real` 交给外层 |

顺带钉住 TS 源码里那行注释的坑：`scroll()` 绕过了 `set_value` 的钳制计算，**活动
通知要自己补**——漏了它，滚轮滚动时浮层滚动条不会浮现（`activity_fires_on_scroll_and_on_set_value`）。

### 7.3 Rust 侧缺陷：`clip` 把节点自己裁掉了（已修）

`render_node2` 原先把 `push_clip(inner)` 放在**父循环**里包住 `child.render_node2(..)`，
于是带 `clip` 的容器连自己的圆角背景与边框一起被内盒裁掉。表现：`canvas2-todo`
里"卡片不见了、条目直接浮在页面上"。

这不是像素误读——三个独立的证据都指向同一处：

1. 第 400 行扫描：`x=50..57` 是页面底色（应为卡片白 + 边框）；
2. 列 `x=50` 全列都是页面底色（卡片左边框整段消失）；
3. 关掉 `clip: true` 后卡片立刻出现（尺寸/位置一直是对的，只有绘制被裁）。

正确语义是 TS 的 `drawAtParent`：**裁剪发生在子节点绘制前、容器自身绘制后**。
所以 `clip` 只夹后代。已加回归门 `clip_bounds_children_but_not_self`：断言图元顺序
是「卡片底色 → ClipPush → 子节点图元 → ClipPop」，且裁剪盒 = 内盒、入出栈配平。

### 7.4 新增示例 `crates/wy-app/examples/canvas2-todo/`

| TS | Rust |
|---|---|
| `todo-store.ts` | `store.rs`（信号 + 快照撤销/重做 + 双主题调色板 + `TextDefaults` 回写） |
| `todo-item.ts` | `item.rs`（Path2 复选框命中 / 链接片段 / 删除按钮） |
| `todo-list.ts` | `list.rs`（`create_scroll` + `scroll_content` + `for_each_node` + `overlay_scroll_bar` + 空态） |
| `todo-toolbar.ts` | `toolbar.rs`（`text_button` 胶囊 + 四块工具栏） |
| `view.ts` | `main.rs`（根就是那棵 flex 列 + 页面底色） |

暂未移植：`todo-editor.ts`（编辑器面板）、`todo-demos.ts`（`modifierHud` /
`nowrapDemo` / `nestedScrollDemo`）——前者依赖编辑器 + IME，后者依赖修饰键 HUD。

验证（`--snapshot 480x900`）：

- 卡片 276..616、内盒 284..608、行 58..422（= 380−2×8）→ **浮层滚动条确实不占布局宽度**；
- `--scroll 150`：滑块 y=375..561（长 187 = 316×324/546，偏移 87 = (316−187)×150/222），
  与手算一致 → 几何派生正确；
- `--select`：图元 233 → 255，标题/统计/每一行/每个按钮各自铺出高亮 → **跨组件选择在
  滚动列表里同样成立**（每行是独立 `richTextNode`，范围由管理器按文档序派生）。

---

## 8. 写法回归修正（2026-11-01）：把 builder 链改回声明式组件

本轮没有任何视觉改动，**输出与上一轮逐像素相同**（三张快照 diff = 0）。修的是
写法：上一轮移植 `canvas2-todo` / `canvas2-demo` 时，我在示例里写回了

```rust
RectNodeArg::builder().background(..).rest(Node2Props::builder().width(..).build()).build().build()
```

以及一个手搓的 `ButtonArg { label: Rc<dyn Fn() -> String>, .. }` 结构体 + `.mount(c)`
流水线。这正是本仓库已经淘汰的过程式写法（`#[rec]` 的 builder 是**宏的产物**，
不是给人手写的接口）。修正后：

| 位置 | 改前 | 改后 |
|---|---|---|
| 组件外壳 | `RectNodeArg::builder().rest(Node2Props::builder()..)` | `rect_node! { background: .., width: .., children: vec![..] }` |
| 纯容器 | `Node2Props::builder()..build()` | `node2! { .. }` |
| 文本 | `RichTextNodeArg::builder()..` | `rich_text_node! { spans: vec![..], .. }` |
| 图片 | `ImageNodeArg::builder()..` | `image_node! { image: .., radius: .. }` |
| 按钮 | `fn text_button(arg: ButtonArg) -> Fc` | `#[rec] fn text_button(#[getter] label, #[props(default)] primary, #[getter] active, #[getter] disabled, on_click)` → `text_button! { .. }` |
| 工具栏外壳 | `fn bar(children: Vec<Fc>)` | `#[rec] fn bar(children: Vec<Fc>)` → `bar! { children: vec![..] }` |
| 分支 | `if_node(..).mount(cx)` | `cx.child(if_node(..))` |
| **根** | `Node2Props::builder()..build()` + `build_root_node2` | `build_root_fc(node2! { .. })` |

### 8.1 新增 `build_root_fc`（补的是入口，不是抽象）

`build_root_node2(root_arg: Node2Props, page: impl FnOnce(&mut NodeContext))` 要求
调用方在**声明树之外**再写一份 props，于是每个示例的根都退化成一串
`Node2Props::builder()`。新增的 `build_root_fc(page: Fc)` 让根就是 `node2! { .. }`
本身（TS `renderCanvas({ .., children: [..] })` 的直译）。

实现绕了一层"探针父"：`Fc` 是 `FnOnce(&mut NodeContext)` 的黑盒，造出的节点只经
`Node2::mount` 挂到上下文携带的父上、**不返回句柄**。所以给它一个一次性父，
节点挂上来后立刻 `Node2::detach()` 断开——产物与 `build_root_node2` 同构
（`parent().is_none()`、已 `expand_node2`）。回归门
`node2::tests::build_root_fc_makes_a_parentless_expanded_root` 锁这三条：
无父、子节点已展开、能出图元。

### 8.2 两个使用陷阱（都是宏的既定语义，写错才踩）

1. **`$crate::XxxArg` 指 crate 根**：组件放在子模块（`toolbar::text_button`）时，
   `!` 宏里的 `$crate::TextButtonArg` 找不到该类型，报
   `cannot find TextButtonArg in $crate`。解法是根上再导出一次：
   `pub use toolbar::{BarArg, TextButtonArg};`（`finance/main.rs` 早有同样一行）。
2. **`#[getter]` 字段写的是"值表达式"，不是闭包**：宏自己会包 `move ||`，
   写 `spans: move || vec![..]` 就变成"返回闭包的闭包"（`expected Vec<..>, found
   closure`）。只有声明成 `impl Fn() -> T` 的字段（如 `wrapped_text_node` 的
   `spans`）才由调用方直接给闭包——同一个文件里两种写法并存，差别只在签名。

### 8.3 验证

| 检查 | 结果 |
|---|---|
| `canvas2-demo --snapshot 520x820` | 与改前逐像素相同（32 个图元） |
| `canvas2-todo --snapshot 480x900` | diff = 0（233 个图元） |
| `--scroll 150` | diff = 0（234 个图元） |
| `--select` | diff = 0（255 个图元，跨组件高亮照旧） |
| `cargo test --workspace --lib` | **523 passed** |
| `cargo test -p wy-app --example finance` | 68 passed |
| 两个示例的 clippy | 零告警 |

---

## 9. 写法修正（2026-11-01）：`wrapped_text_node` 继承 `rich_text_node`，不是 `node2`

同 §8，本轮**零视觉改动**（两张快照 diff = 0）。修的是继承层级选错了。

### 9.1 问题

TS 的声明写得很直白：

```ts
interface WrappedTextNodeArg extends Omit<RichTextNodeArg, 'spans'> { .. }
```

`RichTextNodeArg` 自己又 `extends Partial<Node2Arg>`。也就是**三层**：
`WrappedTextNodeArg` → `RichTextNodeArg` → `Node2Arg`。

Rust 侧当时写成 `#[rec(extends = node2)]`，等于跳过中间一层，然后把
`RichTextNodeArg` 的字段**手抄**了 6 个当"透传"（`selection_color` / `auto_width` /
`max_lines` / `ellipsis` / `text_align` / `selection_manager`）——抄的时候就漏了
`no_wrap`，`node_ref` 也没有入口（只能内部自建、调用方拿不到句柄）。凡"手抄一份
别人的字段"，漏是迟早的事。

### 9.2 修法（改的是宏，不是抄得更全一点）

1. `extends.rs` 从「只有 `node2` 一张表」扩成 `BaseSpec { props, fields, parent }`：
   一个基类可以自带「再上一层」的字段表。`rich_text_node` 登记为
   `props = RichTextNodeArg`、`fields = 它的 9 个自有字段`、`parent = NODE2_FIELDS`。
   顺带解决基类 props 名字不一定是 `<Pascal>Props` 的问题（`rich_text_node` 的
   props 叫 `RichTextNodeArg`）。
2. `rules.rs` 的 `!` 宏从两桶扩成**三桶**（own / base / node），并按基类给出终局
   表达式：
   - `node2`：`.rest(Node2Props::builder() $base .build())`
   - 二级：`.rest(RichTextNodeArg::builder() $base .rest(Node2Props::builder() $node .build()) .build())`
   终局分**两条**（`$node` 空 / 非空）：空时不能补一份 `Node2Props::default()`，
   否则 `..rest` 带进来的 node2 参数会被覆盖式 setter 整份冲掉。
3. `wrapped_text.rs` 变成 `#[rec(extends = rich_text_node)]`，函数体只剩一件事：
   `spans` 没给就补一段合成的。原来那串
   `RichTextNodeArg::builder().rest(rest)...build().build()`（§8 里唯一保留的 builder）
   随之消失——**库内最后一处手写 builder 也没有了**。

### 9.3 调用点的连带简化

`spans` 现在是继承来的 `#[getter]` 字段，调用处写**值表达式**（`spans: vec![..]` /
`spans: { .. }`），不再手写闭包。`canvas2-todo` 的 `item.rs` / `toolbar.rs` 各改了
一处（`spans: move || { .. }` → `spans: { .. }`），与其它 getter 字段终于同构。

### 9.4 回归门（`wrapped_text::tests`，5 条）

| 测试 | 锁什么 |
|---|---|
| `inherits_rich_text_params_not_just_node2` | `no_wrap` 等**只存在于 RichTextNodeArg** 的 key 能被写出并生效（同文本单行 vs 折行高度差） |
| `inherited_node_ref_points_at_the_tree_node` | `node_ref` 走同一条继承通道，且拿到的是树上那个真节点 |
| `synthesizes_one_span_when_spans_absent` | 没写 `spans` → 由 text 合成一段（TS `?? [singleSpan()]`） |
| `explicit_spans_wins_over_text` | 写了 `spans` 就以它为准（`text` 被忽略） |
| `rest_of_wrapped_text_seeds_the_base_args` | `..rest`（`RichTextNodeArg`）展开成基类 builder 的 `from_props`，且与祖父字段并存 |

验证：`cargo test --workspace --lib` **528 passed**（+5）；finance 68 passed；
`canvas2-todo` 233 图元 / `canvas2-demo` 32 图元，两张快照与改前 **diff = 0**。

### 9.5 顺带核对：`editable_text_node` 不改

TS `EditableTextNodeArg extends Partial<Node2Arg>` —— 它是**直接**挂在 `node2` 上的，
中间没有富文本那一层。所以 Rust 侧继续 `extends = node2` 是对的（这一条是查过
源码才不改的，不是漏改）。

---

## 10. `omit(..)`：把 TS 的 `extends Omit<Base, 'k'>` 补上（2026-11-01）

§9 只做到「继承到最近那一层」，但 TS 的声明还有后半句：

```ts
interface WrappedTextNodeArg extends Omit<RichTextNodeArg, 'spans'> { .. }
```

`Omit` 不是装饰：`spans` 被从基类里**摘出去**重新声明（改为可空），再在函数体里
`{ ...rest, spans: () => spans?.() ?? [singleSpan()] }` 落地。缺了它，「继承」
就变成「整份照抄」，与源码一一对不上。

### 10.1 语法与产物

```rust
#[rec(extends = rich_text_node, omit(spans))]
pub fn wrapped_text_node(.., #[getter] spans: Vec<RichTextSpan>) -> Fc
```

| TS | Rust |
|---|---|
| `extends Omit<RichTextNodeArg, 'spans'>` | `extends = rich_text_node, omit(spans)` |
| `spans?: GetValue<RichTextSpan[] \| undefined>` | `#[getter] spans: Vec<RichTextSpan>`（三态之「无默认的 getter」→ `Option<Get<..>>`，与基类同一类型） |
| `const { .., spans, ...rest } = arg` | 宏不再把 `spans` 转发进 `RichTextNodeArg::builder()`，它只落在自有字段上 |
| `{ ...rest, spans: () => spans?.() ?? [singleSpan()] }` | `rest.spans = Some(spans.unwrap_or_else(合成一段))` |
| `Omit` 后可省的名字必须存在（类型系统保证） | 编译期校验：拼错直接报错并列出可省略的字段名 |

实现落在三处，都不涉及运行期：

1. `attr.rs`：解析 `omit(a, b)`（组件级唯一的分组写法，因为 `Omit` 本身就是点名）；
   空括号、无 `extends` 而写 `omit`、写成 `omit = ..` 都是编译错误。
2. `extends.rs`：`base_arms_for(base, omit)` 跳过被点名的字段——**两层一起过滤**
   （基类自有 + 再上一层 `node2`），所以 `omit(children)` 对 `extends = node2` 同样有效。
   `validate_omit` 负责「名字必须是基类真有的」。
3. `rules.rs` / `mod.rs`：把 `omit` 透传进模板。自有字段分支本来就排在基类分支之前，
   `omit` 之后不会再有同名基类分支与它并存（此前即使并存也是静默双份，靠顺序取胜）。

### 10.2 TS 侧另一处 `Omit`：不需要跟

`canvasRender2/Renderer.ts` 里还有一处：

```ts
export interface RendererArgs extends Omit<Partial<Node2Arg>, 'x' | 'y' | 'width' | 'height'> {
  canvas: HTMLCanvasElement;
}
```

它是**宿主入口**（画布尺寸由 viewport 决定，所以把这四个位置/尺寸项省掉），对应
Rust 的 `Node2App::new(..).with_size(..)`——不是 `#[rec]` 组件，没有 `!` 宏与
`rest`，因此不涉及本次的 `omit`。语法上多条也是支持的（`omit(a, b, c)`），
`extends = node2` 时甚至可以省掉 `node2` 层的字段（如 `omit(children)`）。

### 10.3 一处**必须说明**的残余差异

TS 的 `Omit` 是**类型级**的：`rest` 的类型里根本没有 `spans` 这一格。
Rust 的 `rest: RichTextNodeArg` 仍是完整类型，`rest.spans` 字段存在、入场恒为 `None`。
也就是说，「省掉的字段不能从 `..rest` 进来」这件事，在这里由 **`!` 宏的表面**保证，
而不是由类型保证。要做到类型级，得为每个 `omit` 生成一份新的 props 结构体
（`Omit<Base, k>` 的展开），代价是基类 props 的类型名不再可预测、且 `rest.build()`
要跨类型重组——收益只在校验强度上，暂时不做，如实记在这里。

### 10.4 回归门

| 层 | 测试 | 锁什么 |
|---|---|---|
| 宏 | `extends::tests::omit_drops_the_base_arm` | 被 omit 的字段不再生成转发分支，其它字段照旧 |
| 宏 | `extends::tests::omit_reaches_the_grandparent_layer` | `omit` 覆盖 `node2` 那一层 |
| 宏 | `extends::tests::validate_omit_rejects_unknown_names` | 拼错当场报错并列出候选 |
| 宏 | `extends::tests::validate_omit_accepts_known_names` | 合法名（含跨层）不误报 |
| 组件 | `wrapped_text::tests::explicit_spans_wins_over_text` | 自有 `spans` 生效（不是被合成段冲掉） |
| 组件 | `wrapped_text::tests::own_spans_wins_over_rest_spans` | 自有 > `..rest`（TS `{...rest, spans}` 顺序） |
| 组件 | `wrapped_text::tests::rest_spans_is_ignored_under_omit` | `Omit` 后 `rest.spans` 这条车道是死的 |
| 组件 | `wrapped_text::tests::rest_of_wrapped_text_seeds_the_base_args` | 省一个字段 ≠ 关掉整条通道（`node_ref` 仍能从 `..rest` 进来） |

验证：`cargo test --workspace --lib` **534 passed**；`cargo test -p wy-mve-macros` 4 passed；
finance 68 passed；`canvas2-todo` 233 图元 / `canvas2-demo` 32 图元，两张快照 **diff = 0**
（`omit` 是参数口径的整理，不改布局）。

## 11. `todo-demos.ts` 移植（2026-11-01）：三块演示 + 两个真实缺陷

`pages/canvas-render2/todo-demos.ts` 那块「新特性演示」落在
`crates/wy-app/examples/canvas2-todo/demos.rs`：

| TS | Rust |
|---|---|
| `modifierHud()`：`ctrl/shift/alt/meta` + `pressedSignal` | `modifier_hud()`：读 `modifiers()` / `pressed_node()` |
| `registerKeyPress` / `notifyKeyPress` | `register_key_press` + 线程局部 `LAST_KEY`（按 `Rc` 引用同一性去重，句柄必须留住） |
| `nowrapDemo()`：`noWrap` + `grow` | `nowrap_demo()` |
| `nestedScrollDemo()`：`createScroll(c, 'y', undefined, parentScroll)` | `nested_scroll_demo()`：`Scroll2::with_parent` |

移植过程里撞出两个**实现缺陷**，都不是"少抄一点"能解释的：

### 11.1 缺陷一：`ScrollContent.drawAtParent` 没有对应实现

TS 的裁剪不在容器上，而在**滚动内容节点**上（`assemble.ts`：
`save → drawAtParent(剪) → translate → draw → restore`），用的是**父**的内盒、
发生在 `translate` 之前。Rust 早先把裁剪全压在容器的 `clip` 上，
于是「内容节点要求裁到父框」这件事无处表达，嵌套滚动里内容直接溢出卡片。

修法：`Node2Props::clip_to_parent`（对应 TS `drawAtParent` 的这条用法），
在父的绘制循环里于 `push_transform` **之前**压栈、用父的 padding inner 盒，
绘制完再弹。容器不需要写 `clip`——`todo-list.ts` 里也没有。

顺带补齐同一函数的 `acceptClip`：命中门控以**父（容器）inner 区**判定，
`hitest` 传进来的坐标本来就相对父，正好同义。

### 11.2 缺陷二：行节点写了 `layout` 导致宽度塌回文本宽

嵌套滚动里"内层条目"宽度只有 78px（文本宽），而不是父分配的 336px。
根因是 Rust 侧给行节点写了 `layout: flex!{}`，而 TS 里这些矩形**不写 layout**
（= 绝对定位）→ `defaultSize` 走 `sizeFromParent` 拿父布局分配的子尺寸。
有 layout 的节点 `allows_size_from_children()` 为真 → 走 `sizeFromChildren`
→ 塌到内容宽。去掉 layout 即恢复。

结论：**「宽度应该跟着父走」的行不要挂空 flex**；`layout` 的有无就是
TS 里"容器 / 被摆放的子项"这条分界。

### 11.3 验证

- `--nested 100` → 内层真实滚动 100 / 内层 100 / 外层 0（内层够滚，剩余量为 0）
- `--nested 300` → 内层 224（= 524 − 300）/ 外层 76（= 300 − 224）——剩余量确实
  经 `parentScroll` 交给了外层，与手算一致。
- 快照 480×1240 共 389 图元；`clip_to_parent_clips_this_branch_to_parent_inner`
  锁定裁剪盒 = 父内盒 `(10, 10, 180, 80)` 与入出栈配平。

## 12. `todo-editor.ts` 移植（2026-11-01）：编辑器面板

`pages/canvas-render2/` 最后一块没搬的组件。逐行对照后没有新的引擎能力要补——
面板本身就是 `rect_node` + `editable_text_node` 的组合，落在
`crates/wy-app/examples/canvas2-todo/editor.rs`。

| TS | Rust |
|---|---|
| `draftText` / `noteText` / `secretText` | 三个线程局部 `Signal<String>`（`DRAFT` / `NOTE` / `SECRET`） |
| `fieldLabel(text)` | [`field_label`]（宽度走 `FIELD_WIDTH`） |
| `interface FieldArg` | `struct FieldArg` + `FieldArg::single(..)` / `::multi(..)` |
| `textField(arg)` | `text_field(arg)` |
| `editorPanel()` | `editor_panel()` |

### 12.1 TS 侧一处自相矛盾（两侧一起修）

备注框的占位文案写的是 **「多行备注：Enter 换行，Ctrl+Enter 加入列表」**，但
`textField` 调用里**没传 `onSubmit`**——多行里按 Ctrl+Enter 什么都不会发生。
按「双向检测，错的改对面」的口径，TS 与 Rust 一起补上：

```ts
// apps/mve-vite-demo/src/pages/canvas-render2/todo-editor.ts
onSubmit() {
  const v = noteText.get().trim();
  if (!v) return;
  addTodo(v);
  noteText.set('');
},
```

提交语义与单行新增框完全一致（`trim` 后非空才加，加完清空）。`tsc --noEmit` 通过。

### 12.2 内层显式尺寸不是"没用好布局"

TS 给内层编辑器写的是 `width: 342`（= 356 − 6×2 − 1×2）与 `height: height - 14`。
这不是手算偷懒：`EditableTextController::size` 在没有显式宽高时**按段落量出来的宽高
报数**（`LayoutSize::inside`），并不向父内容盒拉伸。空文本时量出来是 0，
所以"省掉它、交给 `sizeFromParent`"会直接得到 0 宽输入框。两侧都是这个语义，
因此照抄，并在函数文档里写明理由（`editor_fields_keep_their_width_and_focus_order`
这条测试就是这条结论的门）。

### 12.3 面板高度来自子节点

`rectNode` 不写 `height`：竖向 flex、四个子节点都没有 `grow` →
`allows_size_from_children()` 为真 → 高度由子节点撑起。Rust 走的是同一条
`default_size` 路径，`editor_panel_stacks_title_and_three_fields` 锁住
「高度 = padding×2 + 子节点之和 + gap×3」。

### 12.4 回归门

| 层 | 测试 | 锁什么 |
|---|---|---|
| 组件 | `editable_text_node::tests::enter_submits_single_line_and_ctrl_enter_submits_multi_line` | 单行回车提交不换行；多行裸回车换行、Ctrl+Enter 提交 |
| 示例 | `editor::tests::editor_panel_stacks_title_and_three_fields` | 标题 + 三字段、gap 8、字段高 `lines×20+12`、面板高由子节点撑起 |
| 示例 | `editor::tests::editor_fields_keep_their_width_and_focus_order` | 内层宽 342（**空文本也不塌**）、`focus_order` 1/2/3 |
| 示例 | `editor::tests::all_three_editors_are_focusable` | 三个字段（含掩码）都能点进去 |

验证：`cargo test --workspace --lib` **536 passed**（新增 1 条编辑器测试）；
`cargo test -p wy-app --example canvas2-todo` 3 passed；finance 68 passed；
`tsc --noEmit` 通过。快照 480×1240 共 427 图元，`docs/shot-canvas2-todo{,-scroll,-select,-editor}.png`。

---

## 13. 交互缺陷修复（2026-11-01 晚）：按键双发与 resize 不重排

用户实测报的两条，都不在组件层，而在**引擎/框架层**，因此单侧对读源码看不出来。

### 13.1 一次按键走了两格：`Pressed` + `Released` 都派发

**现象**：编辑器里按一次方向键，光标跳两格；退格删两个字符；回车提交两次。
正文字符不明显——因为字符走 IME `Commit`，不在这条路上。

**根因**：winit 对同一枚键发两条 `KeyboardInput`（`Pressed`、`Released`），
`runner.rs` 的 `WindowEvent::KeyboardInput` 分支把两条都翻译并转给
`app.handle_key_event`。`KeyEvent` 虽然有 `pressed` 字段，但只有 v1 的
`text_input` 在 `Char` 分支里用过，方向键/退格分支根本没看。

**修法**（`crates/wy-engine/src/runner.rs`）：只在 `Pressed` 派发，
并在 `WyApp::handle_key_event` 的 rustdoc 里写死契约「一次物理按键只调一次，
实现方不要再检查按下状态」。松开事件不产生按键语义。

**判定口径**：新增任何事件派发入口前，先数一数同一个物理动作会被调用几次。
详见 `docs/08-实现原则.md` §6。

### 13.2 改窗口大小布局不跟随，点一下才变

**现象**：拖动窗口边缘放大，内容停在旧布局；随便点一下（触发 hover/selection
之类的信号）才重排。

**根因**：绘制缓存 `RecordMemo<Scene>` 的失效条件**只有信号**。
`resize → on_resize → viewport.set → request_redraw` 这条链路上没有任何信号变化，
于是 `record()` 判定"依赖没变"，直接复用按旧 `(w,h)` 录的 Scene；
`Node2App::draw`（含 `clear_layout_memo()`、`viewport.set`）根本没被调用。
点击会产生信号变化 → 追踪器重录 → 布局跟上。这与"点一下才改变"完全吻合。

**修法**

| 层 | 改动 |
|---|---|
| `wy-signal` | `RecordMemo::invalidate()` + `dirty` 标志：一次性置脏，下次 `record()` 必重录 |
| `wy-engine/redraw_tracker` | `RedrawTracker::invalidate()` 透传 |
| `wy-engine/runner` | `Resized` 与 `ScaleFactorChanged` 分支在 `request_redraw` 前置脏 |

**为什么不做成"自动感知"**：`w/h/scale` 不是信号，追踪器没有合法途径看见它们。
把帧环境也做成信号会污染整个响应式模型；显式 `invalidate` 是这类情况的正确形状。

**判定口径**：引入缓存前先列全部输入维度（信号/帧环境/字体/locale/主题），
信号之外的必须留失效口子。详见 `docs/08-实现原则.md` §7。

### 13.3 回归门

| 层 | 测试 | 锁什么 |
|---|---|---|
| `wy-signal` | `record_memo::tests::reuses_the_cache_when_nothing_changed` | 无变化不重录 |
| `wy-signal` | `record_memo::tests::invalidate_forces_exactly_one_rerun` | 置脏必重录、且只重录一次 |
| `wy-signal` | `record_memo::tests::invalidate_still_reports_change_for_following_signal_updates` | 重复置脏每次生效（依赖边没丢） |
| `wy-engine` | `redraw_tracker::tests::cache_is_reused_until_invalidated` | 同上，在 Scene 缓存的真实形态上 |

验证：`cargo test --workspace --lib` **540 passed**（+4）；示例快照 480×1240
修复前后 **diff = 0**（427 图元），确认这两处改动不动视觉。

### 13.4 原则沉淀

新增 `docs/08-实现原则.md`（10 条 + AI 失效模式自查表 + 动手前五问），
`AGENTS.md` 指向它并修正了过时的"本环境无 toolchain"一节。
---

## 14. 写法清理与空行缺陷（2026-11-01 深夜）

### 14.1 手写 builder 清零：`Props::builder()` 只是宏的机械通道

**质疑**：`Node2Props::builder()` 还有残留，主要在测试里。

**核查结论**：产品代码早就是零，但测试与 doc 示例里还有 10 处手写 builder
（`node2_app.rs` 1 处 doc + 1 处测试、`image_node.rs` 3、`rich_text.rs` 2、
`wrapped_text.rs` 3）。`node2!` 宏展开**本身就是** `Node2Props::builder() ...
.build().build()`——所以 builder 不能删，它是 `!` 宏的机械通道；但**手写**它就是
过程式装配，而且测试/文档的示范价值不分生产与测试。

| 处理 | 位置 |
|---|---|
| doc 示例改 `build_root_fc(node2! { .. })` | `wy-engine/src/node2_app.rs` |
| 测试改 `build_root_fc(rect_node! { children: vec![rich_text_node! {..}] })` | `node2_app.rs`、`rich_text.rs` |
| 测试改 `image_node! { .. }` | `image_node.rs` |
| `..rest` 载荷改结构体字面量（宏产节点、不产 props） | `wrapped_text.rs` |
| `builder()` / `build()` 加 `#[doc(hidden)]` + 「不要手写」文档 | `wy-mve-macros/src/rec/codegen.rs` |

自检：`grep -rn "::builder()" crates/` 现在只命中注释。

### 14.2 空行上上下键：行号不能拿光标矩形去反推

**现象**：文本里有空行时，上键跳不过空行（原地不动），下键从空行直接跳到文末。

**排版层是好的**（已实测 `a\n\nb` → 3 条 `LineMetric`，空行有行盒、区间 `[2,3)`）。
坏在两处都在"用几何反查"：

1. `caret_line_index` 拿光标矩形竖直中心去碰行盒。空行没有字形簇，
   `rects_for_range` 返回空 → `caret_rect` 回退到"前一格" → 光标矩形落在
   **上一行末尾** → 行号少算一行 → 上键 `j = i - 1` 撞到 `-1` 直接 return。
2. `move_to_line` 用 `glyph_position_at_coordinate(px, y)` 全段落找簇。
   空行上 `Cluster::from_point` 返回 `None`，兜底分支是"`dy >= 0` → 文末"，
   于是"下键从空行直接跳到最后一行"。

**修法**（把"行"当索引而不是几何）：

| 层 | 新增 / 改动 | 作用 |
|---|---|---|
| `wy-text` | `TextParagraph::line_index_of_byte` | 按字节偏移查行；空行有归属（`start <= byte < end`），文末归最后一行 |
| `wy-text` | `TextParagraph::position_in_line` | 行内按 x 取偏移，**行范围当硬边界**；空行短路返回行首 |
| `wy-text` | `TextParagraph::is_blank_line` | 行内只有 `\r`/`\n`（或空区间）即空行 |
| `editable_text_node` | `caret_line_index()` 改按偏移；`move_to_line` 用 `position_in_line` | 上下键 / PageUp/Down 全部受益 |
| `editable_text_node` | `caret_rect` 空行返回行盒零宽矩形 | 光标画在空行行首，而不是上一行末尾（IME 候选窗也跟着对） |

### 14.3 回归门

| 层 | 测试 | 锁什么 |
|---|---|---|
| `wy-text` | `blank_lines_are_real_lines` | 空行有独立行盒与字节区间 |
| `wy-text` | `line_index_of_byte_lands_on_blank_lines` | 空行不被跳过、文末归最后一行 |
| `wy-text` | `position_in_line_never_leaves_the_line` | 空行只返回行首，绝不兜底到文末 |
| `wy-text` | `blank_line_detection_covers_trailing_newline` | `"a\n"` 的末尾空行 |
| `wy-mve` | `vertical_moves_stop_on_blank_lines` | `a\n\nb` 上/下逐行推进，停在空行 |
| `wy-mve` | `trailing_blank_line_is_reachable_and_escapable` | 末尾空行可停留、可离开 |

验证：`cargo test --workspace --lib` **546 passed**；`--example canvas2-todo` 3 passed；
`wy-mve-macros` 4 passed；`--fill "note=第一行\n\n第三行"` 出图确认空行占位正常
（427 图元，字体排版与行距不变）。

## 15. 编辑内核统一：两套变一套（2026-11-01 收尾）

### 15.1 起点：2503 行零消费的 `EditCore`

`wy-text` 里躺着 `edit_core.rs`(1687) + `editable.rs`(341) + `editing.rs`(475)
= 2503 行、74 条测试，是 Kotlin `EditableTextNode` 纯逻辑层的逐条复刻，全绿，
**但在 `wy-text` 之外一个调用点都没有**。
`canvas_render2::editable_text_node` 自己在 2368 行里又写了一份：
`caret_pair` / `composing_text` / `composing_cursor_pos` / `composition_base` /
`undo_redo` / `preferred_x` 六个字段各管一摊，`undo_redo.rs` 单独 297 行。

两套实现 = 两份真相。空行、掩码、IME 组合态这类行为一旦只改一边，症状是
"某个入口对、另一个入口不对"，最难查。本轮把组件层**统一到 `EditCore`**上。

### 15.2 职责切分（谁管什么，边界在哪）

| 层 | 归 `EditCore`（`wy-text`） | 归组件（`canvas_render2`） |
|---|---|---|
| 文本 | 正文、`replace_text` | 对外 `Signal<String>` 的双向同步 |
| 光标 / 选区 | 字符索引、锚点、扩展、词/字素边界 | 全局选区协调（`SelectionManager2`） |
| 撤销重做 | `undo` / `redo` + 历史栈 | 快捷键路由 |
| IME | 组合串、组合态、掩码映射、`commit_composition` | 候选窗定位（要行几何） |
| 段落 | — | `ParagraphCache` 与失效 |
| 几何 | — | 光标矩形、垂直移动 `preferred_x` 消费方、命中、绘制、闪烁 |

判据只有一条：**纯逻辑层给不出答案的才留在组件**。垂直移动是典型例子——
`move_to_line` 要行盒，`EditCore` 没有排版，所以行号解析留在组件，`EditCore`
只负责"给我第几行第几列"。

### 15.3 三个适配壳：`sync_params` / `edit` / `with_core`

组件层不再直接摸字段，只剩三条通路：

```rust
fn sync_params(&self);                        // 外部 Signal → 内核（text/placeholder/obscure/single_line）
fn edit(&self, f: impl FnOnce(&mut EditCore)) -> bool;  // 可能改文本的动作；改了就写回
fn with_core(&self, f: impl FnOnce(&mut EditCore));     // 只动光标/选区
```

`edit` 用"前后文本比对"决定要不要 `write_text`——内核的每个增删都自带这个
判断太啰嗦，而 `Signal::set` 同值写会白白唤醒一次重绘。

### 15.4 借用纪律：`Signal::set` 是同步 flush，`BatchGuard` 是必需品

这是本轮踩得最深的一处。`Signal::set` **同步**通知观察者，观察者
（`SelectionManager2` 派生的选区 memo）会回头 `core.borrow()`。而
`edit()` 里 `borrow_mut()` 还活着 → `RefCell already mutably borrowed`，
`wy-engine` 三条 IME 测试直接崩。

第一反应是"改成 flush 后再借"，但那要求把借用切碎、把中间状态外露。正解是把
**flush 推迟到借用释放之后**，靠析构顺序而不是手工调用：

```rust
let _batch = begin_batch();        // ① 先声明
let mut core = cell.borrow_mut();  // ② 后声明
// ← ② 先析构：借用释放
// ← ① 后析构：此刻 flush，观察者读到的已经不是借用中的状态
```

声明顺序反过来写就失效（守卫先析构 → 借用还在时就 flush）。`wy-signal` 因此
新增 `BatchGuard` / `begin_batch()`：与 `batch(|| ..)` 的区别是**不包闭包**，
能给"借用 + 批次"规定析构次序。

### 15.5 顺带删掉的三个旧模块

| 删除 | 行数 | 理由 |
|---|---|---|
| `canvas_render2/undo_redo.rs` | 297 | 内核已有 `undo`/`redo`，这份是第二实现 |
| `canvas_render2/tab.rs` 的 `logic_to_display` / `display_to_logic` | 净 −25 | 用 `text.len()`（字节）当字符索引上界，多字节文本必 panic；且无调用点 |
| `canvas_render2/graphemes.rs` 的 `Graphemes` | 净 −55 | 与内核字素边界重复；`Words`（双击选词）保留 |

`tab.rs` 只留 `expand_tabs` / `SPACES_PER_TAB`（排版前把 `\t` 换成空格，纯字体层）。

### 15.6 回归门

| 层 | 测试 | 锁什么 |
|---|---|---|
| `wy-text` | `commit_composition_keeps_window_text` | 组合提交**保留**已上屏文本（不同于 `end_composition(false)`） |
| `wy-text` | `commit_composition_without_window_is_harmless` | 无组合态时调用是空操作 |
| `wy-engine` | `empty_preedit_ends_composing` 等 8 条 | 借用纪律：`Signal::set` 同步 flush 不再撞借用 |

验证：`cargo test --workspace --lib` **543 passed**；`--example canvas2-todo`
3 passed（427 图元）；快照 480×1240 与统一前 **像素级 `bbox = None`**；
`cargo clippy --workspace --all-targets` 与 `RUSTDOCFLAGS=-D warnings cargo doc`
均无代码告警。
