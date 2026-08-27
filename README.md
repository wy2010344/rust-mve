# wy-ui

纯 Rust 原生、信号驱动的高性能 UI 框架。

## 定位

对标 GPUI（Zed）、Iced、Xilem（Linebender），目标是：

- **纯 Rust**：无 C++ 依赖（不桥接 Skia/Qt）
- **信号驱动**：push-pull 混合响应式，构造只执行一次
- **GPU 加速**：Vello compute shader 2D 渲染 + wgpu 跨平台
- **小体积**：release + lto + strip 后 5-8MB
- **高灵活度**：用户可在 Widget 的 `draw()` 中直接操作 Scene，控制每个像素

## 架构

```
┌─────────────────────────────────────────────────┐
│                用户 Widget 代码                    │
│   struct MyWidget { count: Signal<i32> }          │
│   impl Widget for MyWidget { ... }               │
└─────────────────────┬───────────────────────────┘
                      │ children() / draw()
┌─────────────────────▼───────────────────────────┐
│               wy-engine                          │
│   事件系统 · 焦点管理 · 无障碍 · 动画             │
└──────┬──────────────┬──────────────┬────────────┘
       │              │              │
┌──────▼──────┐ ┌─────▼─────┐ ┌─────▼─────┐
│  wy-signal  │ │ wy-layout │ │ wy-text   │
│  Signal     │ │ Flex/Stack│ │ Parley    │
│  Memo       │ │ Taffy     │ │ shaping   │
│  TrackSignal│ │           │ │ 缓存      │
└─────────────┘ └───────────┘ └───────────┘
       │              │              │
┌──────▼──────────────▼──────────────▼────────────┐
│               wy-render                          │
│   Scene 中间层 · Widget trait · Vello 集成       │
└─────────────────────┬───────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────┐
│          Vello + wgpu（GPU compute 2D）           │
│     Vulkan · Metal · DX12 · WebGPU              │
└─────────────────────────────────────────────────┘
```

## 技术栈

| 层 | 方案 | 说明 |
|---|---|---|
| 渲染 | **Vello** + wgpu | GPU compute shader 2D 渲染，纯 Rust |
| 布局 | **Taffy** | Flexbox / Grid，GPUI/Iced/Dioxus 共用 |
| 文本 | **Parley** + swash | 富文本排版 + 字体光栅化，linebender 生态 |
| 窗口 | **winit** | 跨平台窗口抽象，事实标准 |
| 无障碍 | **AccessKit** | 屏幕阅读器 / 语音控制支持 |
| 信号 | **自研** | push-pull 模型 + memo relay map 优化 |

## 信号模型

```rust
// 信号是响应式的核心
let count = cx.create_signal(0);

// Widget 通过闭包读取信号（自动追踪依赖）
text(move || format!("Count: {}", count.get()));

// 写入信号 → 自动触发依赖它的 memo/layout/draw 重算
count.set(count.get() + 1);
```

关键特性：
- **构造只执行一次**：Widget 树在初始化时构建，之后通过信号驱动更新
- **自动依赖追踪**：在 memo/layout/draw 闭包中读取 `.get()` 时自动注册依赖
- **批量更新**：多个信号写入自动合并为一次重算
- **Memo 缓存**：派生值通过 relay map 惰性比对，避免不必要的重计算

## Widget 模型

```rust
use wy_ui::prelude::*;

struct Counter {
    count: Signal<i32>,
}

impl Widget for Counter {
    fn children(&self, cx: &mut ChildBuilder) {
        // 子节点声明，只执行一次
        cx.child(text(move || format!("Count: {}", self.count.get()))
            .font_size(24.0));

        cx.child(row(|cx| {
            cx.child(button("−", {
                let c = self.count.clone();
                move || c.set(c.get() - 1)
            }));
            cx.child(button("+", {
                let c = self.count.clone();
                move || c.set(c.get() + 1)
            }));
        }).gap(8.0));
    }

    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        // 读布局尺寸（自动注册依赖）
        let w = cx.outer_width();
        let h = cx.outer_height();

        // Scene API — GPU 自动批量处理
        scene.fill_round_rect(Rect::from_wh(w, h), 12.0, Color::WHITE);
        scene.stroke_round_rect(Rect::from_wh(w, h), 12.0, Color::from_hex(0xE0E0E0), 1.0);
    }
}
```

## 项目结构

```
wy-ui/
├── AGENTS.md              # AI 协作规范
├── Cargo.toml             # workspace 根配置
├── crates/
│   ├── wy-signal/         # 信号系统
│   ├── wy-layout/         # 布局引擎（Taffy 封装）
│   ├── wy-render/         # 渲染管线（Scene + Widget trait）
│   ├── wy-text/           # 文本排版（Parley 封装）
│   ├── wy-engine/         # 引擎整合（事件/焦点/无障碍）
│   └── wy-app/            # 示例应用
└── docs/                  # 详细文档
```

## 快速开始

```bash
# 运行计数器示例
cargo run -p wy-app --example counter
```

## 文档

- [AGENTS.md](AGENTS.md) — AI 协作规范与项目约束
- `docs/` — 详细使用文档（开发中）

## 设计参考

本项目的信号系统设计参考了 [wy-helper](https://github.com/your-name/wy-helper)（Kotlin + Skia 版本），核心理念：

- 信号通过闭包管道读取，不绑定值
- 构造只执行一次，动态组件通过 `renderForEach` 实现
- 选区是纯计算派生，无命令式状态
- Widget 的 `draw()` 直接操作 Scene，保留最大灵活度
