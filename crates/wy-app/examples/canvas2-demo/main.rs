//! canvasRender2 组件展示 —— 对照移植 mve-vite-demo 的
//! `pages/canvas-render2-basic/view.ts`。
//!
//! 这里刻意**逐项对齐 TS 源码**，而不是另写一个"好看的 demo"：
//!
//! | TS `view.ts` | 本文件 |
//! |---|---|
//! | `renderCanvas({ layout: flex({ directionJustify:'center', gap:10, alignFix:true }) })` | [`page`]：根节点本身就是这棵 flex 列 |
//! | `rectNode({ width:200, height:90, background, borderColor, borderWidth, radius, padding })` | [`rect_demo`]：`rect_node! { .. }` |
//! | `imageNode({ image, radius:12, padding:8, mouseClick })` | [`image_demo`]：`image_node! { .. }` |
//! | `richTextNode({ width:320, padding:8, spans:[...] })` | [`rich_demo`]：`rich_text_node! { .. }` |
//! | `richTextNode({ autoWidth:true, padding:8, spans:[...] })` | [`rich_auto_demo`] |
//! | `richTextNode` 逐条列表项（`canvas-render2/todo-item.ts`） | [`select_demo`]（跨组件选择） |
//!
//! # 写法：声明式树 + `#[getter]` 语义
//!
//! 每个组件都是一次 `xxx! { .. }` 声明：`background` / `spans` / `image` 这些
//! **读取式参数直接写表达式**（`#[rec]` 宏把它包成 `move || 表达式`，绘制期才求值），
//! 节点参数与视觉参数写在同一个块里，与 TS 的 `rectNode({ .. })` 同构。
//! 没有 `XxxArg::builder()` 链，也没有 `.rest(Node2Props::builder()..)`。
//!
//! # 关键：根必须是 flex 容器本身
//!
//! TS 的根布局是 `directionJustify.center + alignFix`，因此**根自己的交叉轴尺寸
//! 取自父级（= 视口）**，子节点才能在整幅画布里居中。[`build_root_fc`] 收的就是
//! 这棵声明树——**不要**在外面再套一层绝对定位的空壳：`AbsoluteLayout::child_size`
//! 会报错（→ 宽 0），里面的列拿不到宽度，所有子节点都会被挤到左上角并被左边缘裁掉。
//!
//! 两个 demo 都在 `examples/` 下自动被发现（`examples/<name>/main.rs`）。
//!
//! 运行：`cargo run -p wy-app --example canvas2-demo`
//! 出图：`cargo run -p wy-app --example canvas2-demo -- --snapshot target/canvas2.png --snapshot-size 520x820`
//! 选区证据：再加 `--select`（出图前 `selectAll()`，一张图看到跨组件高亮）

use std::cell::Cell;
use std::rc::Rc;

use wy_engine::node2_app::Node2App;
use wy_engine::runner::run;
use wy_mve::canvas_render2::build_root_fc;
use wy_mve::canvas_render2::RichTextSpan;
use wy_mve::{
    flex, image_node, node2, rect_node, rich_text_node, wrapped_text_node, AlignItem,
    DirectionJustify, Fc, Layout, Node2, Scene,
};
use wy_render::{Color, PlatformImage, Rect};
use wy_signal::{GetValue, SetValue, Signal};
use wy_text::TextStyle;

use wy_app::snapshot;

// ============================================================================
// 配色（对齐 TS demo 的浅色 slate 系）
//
// ⚠️ 端口陷阱：TS 用 `0xAARRGGBB`（`0xfff1f5f9` = 不透明 slate-100），
// 而 Rust 的 `Color::from_u32` 收的是 **`0xRRGGBBAA`**。照抄 TS 字面量会把
// 通道和 alpha 全打乱（`0xfff1f5f9` 在 RGBA 里是"半透明、通道移位"的怪色）。
// 所以下面统一改写成 `0xRRGGBBAA`：前六位是 RGB，末两位固定 `FF` 不透明。
// ============================================================================

/// 画布底色（slate-50 `#f8fafc`）——`renderCanvas` 的默认底色。
const BG: Color = Color::from_u32(0xF8FAFCFF);
/// rect 背景（slate-100 `#f1f5f9`）。
const SLATE_100: Color = Color::from_u32(0xF1F5F9FF);
/// rect 边框（slate-300 `#cbd5e1`）。
const SLATE_300: Color = Color::from_u32(0xCBD5E1FF);
/// 次要文字（slate-500 `#64748b`）。
const SLATE_500: Color = Color::from_u32(0x64748BFF);
/// 主文字（slate-900 `#0f172a`）。
const SLATE_900: Color = Color::from_u32(0x0F172AFF);
/// 正文（slate-700 `#334155`）。
const SLATE_700: Color = Color::from_u32(0x334155FF);
/// 富文本正文色（`#333333`）。
const INK: Color = Color::from_u32(0x333333FF);
/// 富文本强调色（`#3d7bff`）。
const BLUE: Color = Color::from_u32(0x3D7BFFFF);
/// 富文本灰字（`#888888`）。
const GREY: Color = Color::from_u32(0x888888FF);

// ============================================================================
// 离屏渐变图（对齐 TS `makeImage`：无外部资源依赖）
// ============================================================================

/// 用 RGBA 缓冲区生成一张**对角**线性渐变测试图。
///
/// TS 用离屏 canvas 的 `createLinearGradient(0,0,w,h)`（左上 → 右下）；
/// 这里按 `(x + y) / (w + h - 2)` 做同样的对角插值。
fn gradient(w: u32, h: u32, ca: u32, cb: u32) -> PlatformImage {
    let ar = (ca >> 24) & 0xff;
    let ag = (ca >> 16) & 0xff;
    let ab = (ca >> 8) & 0xff;
    let br = (cb >> 24) & 0xff;
    let bg = (cb >> 16) & 0xff;
    let bb = (cb >> 8) & 0xff;
    let span = (w as f32 + h as f32 - 2.0).max(1.0);
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = (x as f32 + y as f32) / span;
            let r = (ar as f32 + (br as f32 - ar as f32) * t) as u8;
            let g = (ag as f32 + (bg as f32 - ag as f32) * t) as u8;
            let b = (ab as f32 + (bb as f32 - ab as f32) * t) as u8;
            bytes.extend_from_slice(&[r, g, b, 255]);
        }
    }
    PlatformImage::from_rgba8(w, h, bytes).expect("渐变图字节长度应匹配")
}

// ============================================================================
// 文本片段（TS 里 span 是数据，不是组件）
// ============================================================================

/// 一个文本片段，对应 TS `{ text, style: { fontFamily, fontSize, color } }`。
///
/// 是**数据**不是组件：`rich_text_node!` 的 `spans` 收的就是它的列表，
/// 与 TS 把 span 写成对象字面量一致。
fn span(text: &str, style: TextStyle) -> RichTextSpan {
    RichTextSpan {
        text: text.to_string(),
        style,
        url: None,
    }
}

// ============================================================================
// 四个 canvasRender2 组件
// ============================================================================

/// `rectNode`：背景 + 边框 + 圆角 + padding 一站式盒子。
///
/// TS 里内层文本被注释掉了（当时 rect 内子节点排版没跑通）；这里补上两个
/// 文本子节点，正好检验"rect 作为容器 + 内部 flex 列排版"这条路。
/// 这是双向对齐的一个样本：TS 侧那两段注释应恢复。
fn rect_demo() -> Fc {
    rect_node! {
        background: Some(SLATE_100),
        border_color: Some(SLATE_300),
        border_width: 1.0,
        radius: 8.0,
        width: 200.0,
        height: 90.0,
        padding: 10.0,
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Start,
            gap: 6.0,
        },
        children: vec![
            // 宽度由内容撑开（`autoWidth`），父级 `align_item: Start` 才真正把它们
            // 靠左排（宽度为 0 的节点对齐=贴左，看着像没生效）。
            wrapped_text_node! {
                auto_width: true,
                text: "rectNode：背景 / 边框 / 圆角",
                font_size: 13.0,
                color: SLATE_900,
            },
            wrapped_text_node! {
                auto_width: true,
                text: "内部是 wrappedTextNode 子节点",
                font_size: 11.0,
                color: SLATE_500,
            },
        ],
    }
}

/// `imageNode`：原图尺寸 + 圆角 + padding，点击切换两张图（信号驱动重绘，
/// 对照 TS `current.set(...)`）。
fn image_demo() -> Fc {
    // #ff8a3d → #ffd23d（橙 → 黄）；#3d7bff → #3dffd2（蓝 → 青）。
    let items: [(u32, u32); 2] = [(0xFF8A3DFF, 0xFFD23DFF), (0x3D7BFFFF, 0x3DFFD2FF)];
    let first = Signal::new(true);
    image_node! {
        // 每次读取现生成：信号每次 tick 都会读，避免依赖 `PlatformImage` 的 Clone。
        image: {
            let (a, b) = if first.get() { items[0] } else { items[1] };
            gradient(120, 80, a, b)
        },
        radius: 12.0,
        padding: 8.0,
        mouse_click: move |_e: &mut wy_mve::canvas_render2::MouseEvent2| {
            first.set(!first.get());
        },
    }
}

/// 定宽富文本：自动换行、按住拖拽可划词（选区需要接 `SelectionManager2`，
/// 这里只演示排版 + 多片段样式）。
fn rich_demo() -> Fc {
    rich_text_node! {
        // 读取式：宏把这段表达式包成 `move || ..`，每次重绘重新构造片段列表
        spans: vec![
                span(
                    "富文本",
                    TextStyle::normal()
                        .with_font_size(20.0)
                        .with_font_weight(700)
                        .with_color(INK.to_u32()),
                ),
                span(
                    "RichText",
                    TextStyle::normal()
                        .with_font_size(16.0)
                        .with_color(BLUE.to_u32()),
                ),
                span(
                    "，按住拖拽可以划词选中（CanvasKit 渲染）。",
                    TextStyle::normal()
                        .with_font_size(14.0)
                        .with_color(GREY.to_u32()),
                ),
        ],
        width: 320.0,
        padding: 8.0,
    }
}

/// `autoWidth` 富文本：宽度由内容决定。
fn rich_auto_demo() -> Fc {
    rich_text_node! {
        spans: vec![span(
            "autoWidth：宽度由内容决定",
            TextStyle::normal()
                .with_font_size(16.0)
                .with_color(INK.to_u32()),
        )],
        auto_width: true,
        padding: 8.0,
    }
}

/// 跨组件选择：一块面板里两段**独立**的富文本节点。
///
/// 对标 TS `canvas-render2/todo-item.ts` —— 逐条列表项各自一个 `richTextNode`。
/// 按住从第一段拖到第二段，两段会各自被分配一段范围，高亮**分别由两个节点
/// 自己绘制**：选区真相只在 `SelectionManager`（纯派生），节点只负责读回。
/// 这正是"跨组件选择"与"节点内选择"的分界：节点自装拖选永远只能同节点内选。
fn select_demo() -> Fc {
    rect_node! {
        background: Some(SLATE_100),
        border_color: Some(SLATE_300),
        border_width: 1.0,
        radius: 8.0,
        width: 420.0,
        padding: 12.0,
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Start,
            gap: 4.0,
        },
        children: vec![
            paragraph(
                "第一段：选区不是节点的本地状态，",
                "拖过本段末尾会接上下一段。",
            ),
            paragraph("第二段：谁都不持有选区，", "范围由管理器按文档序派生。"),
        ],
    }
}

/// 一段双色富文本（定宽，便于观察跨节点选区在行尾/行首的衔接）。
fn paragraph(head: &'static str, tail: &'static str) -> Fc {
    rich_text_node! {
        spans: vec![
            span(
                head,
                TextStyle::normal()
                    .with_font_size(14.0)
                    .with_color(SLATE_700.to_u32()),
            ),
            span(
                tail,
                TextStyle::normal()
                    .with_font_size(14.0)
                    .with_color(SLATE_500.to_u32()),
            ),
        ],
        width: 396.0,
    }
}

// ============================================================================
// 应用：根节点就是 flex 列
// ============================================================================

/// 整页（对标 TS `renderCanvas({ .., children: [..] })`）。
///
/// 对齐 TS：`flex({ directionJustify:'center', gap:10, alignFix:true })`
/// （不写 direction → 默认列）。根自身尺寸取自视口，故 `align_fix` 让交叉轴
/// 直接用"整幅画布的宽"来居中子节点。
fn page(viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    let (vp_w, vp_h) = (Rc::clone(&viewport), viewport);
    node2! {
        x: 0.0,
        y: 0.0,
        width: vp_w.get().0,
        height: vp_h.get().1,
        padding: 20.0,
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Center,
            align_fix: true,
            direction_justify: DirectionJustify::Center,
            gap: 10.0,
        },
        draw: |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), BG);
        },
        children: vec![
            rect_demo(),
            image_demo(),
            rich_demo(),
            rich_auto_demo(),
            select_demo(),
        ],
    }
}

fn make_app() -> Node2App {
    Node2App::new(move |viewport: Rc<Cell<(f32, f32)>>| build_root_fc(page(viewport)))
        .with_title("wy-ui · canvasRender2 demo")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--select`：出图前先全选一次，用一张图证明"跨组件高亮真的画出来了"
    // （真实交互里由指针路由驱动，不需要这个开关）。
    let select = std::env::args().any(|a| a == "--select");
    if let Some((path, w, h)) = snapshot::parse_snapshot_args() {
        let app = make_app();
        if select {
            app.selection_manager().select_all();
        }
        snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    run(make_app())?;
    Ok(())
}
