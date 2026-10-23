# Node 层复刻偏差与补齐方案

> 状态：**已全部实施**（7/7）。逐条对照 `wy-helper`（Kotlin 上游，
> `skia-engine/.../LayoutNode.kt` + `layout/.../FlexLayout.kt` + `StackLayout.kt`）
> 与 rust-mve（`wy-mve/src/node.rs` + `wy-layout/src/{flex,stack}.rs`）得出。

## 实施进度

| 项 | 状态 | 提交 |
| --- | --- | --- |
| P0-1 gap 默认 8.0 → 0.0 | ✅ 已实施 | 见下方「已实施项说明」 |
| P0-2 align_fix 开口 | ✅ 已实施 | `c594d45` |
| P0-3 padding 四向（pad_x/pad_y） | ✅ 已实施 | `102caa9` + `e75ecfc` + `283bd50` |
| P1-1 from_inside | ✅ 已实施 | `e0b5a75` |
| P1-3 双击/右键 | ✅ 已实施 | `edfc077` |
| P1-2 捕获相位 | ✅ 已实施 | `bd3a9a8` |
| P1-4 显式定位判据 | ✅ 已实施 | `3284ce4` |

### 各项目要

- **P0-2 align_fix**：`Node.align_fix`（默认 false）+ rsx 属性；`natural_size_of`
  在 align_fix 时交叉轴不取子节点 max（`allow_size_from_children=false` 语义）。
  注意 wy-mve 的交叉轴分配从未走 wy-layout `StackLayout`（node.rs 手写四分支），
  故落点在自然尺寸而非适配层透传。
- **P0-3 padding 四向**：`pad_x`/`pad_y: Option<f32>` 覆写 `padding` 简写；
  两个收敛点（`natural_size_of` / `layout_boxes` 内区与偏移）按方向取值。
  finance 标题栏拆掉「padding:12 四向同值把 20 高栏内区压成 0、文字靠负偏移
  碰巧落位」的手算补偿。
- **P1-1 from_inside**：`effective_width/height` 统一换算（布局链路全走这两个
  入口）；与 pad_x/pad_y 叠加按方向换算。
- **P1-3 双击/右键**：引擎 `DoubleClickDetector`（400ms/5px，对齐 Kotlin 常量）；
  双击不吞单击、构成即消费候选（三连击只触发一次）；右键只派发
  ContextMenu 不派发 Down/Click。`PointerEvent` 加 `button` 位。
- **P1-2 捕获相位**：6 个 dispatch 函数双相位（捕获根→叶 + 冒泡叶→根）；
  `dispatch_click_chain` 展开为 down_capture → click_capture → down → click
  四轮；悬停移动保持单相（Kotlin 无 HoverCapture 对应物）。Capture 变体
  全集与 Kotlin 一一对应（Click/Down/Up/Move/Wheel/DoubleClick/ContextMenu）。
- **P1-4 显式定位**：判据从「x/y 非 0」改为「x_fn/y_fn 存在」；四个 `*_at`
  简写改设常量闭包（它们此前完全靠旧判据挪出 flex 流）；审计确认示例层
  无 rsx 静态 x/y 定位、overlay/scroll 本就走 fn 路径。

### 验证基线（每项同一套门禁）

- wy-mve 173 passed / wy-engine 96 passed / finance 61 passed / shadcn 14 passed
- `cargo clippy --workspace --lib --all-features -- -D warnings` 干净
- `cargo fmt --all -- --check` 干净
- **行为/默认值变更项（P0-1 / P1-4）额外**：双示例离屏快照逐像素 diff 一致
  （finance 1170 图元 / shadcn 309 图元）——对既有视觉零影响

## 结论先行

**核心布局语义复刻正确**，没有跑偏：

| 语义 | Kotlin | rust-mve | 结论 |
| --- | --- | --- | --- |
| 交叉轴默认对齐 | `AlignItem.center` | `AlignItem::Center` | 一致 |
| 主轴默认策略 | `DirectionJustify.grow` | `DirectionJustify::Grow` | 一致 |
| grow 分配 | `remaining * grow / growAll`（basis 实效 0） | 同款 | 一致 |
| 交叉轴 Stretch | `align.size(size())` | 同款 | 一致 |
| 嵌套滚动 | 到边界不 `stopPropagation` | 同款 | 一致 |
| `not_in_layout` / `skipDraw` | 有 | 有 | 一致 |
| `focusOrder` / `focusTrap` / `selectionEnabled` | 有 | 有 | 一致 |
| `align_fix` | `StackObject.alignFix`（默认 false） | **wy-layout 有，Node 层没开口** | 半成品 |

以下 8 处偏差按「影响 × 成本」排序，给出改法与波及面。

---

## P0-1 `gap` 默认值 8.0 vs Kotlin 0

- **现状**：`Node::default().gap = 8.0`（`wy-mve/src/node.rs:316`）；
  而 `wy-layout` 的 `FlexObject::gap()` 默认 `0.0`。两层默认值不一致，
  Node 层的 8.0 实际生效。
- **影响**：每个容器默认 8px 间距。"忘了写 `gap: 0.0`"的容器与 Kotlin 版
  行为不同；示例层大量显式 `gap: 0.0` 其实是在还这笔债。
- **改法**：`Node::default()` 的 gap 改 `0.0`。
- **波及**：wy-mve 全部测试（有测试断言默认 8 的话要同步改语义）；
  两个示例里"没写 gap"的容器——改前先 `grep` 一遍确认没有依赖。
- **风险**：低。一行改动，但它是**行为变更**，要全量测试 + 双示例快照对比。

## P0-2 `align_fix`：库里有能力，Node 层没开口

- **现状**：`wy-layout/src/stack.rs` 完整实现了 `align_fix`（交叉轴尺寸取
  外部 `inner_size`，而不是"非对齐子节点的最大尺寸"）；但 `wy-mve` 的
  `Node` 没有对应字段，`layout_boxes` 适配层也没透传。
- **影响**：交叉轴想表达"尺寸由父容器定、不由内容撑"时没有专门入口。
  现在 `align_items: Stretch` 一直在兼任——在子节点无自然尺寸时效果巧合
  一致，但语义不同（Stretch 是"把我拉大到父尺寸"，align_fix 是"父尺寸
  直接给我"），子节点有自然尺寸时行为分叉。
- **改法**：`Node` 加 `align_fix: bool`（默认 false）+ `rsx!` 属性；
  `layout_boxes` 的 StackObject 适配实现透传。
- **风险**：中。库内逻辑已存在且有测试（`StackLayoutIgnoreTest` 等同源），
  主要工作量在 Node→FlexObject/StackObject 适配层。

## P0-3 padding 单值 vs Kotlin 四向

- **现状**：`Node.padding: f32`（四向同值，`node.rs:142`）。
- **Kotlin**：`argPadding(direction, startEnd)` 四向独立
  （`paddingInlineStart/End` + `paddingBlockStart/End`，inline/block 术语
  与书写方向解耦）。
- **影响**：表达不了"左右 12、上下 0"——finance 表头被迫 `padding: 12.0`
  再在文字 y 上手算补偿；`scroll` 的 padding 也是单值。
- **改法**（叠加式，不动现有语义）：保留 `padding` 作四向同值简写，
  新增 `pad_x` / `pad_y`（或 `pad_start/end/top/bottom`）。生效规则：
  四向简写为底，`pad_x/pad_y` 覆写。`node.rs:754/897` 两处 padding 收敛点
  改为读合成后的四向值。
- **风险**：中。布局与自然尺寸、clip 都要走合成值；但收敛点少。

## P1-1 `LayoutSize(value, fromInside)` 缺失

- **Kotlin**：`argWidth/argHeight` 是 `LayoutSize(value, fromInside)`：
  `fromInside=true` → value 是**内容区**尺寸（外包 = value + padding×2）；
  `false` → value 是**外包**尺寸（内容 = value − padding×2）。
  见 `LayoutNode.kt` 的 `outerSize` / `innerSize`。
- **rust-mve**：width/height 一律外包（`node.rs:778-782` 的注释明确：
  自然尺寸要补回 padding）。
- **影响**：写"内容区固定 200px 高"的声明式组件要把 padding 自己加回去，
  这是组件层反复踩的坑。
- **改法**（叠加式）：新增 `from_inside: bool`（或 `width_in` / `height_in`
  属性），`outer_size` / `inner_size` 按 Kotlin 公式换算。
- **风险**：中低。默认 false 即现状，纯增量。

## P1-2 事件模型单相位（无捕获阶段）

- **Kotlin**：每个指针事件都有 Capture 变体（`onPointerDownCapture` 等）。
  命中链**先捕获（子→根）再冒泡（根→子）**，均可 `stopPropagation`。
- **rust-mve**：只有单相位（`on_down_fn` 等在冒泡链上）。
- **影响**：外层想"比后代更早"拦截事件时没有入口（如容器先于内部按钮
  看到按下）；现在 overlay「点外面关闭」靠根 `on_click` 兜底，能用但不通用。
- **改法**：`wy-engine/src/mve_integration.rs` 的 `hit_test_node` 已经产出
  完整命中链——分发时正向走一遍捕获、反向走一遍冒泡即可，实现量小。
- **风险**：中。动事件核心路径，需要专门的捕获/冒泡顺序测试。

## P1-3 双击 / 右键菜单缺失

- **Kotlin**：`onPointerDoubleClick`（双击**不吞**单击，同一次双击仍先派发
  click）+ `onPointerContextMenu`（右键按下触发，且右键**不派发**
  `onPointerDown`，避免误启文本拖选/滑块拖动）。
- **rust-mve**：两者皆无。
- **影响**：交易终端「双击改单、右键菜单」是常见交互，现在做不到。
- **改法**：引擎侧加双击判定（时间间隔 + 落点距离阈值）与鼠标右键位；
  `Node` 加两个回调 + `rsx!` 属性（`on_double_click` / `on_context_menu`）。
- **风险**：低-中，纯增量；双击判定常量要对齐 Kotlin（见其 `EngineGlobal`
  或手势层）。

## P1-4 交叉轴显式定位靠「x/y 非 0」隐式约定

- **Kotlin**：`AlignChild`（`alignX`/`alignY`）显式声明交叉轴对齐方式，
  不靠坐标值猜意图。
- **rust-mve**：以「x/y 非 0」推断显式定位——`x: 0.0` 的显式贴齐与
  「不定位」无法区分。
- **影响**：想显式定位到 0（贴左/贴顶）时反而要写个非 0 值再算回来；
  语义不自解释。
- **改法**：`x_fn`/`y_fn` 的存在（`Option` 已经有）即显式——把判据从
  「值非 0」改为「有 fn」。静态 `x`/`y` 字段降级为简写。
- **风险**：中。**必须先审计** wy-mve 内部所有读 `x`/`y` 的位置
  （`pos_x()`/`pos_y()` 及布局分支），再动判据。

---

## 建议实施顺序

1. **P0-1 gap 默认**——一行改动 + 测试修正，收益/成本比最高
2. **P0-2 align_fix 开口**——库已实现，接出来即可
3. **P0-3 padding 四向**——叠加式 API，示例层的 `padding: 12` 手算补偿可以拆掉
4. **P1-1 from_inside**——叠加式 API
5. **P1-3 双击/右键**——纯增量
6. **P1-2 捕获相位**——动事件核心，单独一轮 + 专门测试
7. **P1-4 显式定位约定**——需先审计内部用法，放最后

## 每项的验证要求（合并为一轮 CI 门禁）

- wy-mve 单测补对应语义测试（Kotlin 侧 `FlexLayoutTest` /
  `StackLayoutIgnoreTest` 等可对标）
- shadcn + finance 双示例离屏快照对比（视觉回归）
- `cargo clippy --workspace -- -D warnings` + `cargo fmt --check`
- 默认值变更项（P0-1）额外：全仓 `cargo test --workspace --lib`

## 不在本方案范围

- 非 UI 功能对齐（binance.rs 增量簿同步 / feed 探活故障转移 / K 线上限）：
  已单列诊断，与布局层解耦，可并行推进。
