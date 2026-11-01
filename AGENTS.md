# AGENTS.md — AI 协作规范

本文件只记录本项目的**特异性知识与真实协作中踩过的坑**。
通用工程常识（命名、格式化、clippy、不要提交密钥等）交给 AI 常识和工具本身，不在此重复。

AI 无法从代码里自推的内容才值得写在这里。

---

## 语言

- 与用户交互、代码内部注释、commit message 用中文。
- Rust 公共 API 的 rustdoc 用英文。

---

## 项目本质

复刻 Kotlin `wy-helper-kotlin`（`skia-engine`/`mve`/`signal`/`layout`）的 Rust MVE 实现。
核心包（signal / mve / layout）保持自研，渲染与其余部分尽量用生态（winit/wgpu/vello/accesskit/parley/log）。

| Crate | 作用 |
|---|---|
| `wy-signal` | 信号系统：Signal、Memo、TrackSignal、批量更新 |
| `wy-layout` | 布局引擎：Flex/Stack/Absolute **一维**布局（复刻 Kotlin，不依赖 Taffy） |
| `wy-render` | 渲染管线：Scene 中间层、Widget trait、Vello 集成 |
| `wy-text` | 文本排版：Parley/shaping、字体缓存、行布局缓存 |
| `wy-engine` | 引擎整合：事件、焦点、无障碍（AccessKit）+ MVE↔WidgetTree 桥接 |
| `wy-app` | 示例应用与工具 |

分层依赖方向：`wy-engine` → `wy-render`/`wy-mve` → `wy-layout`/`wy-signal`。改动别绕开这条边界。

---

## 架构关键约束（违反会出 bug）

**信号系统 push-pull 混合模型：**
- `.get()` 读信号必须在 memo 或 track 闭包内执行，才能被追踪为依赖。
- 不允许在 memo 计算期间写信号（会 panic）。
- 批量更新用 `batch()` 包裹，结束后统一 flush。
- `stateVersion` 用 `u64` 递增，避免 ABA。
- 信号 MVE 是**管道精确更新**，不是 React 瀑布式重建：`build_root_node2` 一次性构造 `Node2` 树，之后由信号触发局部重绘，`WidgetTree` 仅在信号变化时重建，不是每帧重建。

**渲染管线三阶段：Widget → Scene → GPU**
- Widget 的 `draw()` 只向 Scene 添加高层图元，**禁止直接操作 GPU 资源**。
- Scene 是平台无关中间表示，最终由 Vello/wgpu 提交。
- `draw()` 可读信号自动追踪依赖，但不应有副作用；避免每帧大量分配。

**布局是一维的：** `FlexLayout`/`StackLayout` 只回答主轴上的位置/尺寸两个标量，二维组合由上层叠加多个一维布局完成。

---

## 代码组织

- 声明式构造风格：节点用 `node2! { .. }` 命名参数宏（字段即 getter，写表达式不写值），
  拒绝链式/fluent builder。组件是 `#[rec]` 注解的普通函数，直接返回 `Fc`。
  反例与判定口径见 `docs/08-实现原则.md` §1。
- 单文件超 ~300 行时拆分模块；`lib.rs` 只做模块入口。
- 每个 crate 之间通过清晰 `pub` 接口交互，避免循环依赖。
- 只改与需求相关的最少代码；确需大改先说明再动手。

---

## 协作方式（从实际教训总结）

**动手前先读 `docs/08-实现原则.md`。** 那里是验收标准，不是背景资料；本项目
每一次返工都能归到它的某一条被跳过。

- **先评估再动手**：改动前先说明 understood 与潜在影响，尤其关注 Rust 所有权/生命周期（信号系统大量用 `Rc<RefCell>`/`thread_local`）。
- **从 TS/Kotlin 对照理解**：本项目本质是复刻，遇到 Rust 侧语义不清时，先读对应源码再实现。这是验证正确性的主路径。
- **对齐是双向的**：TS、Kotlin、Rust 谁错改谁，不许用"TS 就是这么写的"当挡箭牌（TS 里真有过 bug：占位文案承诺 Ctrl+Enter 却没接 `onSubmit`）。
- **及时问用户**：实现方向有歧义、或需要大范围改动时，先问再做，别闷头写。

### 三条最容易复发的红线

1. **不要退化成 builder / pipeline。** 只要开始写"构造对象 → 改字段 → append 子节点"、
   出现 `set_*` / `append` / `mount` / controller，就已经偏离声明式树形结构，
   停下来重写成 `node2! { .. }` / `#[rec] fn` 返回 `Fc`。**测试与文档示例同样算数**
   （`Props::builder()` 是 `!` 宏的机械通道，已 `#[doc(hidden)]`，手写即违规）。
2. **不要手算尺寸。** 先分清这个尺寸是**输入**（空态量不出来，必须写并注释）
   还是**结果**（父分配 / 子节点撑起，交给布局）。有 `layout` = 容器
   （`sizeFromChildren`），无 `layout` = 被摆放（`sizeFromParent`）。
3. **不要只改单侧。** 发现 Rust 与 TS 不一致时，先判定谁对，再改对应那一侧。

---

## 验证策略

- 本机**有** cargo/rustc：改完必须真的跑，不许"应该没问题"。
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets`（不留新告警）
  - `cargo test --workspace --lib` + 相关 `--example` 测试
  - 涉及绘制的改动：出快照并比对图元数 / `diff`（`--snapshot`、`--snapshot-size`）
- 跨仓库改动（`es-pseudo-monorepo` 的 TS 侧）要确认落盘：`git status`、`tsc --noEmit`。
- 修 bug 必须伴随一条能复现它的测试；只改代码不加测试视为没修完。
- 确实无法本地验证时（缺依赖、缺 GPU 等）必须**明确说明哪一步没验证**，不得假装已通过。
- 交付前删掉临时调试开关（`--dump`、临时 `eprintln`）。
