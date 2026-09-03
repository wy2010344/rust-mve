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
- 信号 MVE 是**管道精确更新**，不是 React 瀑布式重建：`render_root` 一次性构造 Node 树，之后由信号触发局部更新，`WidgetTree` 仅在信号变化时重建，不是每帧重建。

**渲染管线三阶段：Widget → Scene → GPU**
- Widget 的 `draw()` 只向 Scene 添加高层图元，**禁止直接操作 GPU 资源**。
- Scene 是平台无关中间表示，最终由 Vello/wgpu 提交。
- `draw()` 可读信号自动追踪依赖，但不应有副作用；避免每帧大量分配。

**布局是一维的：** `FlexLayout`/`StackLayout` 只回答主轴上的位置/尺寸两个标量，二维组合由上层叠加多个一维布局完成。

---

## 代码组织

- 声明式构造风格（用户明确偏好 `Node { ... }` 结构体字面量，拒绝链式/fluent API）。
- 单文件超 ~300 行时拆分模块；`lib.rs` 只做模块入口。
- 每个 crate 之间通过清晰 `pub` 接口交互，避免循环依赖。
- 只改与需求相关的最少代码；确需大改先说明再动手。

---

## 协作方式（从实际教训总结）

- **先评估再动手**：改动前先说明 understood 与潜在影响，尤其关注 Rust 所有权/生命周期（信号系统大量用 `Rc<RefCell>`/`thread_local`）。
- **从 Kotlin 对照理解**：本项目本质是复刻，遇到 Rust 侧语义不清时，先读对应 Kotlin 源码（`wy-helper-kotlin`）对照，再实现，必要时补测试。这是本项目验证正确性的主路径。
- **及时问用户**：实现方向有歧义、或需要大范围改动时，先问再做，别闷头写。

---

## 验证策略（本环境无 Rust toolchain）

- 当前环境（termux proot）没有 `cargo`/`rustc`，**无法本地编译/运行/测试**。
- 因此在无 toolchain 环境里，交付前用人工方式核验：对照相关 crate 的 `pub` API 签名、`wy-layout` 布局算法手算校验、核对 Kotlin 语义。
- 有工具链的地方（真机/CI/用户终端）仍需跑 `cargo check`、`cargo test`、`cargo fmt --check`、`cargo clippy -- -D warnings`。
- 无法本地验证时必须**明确说明**，不得假装已通过。
