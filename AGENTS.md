# AGENTS.md — AI 协作规范

本文件为 AI 编码助手提供项目协作规范。在本仓库工作时，请遵守以下规则。

---

## 一、语言

- **尽量用中文回复**，包括代码中的注释、commit message（如有）、以及与用户的交互。
- Rust 代码中的公共 API 文档注释使用英文（rustdoc 惯例），内部实现注释使用中文。

---

## 二、先评估，再动手

执行任何指令前，**先评估可行性与潜在问题**，说明后再动手。评估维度包括但不限于：

- **工具链约束**：本项目使用 Rust edition 2021+，cargo workspace 管理多 crate。注意 MSRV（最低支持 Rust 版本）要求。
- **所有权与生命周期**：Rust 的所有权模型是核心约束。信号系统大量使用 `Rc<RefCell<>>` 和 `thread_local!`，改动时必须考虑生命周期安全。
- **对架构的影响**：本项目采用分层 crate 设计（`wy-signal` / `wy-layout` / `wy-render` / `wy-text` / `wy-engine`），改动前思考是否破坏了 crate 边界和依赖方向。

---

## 三、不要用歪门方法绕问题

遇到问题时，**不要使用以下规避手段**：

- 不要通过 `unsafe` 绕过借用检查器来"解决"编译错误。
- 不要 `unwrap()` / `expect()` 滥用来忽略错误处理。
- 不要在不理解原理的情况下强行 `clone()` / `Rc::clone()` 来"解决"所有权问题。
- 不要用 `#[allow(...)]` 抑制 clippy 警告来"通过"检查。

正确的做法：

1. **先搜 skills**：使用 `npx skills find <关键词>` 搜索可用的技能。
2. **查阅 Rust API Guidelines**：参考 [rust-lang.github.io/api-guidelines](https://rust-lang.github.io/api-guidelines/)。
3. **找不到再问用户**：搜索无果时，向用户说明情况并请求指导。

---

## 四、代码风格一致性

- **遵循项目现有风格**：新增代码必须与同 crate 的风格保持一致。
- 所有代码必须通过 `rustfmt` 格式化：`cargo fmt`
- 所有代码必须通过 `clippy` 检查：`cargo clippy -- -D warnings`
- 遵循 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)：
  - 类型名 UpperCamelCase，函数/方法名 snake_case
  - 公共 API 必须有文档注释（`///`）
  - Error 类型实现 `std::error::Error` + `Display` + `Debug`

---

## 五、代码组织

- 单文件代码量过多时（建议超过 ~300 行），**恰当地拆分为模块**。
- 拆分时遵循 **高聚合、低耦合** 的原则：
  - 相关功能放在同一文件或子模块下；
  - crate 之间通过清晰的 `pub` 接口交互，避免循环依赖。
- 添加**必要的中文注释**方便阅读理解——尤其是核心算法、信号机制、渲染管线等不易直观理解的部分。
- 每个 crate 的 `lib.rs` 应该是模块的入口，不要放过多实现代码。

---

## 六、最小变更原则

- **只改与需求相关的最少代码**。不要顺手重构、不要大面积改格式、不要动无关 crate。
- 每处改动都应有明确目的。如果一段代码看起来"不太优雅"但不影响当前任务，留给将来处理。
- 如果确实需要大范围重构，**先单独说明并获得确认**。

---

## 七、先写测试，再写实现

- 遇循 **TDD（测试驱动开发）**：先写测试用例，再写实现代码。
- 测试放在对应 crate 的 `tests/` 目录或模块内 `#[cfg(test)]` 块中。
- 测试用例命名清晰，覆盖正常路径和边界情况。
- 信号系统的关键路径必须有测试：依赖收集、批量更新、memo 缓存失效。

---

## 八、提交前验证

- 修改完成后，**必须验证**：
  1. 编译通过：`cargo check`
  2. 格式正确：`cargo fmt --check`
  3. Clippy 通过：`cargo clippy -- -D warnings`
  4. 测试通过：`cargo test`
- 不能编译的代码视为**未完成**，不得交付。
- 涉及 unsafe 的代码必须额外运行：`cargo miri test`（如适用）。

---

## 九、依赖管理

- **不要随意引入新依赖**。如需引入，先说明理由、影响范围和可能的替代方案。
- 所有依赖版本统一在 workspace `Cargo.toml` 的 `[workspace.dependencies]` 中管理。
- 优先使用 Rust 标准库和项目已有依赖，避免功能重复的库。
- 优先选择无 `unsafe` 的纯 Rust 实现。
- 引入新的系统级依赖（如 GPU 驱动、字体库）必须充分说明跨平台影响。

---

## 十、公共 API 变更需同步文档

- 修改或新增公共 API（如 `Signal`、`Widget` trait、`Scene` 等）时，**必须确保 rustdoc 注释完整且准确**。
- 重大 API 变更需同步更新 `README.md` 和 `docs/` 下对应的文档。
- 确保文档与代码保持一致。

---

## 十一、安全红线

- **禁止**在代码或配置中写入密钥、token、密码等敏感信息。
- 调试用的临时代码（如 `dbg!()`、`println!()`、`TODO!()`、测试入口等）不要提交到仓库。
- 不要在代码中硬编码路径、用户名、环境相关的硬编码值。

---

## 十二、unsafe 使用策略

- **默认不使用 unsafe**。所有代码必须先尝试 safe 实现。
- 仅在以下场景允许 unsafe：
  - FFI 绑定（如绑定 Vello/wgpu 的 C 接口）
  - 性能关键路径（如信号系统的热路径，且有基准测试证明收益）
  - 实现必要的抽象（如 `Send`/`Sync` 的安全包装）
- 每处 unsafe 必须有 `// SAFETY:` 注释，说明为什么这里是安全的。
- unsafe 代码必须有对应的测试覆盖。

---

## 十三、平台分离

- 代码按平台通过条件编译分离：
  - `src/` — 跨平台通用逻辑
  - `src/platform/` — 平台特定实现
  - 使用 `#[cfg(target_os = "...")]` 或 feature flag 区分
- **不要**在平台无关代码中引用平台特定的 API。
- 跨平台共享的接口通过 trait 抽象。

---

## 十四、信号系统规范

本项目采用 **push-pull 混合信号模型**：

- **Signal**：叶子节点，存储值 + 订阅者集合。写入时 push 到批次队列。
- **Memo**：派生节点，惰性求值 + relay map 快照比对。读取时 pull 依赖。
- **TrackSignal**：副作用观察者，依赖动态收集。

关键约束：
- 信号读取（`.get()`）必须在 memo 或 track 的闭包内执行，以确保依赖被追踪。
- 不允许在 memo 计算期间写入信号（会 panic）。
- 批量更新通过 `batch()` 闭包包裹，结束后统一通知。
- `stateVersion` 使用 `u64` 递增，避免 ABA 问题。

---

## 十五、渲染管线规范

渲染管线分三阶段：**Widget → Scene → GPU**

- Widget 的 `draw()` 方法向 Scene 添加高层图元（Rect/Text/Shadow）。
- Scene 是平台无关的中间表示，最终由 Vello/wgpu 提交 GPU。
- **禁止**在 Widget 中直接操作 GPU 资源。
- Widget 的 `draw()` 可以读取信号（自动追踪依赖），但不应有副作用。

性能约束：
- `draw()` 应尽量轻量，避免每帧分配大量内存。
- 复杂图元（SVG、自定义路径）应缓存到 Scene 层。
- 文本排版结果应通过 memo 缓存，避免重复 shaping。

---

## 十六、项目概览

| Crate | 作用 |
|---|---|
| `wy-signal` | 信号系统：Signal、Memo、TrackSignal、批量更新 |
| `wy-layout` | 布局引擎：Flex/Stack/Absolute 一维布局（复刻 Kotlin wy-helper 模型，不依赖 Taffy） |
| `wy-render` | 渲染管线：Scene 中间层、Widget trait、Vello 集成 |
| `wy-text` | 文本排版：Parley/shaping、字体缓存、行布局缓存 |
| `wy-engine` | 引擎整合：事件系统、焦点管理、无障碍（AccessKit） |
| `wy-app` | 示例应用与工具 |

更多信息请参阅 [README.md](README.md)。
