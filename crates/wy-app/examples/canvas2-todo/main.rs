//! 待办清单整页 —— 对照移植 mve-vite-demo 的 `pages/canvas-render2/`。
//!
//! | TS | 本示例 |
//! |---|---|
//! | `todo-store.ts` | [`store`]：信号状态 + 快照撤销/重做 + 主题调色板 |
//! | `todo-item.ts` | [`item`]：复选框(Path2) / 文本(链接片段) / 删除 |
//! | `todo-list.ts` | [`list`]：滚动容器 + `for_each_node` + 浮层滚动条 + 空态 |
//! | `todo-toolbar.ts` | [`toolbar`]：头部 / 过滤栏 / 操作栏 / 工具行 |
//! | `todo-demos.ts` | [`demos`]：修饰键 HUD / `noWrap` / 嵌套滚动 |
//! | `todo-editor.ts` | [`editor`]：编辑器面板（单行 / 多行 / 掩码 + 焦点顺序） |
//! | `view.ts` | 本文件：[`page`]——根节点就是那棵 flex 列 + 页面底色 |
//!
//! # 整棵树在一处声明
//!
//! [`page`] 就是 TS `view.ts` 的 `renderCanvas({ .., children: [..] })`：
//! 一棵 `node2! { .. }`，八个子组件按文档序放进 `children`。没有 builder 链、
//! 没有 `.mount()` 流水线——[`build_root_fc`] 直接把这棵声明树变成画布根。
//!
//! # 这个案例演示什么
//!
//! - **动态列表**：条目按 `todo.id` keyed 复用，增删不重建列表；
//! - **滚动**：容器 = 列表卡片本身（Row 布局），内容 `y = -value` 且
//!   `clip_to_parent` 把自己裁到卡片内盒；浮层滚动条不占布局宽度、静止时点击穿透；
//! - **跨组件选择**：每条的文本都是独立 `richTextNode`，按住从第一条拖到最后
//!   一条，各条高亮**各画各的**——选区真相只在 `SelectionManager2`（纯派生），
//!   节点只 `range_of` 读回；
//! - **TextDefaults 回写**：切主题时同步核心默认文本色，默认色读取方自动换肤；
//! - **编辑器**：单行回车提交 / 多行 Ctrl+Enter 提交 / 掩码圆点 / Tab 焦点循环
//!   （最后一块面板，见 [`editor`]）。
//!
//! 运行：`cargo run -p wy-app --example canvas2-todo`
//! 出图：`cargo run -p wy-app --example canvas2-todo -- --snapshot target/todo.png --snapshot-size 480x900`
//! 滚动后出图：再加 `--scroll 120`
//! 选区证据：再加 `--select`（出图前做一次等价 Ctrl+A 的全选）
//! 编辑器面板有内容：再加 `--fill "draft=买牛奶;note=第一行\n第二行;secret=hunter2"`
//! （`\n` 不会转义，需要真正换行就直接在 shell 里换行传进去）

mod demos;
mod editor;
mod item;
mod list;
mod store;
mod toolbar;

// `#[rec]` 生成的 `!` 宏里写的是 `$crate::XxxArg`——`$crate` 指**本 crate 根**，
// 所以组件放在子模块时，它的 Arg 类型必须在根上再导出一次，否则
// `cannot find TextButtonArg in $crate`。finance/ui2 里同样有一行这样的再导出。
pub use toolbar::{BarArg, TextButtonArg};

use std::cell::Cell;
use std::rc::Rc;

use wy_app::snapshot;
use wy_engine::node2_app::Node2App;
use wy_engine::runner::{run, WyApp};
use wy_mve::canvas_render2::build_root_fc;
use wy_mve::{flex, node2, AlignItem, DirectionJustify, Fc, Layout, Node2, Scene};
use wy_render::Rect;

/// 整页（对标 TS `view.ts`）：根自己就是那棵 flex 列。
///
/// TS `renderCanvas({ layout: flex({ gap:12, alignItem:'center', alignFix:true,
/// directionJustify:'center' }), children: [...] })`——根是**布局容器本身**，
/// 不是"绝对定位空壳里再塞一列"：后者的 `child_size` 会报错（→ 子列宽 0），
/// 所有内容挤到左上角并被裁掉。
fn page(viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    // 两个 getter 各持一份 viewport（`move ||` 会把它搬进闭包）。
    let (vp_w, vp_h) = (Rc::clone(&viewport), viewport);
    node2! {
        // 根没有父节点，位置无从推导，必须显式给 0（TS 画布根同款）。
        x: 0.0,
        y: 0.0,
        width: vp_w.get().0,
        height: vp_h.get().1,
        // 竖向排列、水平居中（子节点均显式定宽）
        layout: flex! {
            direction: Layout::Column,
            gap: 12.0,
            align_item: AlignItem::Center,
            align_fix: true,
            direction_justify: DirectionJustify::Center,
        },
        // 页面底色由根自己画（先于子节点）
        draw: |scene: &mut Scene, n: &Node2| {
            scene.fill_rect(
                Rect::new(0.0, 0.0, n.width(), n.height()),
                store::palette().page,
            );
        },
        children: vec![
            toolbar::todo_header(),
            toolbar::filter_bar(),
            list::todo_list(),
            toolbar::action_bar(),
            toolbar::tool_bar(),
            // TS `view.ts` 里紧跟工具行的三块新特性演示 + 编辑器面板
            demos::modifier_hud(),
            demos::nowrap_demo(),
            demos::nested_scroll_demo(),
            editor::editor_panel(),
        ],
    }
}

fn make_app() -> Node2App {
    Node2App::new(move |viewport: Rc<Cell<(f32, f32)>>| build_root_fc(page(viewport)))
        .with_title("wy-ui · canvasRender2 todo")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    store::apply_theme();
    store::seed();

    let args: Vec<String> = std::env::args().collect();
    let select = args.iter().any(|a| a == "--select");
    let scroll_to = args
        .iter()
        .position(|a| a == "--scroll")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok());
    // 把一次「滚轮量」喂给**内层**滚动：超出内层可滚范围的剩余量应经
    // `parentScroll` 接续外层（TS `nestedScrollDemo` 的演示目标）。
    let nested_delta = args
        .iter()
        .position(|a| a == "--nested")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok());
    // 给编辑器面板预置文本（`--fill draft=xxx note=yyy secret=zzz`）：出图时
    // 才能看到"真文本 / 多行换行 / 掩码圆点"三种形态，而不是三个占位串。
    if let Some(spec) = args
        .iter()
        .position(|a| a == "--fill")
        .and_then(|i| args.get(i + 1))
    {
        editor::fill_from_spec(spec);
    }

    if let Some((path, w, h)) = snapshot::parse_snapshot_args() {
        let app = make_app();
        // 滚动值要在布局落定后才钳得到 `[0, maxScroll]`——先空跑一帧预热，
        // 再设值、再出图（否则第一次读到 maxScroll = 0，滚动被钳回原点）。
        if let Some(px) = scroll_to {
            let mut warm = Scene::new();
            app.draw(&mut warm, w as f32, h as f32);
            if let Some(scroll) = list::last_scroll() {
                scroll.set_value(px);
            }
            // 浮现动画（fadeIn 110ms）也得走完再出图，否则抓到的是 alpha≈0 的
            // 第一帧——"滚了但看不见滚动条"是最容易被误判成没实现的地方。
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if select {
            app.selection_manager().select_all();
        }
        if let Some(delta) = nested_delta {
            // 同 `--scroll`：先空跑一帧让布局落定，`maxScroll` 才是真值。
            let mut warm = Scene::new();
            app.draw(&mut warm, w as f32, h as f32);
            if let Some((outer, inner)) = demos::last_nested() {
                let real = inner.scroll(delta);
                eprintln!(
                    "nested: delta={delta} 内层真实滚动={real} 内层={} 外层={}",
                    inner.value(),
                    outer.value()
                );
            }
        }
        snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    run(make_app())?;
    Ok(())
}
