# 00-Rust-UI框架调研报告

> 本文档是项目启动阶段对 Rust 原生 GUI 框架生态的调研结论，用于固化架构决策依据，
> 避免后续迷失方向时重复调研。最后更新：2026-08。

---

## 一、调研背景与目标

本项目（wy-ui）目标：纯 Rust 原生、信号驱动、GPU 加速、小体积的 UI 框架。

对比对象：GPUI、Iced、Floem、Xilem、egui、Makepad、Slint、Dioxus。

## 二、框架速览矩阵

| 框架 | 归属 | 渲染后端 | 状态模型 | 布局 | 文本 | 成熟度 | 设计哲学 |
|---|---|---|---|---|---|---|---|
| **GPUI** | Zed | 自制 Metal/Vulkan | 信号（context） | 自制 flex | 自制 | 中 | 高性能优先，专用 |
| **Iced** | 社区 | wgpu/glow | Elm（消息+订阅） | 自制 flex | cosmic-text | 高 | 函数式，组件模型 |
| **Floem** | Lapce | Vello + wgpu | 响应式视图（reactive_graph） | Taffy | Parley | 中 | 响应式，ref-count 视图重建 |
| **Xilem** | Linebender | Vello + wgpu | 增量信号（Xilem 项目） | Masonry | Parley | 早 | 未来派，基于 Xi 的旧理念 |
| **egui** | 社区 | wgpu/glow | 立即模式 | 立即模式 | 内置 | 高 | 立即模式，简单直接 |
| **Makepad** | Makepad | 自制 GPU | 保存的原生组件 + 响应式 DSL | 自研 | DrawBoss | 中 | 原生体验，live-design |
| **Slint** | Slint | FemtoVG/Skia/wgpu | 声明式 DSL+属性绑定 | 内置 | 内置 | 中 | 声明式，多语言绑定 |
| **Dioxus** | 社区 | 多种（HTML/桌面） | 响应式信号 | 因后端而异 | 因后端而异 | 高 | React-on-Rust，跨平台 |

## 二·五、技术栈逐层对比（完整版）

以下按"从底层到顶层"逐层列出各框架的选型，便于还原当初的技术栈对比全貌。

| 维度 | GPUI | Iced | Floem | Xilem | egui | Makepad | Slint | Dioxus | **wy-ui** |
|---|---|---|---|---|---|---|---|---|---|
| **窗口/事件** | winit | winit | winit | winit | winit | winit | winit | winit | winit |
| **渲染后端** | 自制 Metal/Vulkan | wgpu/glow | Vello+wgpu | Vello+wgpu | wgpu/glow | 自制 GPU | FemtoVG/Skia/wgpu | 跨后端 | **Vello+wgpu** |
| **2D 图元/Scene** | 自制 | 自制 | Vello Scene | Vello Scene | 自制网格 | 自制 | 自制 | 自制 | **Vello Scene** |
| **GPU 抽象** | 自制 | wgpu | wgpu | wgpu | wgpu | 自制 | wgpu | wgpu | wgpu |
| **布局引擎** | 自制 flex | 自制 flex | Taffy | Masonry | 立即模式 | 自研 | 内置 | 因后端 | **Taffy** |
| **文本 shaping** | 自制 | cosmic-text | Parley | Parley | 内置 | DrawBoss | 内置 | 因后端 | **Parley** |
| **字体光栅化** | 自制 | cosmic-font | swash | swash | 内置 | DrawBoss | 内置 | 因后端 | **swash+fontique** |
| **字体加载/回退** | 自制 | cosmic-font | fontique | fontique | 内置 | 自带 | 自带 | 因后端 | **fontique** |
| **状态模型** | 信号(Context) | Elm 消息 | reactive_graph | 信号+增量 | 立即模式 | 保存组件+DSL | 属性绑定 | 响应式信号 | **信号+Memo relay** |
| **API 形态** | trait+builder | `-> Element` | `view!` 宏 | view struct | 立即调用 | DSL 标记 | .slint DSL | 宏/Rsx | **闭包 builder** |
| **无障碍** | 部分 | accesskit | accesskit | accesskit | 无 | 无 | 内置 | 无 | **AccessKit** |
| **Shader 语言** | 自制 | WGSL | WGSL | WGSL | WGSL | 自制 | 因后端 | 因后端 | WGSL |
| **内存模型** | Rust 借用 | 无 GC | Rc 引用 | Rc 引用 | 每帧分配 | Rc | 值+句柄 | Rc | **Rc<RefCell>** |
| **是否现成组件库** | 部分(zed) | 有 | 部分 | 无 | 有 | 有 | 有 | 有 | 无(自建) |

> 说明：
> - **窗口/事件**：除极少数（Makepad 早期）外，主流框架几乎都用 **winit**，已是事实标准。
> - **渲染**：Floem/Xilem 与 wy-ui 同选 **Vello+wgpu**，是本项目渲染层最强参照。
> - **文本**：Parley/swash/fontique 为 Linebender 同源三件套，Floem/Xilem/wy-ui 共用。
> - **差异点**：wy-ui 在状态模型（Memo relay map 短路）与 API（闭包而非宏/DSL）上与主流不同。
> - **体积**：wy-ui 通过 release(lto/strip/abort) 目标 5-8MB，与 egui/Slint/Iced 相当。


## 三、按关键维度深挖

### 3.1 信号 / 响应式模型（本项目核心关注点）

**GPUI**
- 模型：`Context` 借用链 + `Model`/`Signal`。通过 `cx.notify()` 手动触发重绘，
  `cx.read_signal`/`cx.read_memo` 自动收集依赖。
- 特点：高性能，但 API 依赖复杂的借用、生命周期，学习曲线陡。
- 依赖收集：在 `draw()`/`layout()` 闭包内读取 `Signal`，写入通知。

**Floem**
- 模型：`reactive_graph`（Leptos 的前身）。view 闭包在信号变化时重新执行，
  ref-count 驱动的部分子树重建。
- 特点：对"视图结构性变化"（列表增删）响应自然；但每次更新涉及较多重建。

**Xilem**
- 模型：信号 + 增量 view。`WidgetView` 用 `==` 预检跳过未变子树。
- 特点：精简，尚未成熟。

**Iced**
- 模型：无信号。Elm 消息循环，全局状态 → 消息 → 更新 → 重绘整个树。
- 特点：简单可预测，但不做局部更新，大 UI 性能受限于差分。

**wy-ui（本项目）**
- 模型：**push-pop-混合**（源自 Kotlin 版 `wy-helper`）：
  - 构造只执行一次；动态属性通过 `layout()`/`draw()` 内的闭包读取信号。
  - Signal 写入 push 通知；Memo 用 relay map + 全局 stateVersion 做 O(1) 短路。
  - **关键差异**：静态结构不反复重建，只有真变化的 memo/draw 才重算。

**结论**：wy-ui 与 GPUI 思路相近（构造一次 + 闭包读信号），比 Floem 的
ref-count 重建更精细；relay map 快照比对优于 Floem 的简单信号追踪。
但需 benchmark 兑现——理论优势不等于实现优势。

### 3.2 渲染层（Vello 对比）

- **Vello**（Floem/Xilem 采用）：GPU compute shader 2D 渲染，`Scene` API，
  自动批处理，纯 Rust。**本项目选择**。
- **GPUI**：自研顶点/着色器管线，为 Zed 深度定制，不通用。
- **Makepad**：自研 GPU 管线，Bling/Curve 等抽象，特殊。
- **egui**：立即模式专用网格。
- **Iced**：可选 wgpu 或 glow。
- **Slint**：FemtoVG（软件）或 Skia（GPU）或 wgpu（实验）。

**结论**：Vello 是开源生态中成熟的 GPU 2D 方案，Floem/Xilem 已在生产使用，
是 wy-ui 渲染层的稳妥选择。

### 3.3 布局层

| 框架 | 布局 | 说明 |
|---|---|---|
| Iced/GPUI/Xilem | 自研 flex | 深度定制，非通用 |
| **Floem** | Taffy | 通用 Flexbox/Grid |
| **Taffy** | — | 独立库，Bevy/GPUI/Floem/Dioxus 共用 |

**结论**：Taffy 是生态事实标准，wy-ui 直接复用，避免重复造轮子。
早期版本可保留自研 flex 作为 fast-path（借鉴 Kotlin 版 FlexLayout），
V2 再全面切换到 Taffy 的完整 CSS 能力。

### 3.4 文本层

- **Parley + swash + fontique**（Linebender 系）：富文本排版 + 字体光栅化。
  Floem/Xilem 都在用。**wy-ui 选择**。
- **cosmic-text + cosmic-font**（Iced/Dioxus）。
- 自定义实现（GPUI/Slint/Makepad）。

**结论**：Parley 生态成熟，与 Vello 同源（Linebender），集成顺畅。

### 3.5 API 形态

| 框架 | API 风格 | 示例 |
|---|---|---|
| Iced | `-> Element` 纯组件组合 | `Button::new(text)` |
| Floem | `view!` 宏（HTML-like） | `view!{ cx, <div>...</div> }` |
| Slint | `.slint` DSL 文件 + 生成绑定 | `Text {}` |
| Makepad | 布局 + DSL   标记 | `<col>` |
| GPUI | Rust trait + builder | `h_flex().child(...)` |
| **wy-ui** | 纯 Rust 闭包 builder | `text(move || format!("{n}")).font_size(24.)` |

**结论**：wy-ui 采用**纯 Rust builder + 闭包**，不用宏或 DSL，在 Rust 侧更自然，
保留最大灵活性（可读信号、控制流）。相比 Floem 的 `view!` 宏少一层抽象，
对不熟悉 HTML 的开发者更友好；运行时无宏展开开销。

### 3.6 体积（release + lto + strip 目标）

| 框架 | 典型体积 |
|---|---|
| egui + wgpu | ~5-8MB |
| Slint | 6-12MB |
| Iced | 6-10MB |
| Floem | ~8-10MB（含全部后端） |
| **wy-ui 目标** | 5-8MB（lto="fat", strip="symbols", panic="abort"） |

**结论**：Vello+wgpu 基础层体积与同行相当；通过 release 优化腰斩二进制，
达到小体积目标。

## 四、风险与不确定点

1. **实现质量决定成败**：设计先进 ≠ 实现顺手。需 benchmark 证明 relay map /
   stateVersion 短路确实优于 Floem 的简单追踪。
2. **文本排版是瓶颈**：真正的 UI 性能瓶颈常在 shaping / GPU 提交，
   不在信号系统。需尽早验证 Parley + Vello 集成吞吐。
3. **生态差距**：Floem/Lapce、Iced 已有成熟组件与使用案例；wy-ui 从零起步，
   组件库（按钮/输入框/列表/滚动）需逐步搭建。
4. **依赖绑定**：Vello 后端变更可能牵连 API（如 wgpu 大版本升级）。
5. **C 依赖**：最终链接需要 Windows SDK（本机已确认缺失 kernel32.lib）。

## 五、最终技术栈决策

| 层 | 方案 | 理由 |
|---|---|---|
| 渲染 | **Vello + wgpu** | 成熟 GPU 2D，纯 Rust，自动批处理 |
| 布局 | **Taffy**（V2 全量，V1 可自研 fast-path） | 生态标准，Flexbox/Grid |
| 文本 | **Parley + swash + fontique** | 同源 Linebender，集成顺畅 |
| 窗口 | **winit** | 跨平台标准事件循环 |
| 无障碍 | **AccessKit** | 屏幕阅读器/语音支持 |
| 信号 | **自研**（push-pull 混合） | 差异化核心竞争点 |
| 示例 | **wy-app** | 验证与演示 |

## 六、与 Kotlin 版 wy-helper 的对应关系

| Kotlin 概念 | Rust wy-ui 对应 |
|---|---|
| 匿名类覆盖 getter（信号管道） | struct + 闭包版 `Widget` trait |
| `children()` 只执行一次 | `Widget::children()` |
| `renderForEach` 动态列表 | key 差分子节点生成 |
| memo 派生节点 | `wy-signal::Memo`（relay map） |
| TrackSignal 副作用 | `wy-signal::TrackEffect / Track` |
| 批量更新 `batchSignalEnd` | `wy-signal::batch`/`flush` |
| 选择模型纯计算派生 | 同上（无命令式状态） |
| `draw()` 直接画布控制 | `draw()` 操作 Vello `Scene` |

---

## 七、Q&A（启动阶段重要问题备忘）

**Q: 为什么不用 Iced？**
Iced 的 Elm 模型简单但无增量更新，且不需要引入状态消息循环的样板。

**Q: 为什么不用 egui？**
立即模式不适合"构造一次 + 信号驱动"的持久性 UI 场景，且每帧重建成本高。

**Q: 为什么不用 Slint？**
DSL + 生成绑定层对"保留最大控制力"的目标是负担；binding 模型也不如自研信号灵活。

**Q: 为什么不用 Makepad？**
自研 GPU 管线和 DSL，社区小，与生态脱节，学习/集成成本高。

**Q: 为什么对比 8 个里面没有提 Dioxus 的细节？**
Dioxus 是 React 范式 + 多后端，定位与 wy-ui（单后端原生+信号）不同，故不作主对比目标。

---

*本文档为决策记录，非教程。实际代码以 `crates/` 下源码为准。*
