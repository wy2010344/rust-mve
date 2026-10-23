//! 矢量图标：自绘路径，不依赖字体字形。
//!
//! # 为什么不用字符图标
//!
//! 早期版本用 `✕ ✓ ▾ ▲ ▼ ≡ ⋯` 这类字符当图标。但字形是否存在**取决于系统
//! 字体**：字体缺该字形时，渲染出来是一个空心方框（俗称"豆腐块"）——
//! 界面上就表现为"一堆方块"，而不是图标。
//!
//! 这里改为**自绘矢量路径**：每个图标是一组折线/多边形顶点，由
//! [`wy_render::Scene::stroke_path`] / [`fill_path`] 画出，与字体完全无关，
//! 在任何机器上都一致。
//!
//! # 用法
//!
//! ```ignore
//! use wy_mve::{icon, Icon};
//!
//! icon { name: Icon::ChevronDown, size: 14.0, color: move || t.muted_fg }
//! ```
//!
//! 图标默认按 `size` 居中于自身盒子（盒子是 `size × size`），
//! 调用处可用 `grow` / `align_self` 调整在父容器中的位置。

use std::rc::Rc;

use crate::{component, current_box, rsx};
use wy_render::{Color, Point, Scene};

/// 矢量图标集合。
///
/// 每个变体对应一组归一化坐标（0..1 范围）的路径，
/// 绘制时按目标尺寸缩放。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    /// 勾（checkbox 选中态）。
    Check,
    /// 叉（删除 / 关闭）。
    X,
    /// 下尖角（chevron-down）。
    ChevronDown,
    /// 上尖角（chevron-up）。
    ChevronUp,
    /// 右尖角（chevron-right）。
    ChevronRight,
    /// 左尖角（chevron-left）。
    ChevronLeft,
    /// 三点横排（更多）。
    Dots,
    /// 三点竖排（更多-竖）。
    DotsVertical,
    /// 菜单（两横）。
    Menu,
    /// 加号。
    Plus,
    /// 减号。
    Minus,
    /// 向上箭头。
    ArrowUp,
    /// 向下箭头。
    ArrowDown,
    /// 齿轮（设置）。
    Gear,
    /// 铃铛（通知）。
    Bell,
    /// 搜索（放大镜）。
    Search,
    /// 房子（首页）。
    Home,
    /// 图表（统计）。
    Chart,
    /// 文件夹。
    Folder,
    /// 用户（头像占位）。
    User,
    /// 垃圾桶（删除）。
    Trash,
    /// 刷新（循环箭头）。
    Refresh,
}

impl Icon {
    /// 图标的归一化路径（0..1 坐标，绘制时按尺寸缩放）。
    ///
    /// 返回 `(&[&[f32]], filled)`：
    /// - 第一项：路径段列表，每段是扁平的 `[x0, y0, x1, y1, ...]` 坐标；
    /// - 第二项：是否填充（填充时每段是闭合多边形，否则是描边折线）。
    ///
    /// **多段分开**很重要：叉（两笔）、齿轮（两笔）等必须各自成段，
    /// 否则会被连成一条奇怪的折线。
    /// 图标的归一化路径（**公开**：供调用方在自己的 `draw` 里直接画图标，
    /// 免得为了一个图标再嵌一层子节点）。
    pub fn icon_paths(self) -> (&'static [&'static [f32]], bool) {
        match self {
            // 勾：两笔
            Icon::Check => (
                &[&[0.16, 0.52, 0.40, 0.74], &[0.40, 0.74, 0.84, 0.26]],
                false,
            ),
            // 叉：两笔交叉
            Icon::X => (
                &[&[0.22, 0.22, 0.78, 0.78], &[0.78, 0.22, 0.22, 0.78]],
                false,
            ),
            // 尖角：两笔
            Icon::ChevronDown => (
                &[&[0.20, 0.38, 0.50, 0.68], &[0.50, 0.68, 0.80, 0.38]],
                false,
            ),
            Icon::ChevronUp => (
                &[&[0.20, 0.62, 0.50, 0.32], &[0.50, 0.32, 0.80, 0.62]],
                false,
            ),
            Icon::ChevronRight => (
                &[&[0.38, 0.20, 0.68, 0.50], &[0.68, 0.50, 0.38, 0.80]],
                false,
            ),
            Icon::ChevronLeft => (
                &[&[0.62, 0.20, 0.32, 0.50], &[0.32, 0.50, 0.62, 0.80]],
                false,
            ),
            // 三点横排：三个实心小方块（填充）
            Icon::Dots => (
                &[
                    &[0.14, 0.44, 0.26, 0.44, 0.26, 0.56, 0.14, 0.56],
                    &[0.44, 0.44, 0.56, 0.44, 0.56, 0.56, 0.44, 0.56],
                    &[0.74, 0.44, 0.86, 0.44, 0.86, 0.56, 0.74, 0.56],
                ],
                true,
            ),
            // 三点竖排
            Icon::DotsVertical => (
                &[
                    &[0.44, 0.14, 0.56, 0.14, 0.56, 0.26, 0.44, 0.26],
                    &[0.44, 0.44, 0.56, 0.44, 0.56, 0.56, 0.44, 0.56],
                    &[0.44, 0.74, 0.56, 0.74, 0.56, 0.86, 0.44, 0.86],
                ],
                true,
            ),
            // 菜单：两横
            Icon::Menu => (
                &[&[0.20, 0.36, 0.80, 0.36], &[0.20, 0.64, 0.80, 0.64]],
                false,
            ),
            // 加号：两笔
            Icon::Plus => (
                &[&[0.50, 0.22, 0.50, 0.78], &[0.22, 0.50, 0.78, 0.50]],
                false,
            ),
            // 减号：一笔
            Icon::Minus => (&[&[0.22, 0.50, 0.78, 0.50]], false),
            // 箭头：竖杆 + 两笔箭头
            Icon::ArrowUp => (
                &[
                    &[0.50, 0.22, 0.50, 0.78],
                    &[0.28, 0.42, 0.50, 0.22],
                    &[0.72, 0.42, 0.50, 0.22],
                ],
                false,
            ),
            Icon::ArrowDown => (
                &[
                    &[0.50, 0.22, 0.50, 0.78],
                    &[0.28, 0.58, 0.50, 0.78],
                    &[0.72, 0.58, 0.50, 0.78],
                ],
                false,
            ),
            // 齿轮：四条辐条 + 中心方环（近似）
            Icon::Gear => (
                &[
                    &[0.46, 0.14, 0.54, 0.14],
                    &[0.46, 0.86, 0.54, 0.86],
                    &[0.14, 0.46, 0.14, 0.54],
                    &[0.86, 0.46, 0.86, 0.54],
                    &[0.34, 0.34, 0.66, 0.34, 0.66, 0.66, 0.34, 0.66],
                ],
                false,
            ),
            // 铃铛：钟身两笔 + 底沿 + 铃舌
            Icon::Bell => (
                &[
                    &[0.30, 0.64, 0.30, 0.42],
                    &[0.30, 0.42, 0.50, 0.20],
                    &[0.50, 0.20, 0.70, 0.42],
                    &[0.70, 0.42, 0.70, 0.64],
                    &[0.22, 0.64, 0.78, 0.64],
                    &[0.44, 0.76, 0.56, 0.76],
                ],
                false,
            ),
            // 搜索：镜圈（闭合菱形）+ 手柄
            Icon::Search => (
                &[
                    &[0.36, 0.24, 0.62, 0.24, 0.62, 0.50, 0.36, 0.50],
                    &[0.60, 0.62, 0.78, 0.80],
                ],
                false,
            ),
            // 房子：屋顶 + 两侧墙 + 底
            Icon::Home => (
                &[
                    &[0.16, 0.46, 0.50, 0.18],
                    &[0.50, 0.18, 0.84, 0.46],
                    &[0.28, 0.40, 0.28, 0.82],
                    &[0.72, 0.40, 0.72, 0.82],
                    &[0.28, 0.82, 0.72, 0.82],
                ],
                false,
            ),
            // 图表：三根柱
            Icon::Chart => (
                &[
                    &[0.22, 0.46, 0.22, 0.78],
                    &[0.46, 0.28, 0.46, 0.78],
                    &[0.70, 0.58, 0.70, 0.78],
                ],
                false,
            ),
            // 文件夹：标签 + 主体
            Icon::Folder => (
                &[
                    &[0.16, 0.30, 0.42, 0.30],
                    &[0.42, 0.30, 0.48, 0.40],
                    &[0.48, 0.40, 0.84, 0.40],
                    &[0.84, 0.40, 0.84, 0.72],
                    &[0.84, 0.72, 0.16, 0.72],
                    &[0.16, 0.72, 0.16, 0.30],
                ],
                false,
            ),
            // 用户：头（闭合）+ 肩
            Icon::User => (
                &[
                    &[0.38, 0.24, 0.62, 0.24, 0.62, 0.46, 0.38, 0.46],
                    &[0.24, 0.82, 0.24, 0.62],
                    &[0.24, 0.62, 0.76, 0.62],
                    &[0.76, 0.62, 0.76, 0.82],
                ],
                false,
            ),
            // 垃圾桶：桶沿 + 三笔
            Icon::Trash => (
                &[
                    &[0.28, 0.28, 0.72, 0.28],
                    &[0.36, 0.28, 0.36, 0.80],
                    &[0.64, 0.28, 0.64, 0.80],
                    &[0.36, 0.80, 0.64, 0.80],
                ],
                false,
            ),
            // 刷新：三段圆弧近似
            Icon::Refresh => (
                &[
                    &[0.76, 0.36, 0.62, 0.26, 0.48, 0.26],
                    &[0.34, 0.34, 0.26, 0.48, 0.26, 0.60],
                    &[0.34, 0.76, 0.48, 0.76, 0.62, 0.68],
                ],
                false,
            ),
        }
    }
}

/// 矢量图标节点。
///
/// `name` 为图标种类，`size` 为图标边长（像素），`color` 为描边色
/// （getter，可随主题变化）。
#[component]
pub fn icon(
    /// 图标种类。
    name: Icon,
    /// 图标边长（像素）。
    #[props(default = 14.0f32)]
    size: f32,
    /// 描边色（getter：裸 `Color` 或闭包）。
    #[getter]
    color: Color,
) -> Node {
    // getter 是 `Rc<dyn Fn()>`，draw 与 measure 各捕获一份
    let draw_color = Rc::clone(&color);
    rsx! {
        node {
            width: size,
            height: size,
            draw: move |s: &mut Scene| {
                let b = current_box();
                // 图标在自身盒子里居中（父容器给的盒子可能大于图标）
                let side = b.width.min(b.height).min(size);
                let ox = (b.width - side) / 2.0;
                let oy = (b.height - side) / 2.0;
                let c = draw_color();
                let (paths, filled) = name.icon_paths();
                for seg in paths {
                    let pts: Vec<Point> = seg
                        .chunks(2)
                        .map(|c| Point::new(ox + c[0] * side, oy + c[1] * side))
                        .collect();
                    if filled {
                        s.fill_path(pts, c);
                    } else {
                        s.stroke_path(pts, (side * 0.09).max(1.0), c);
                    }
                }
            },
            measure: (size, size),
        }
    }
    .into_node()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_at_least_one_path() {
        let all = [
            Icon::Check,
            Icon::X,
            Icon::ChevronDown,
            Icon::ChevronUp,
            Icon::ChevronRight,
            Icon::ChevronLeft,
            Icon::Dots,
            Icon::DotsVertical,
            Icon::Menu,
            Icon::Plus,
            Icon::Minus,
            Icon::ArrowUp,
            Icon::ArrowDown,
            Icon::Gear,
            Icon::Bell,
            Icon::Search,
            Icon::Home,
            Icon::Chart,
            Icon::Folder,
            Icon::User,
            Icon::Trash,
            Icon::Refresh,
        ];
        for icon in all {
            let (paths, _) = icon.icon_paths();
            assert!(!paths.is_empty(), "{icon:?} 缺少路径");
            for seg in paths {
                assert!(seg.len() >= 4, "{icon:?} 的路径点数不足");
                // 归一化坐标必须落在 0..1
                for c in seg.chunks(2) {
                    assert!((0.0..=1.0).contains(&c[0]), "{icon:?} x 越界: {}", c[0]);
                    assert!((0.0..=1.0).contains(&c[1]), "{icon:?} y 越界: {}", c[1]);
                }
            }
        }
    }

    #[test]
    fn icon_draws_path_primitives() {
        let props = IconPropsBuilder::default()
            .name(Icon::Check)
            .size(16.0)
            .color(Color::BLACK)
            .build();
        let node = icon(props).into_node();
        let mut scene = Scene::new();
        crate::with_box(crate::LayoutBox::new(0.0, 0.0, 16.0, 16.0), || {
            node.run_draw(&mut scene);
        });
        assert!(!scene.is_empty(), "图标应产生绘制图元");
        // 路径图元而非文本图元——不依赖字体
        assert!(
            scene
                .iter()
                .any(|p| matches!(p, wy_render::Primitive::Path { .. })),
            "图标应由路径图元绘制"
        );
        assert!(
            !scene
                .iter()
                .any(|p| matches!(p, wy_render::Primitive::Text { .. })),
            "图标不应产生文本图元（否则会依赖字体字形）"
        );
    }
}
