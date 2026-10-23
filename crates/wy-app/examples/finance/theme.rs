//! 深色交易终端配色 + 文本/面板小工具。
//!
//! 配色与面板外框是本示例的视觉基座：8px 圆角面板、35px 标题栏、
//! 1px 边框、斑马纹行。所有面板共用，改这里即全局换肤。
//!
//! 本文件只提供**绘制原语**：颜色常量、文本对齐、面板外框。
//! 位置与尺寸一律由调用方从 `current_box()` 拿到，不在这里读窗口。

use wy_render::text_measure::measure_text;
use wy_render::{Color, Point, Rect, Scene};

/// 应用底色。
pub const BG: Color = Color::from_u32(0x0A0E14FF);
/// 顶栏底色。
pub const HEADER: Color = Color::from_u32(0x0D121BFF);
/// 面板底色。
pub const PANEL: Color = Color::from_u32(0x11161FFF);
/// 隔行 / 斑马纹底色。
pub const ROW_ALT: Color = Color::from_u32(0x151B26FF);
/// 选中 / 悬停行底色。
pub const ROW_ACTIVE: Color = Color::from_u32(0x1B2531FF);
/// 边框。
pub const BORDER: Color = Color::from_u32(0x232B3AFF);
/// 主文本。
pub const TEXT: Color = Color::from_u32(0xE6EDF3FF);
/// 次要文本。
pub const MUTED: Color = Color::from_u32(0x9AA4B2FF);
/// 更弱的刻度/说明。
pub const FAINT: Color = Color::from_u32(0x5A6472FF);
/// 网格线。
pub const GRID: Color = Color::from_u32(0x1A2130FF);
/// 分割条抓手（常驻可见，比 BORDER 亮一档才看得见"这里能拖"）。
pub const HANDLE: Color = Color::from_u32(0x4A5568FF);

/// 上涨。加密行情终端惯例（绿涨红跌），与 ooc 版一致。
pub const UP: Color = Color::from_u32(0x3FB950FF);
/// 下跌。
pub const DOWN: Color = Color::from_u32(0xF85149FF);

// 面积/柱子的填充色：底色很暗（#0B0E14 一类），所以这里用**低透明度**的
// 涨跌色叠加即可；透明度取得小，避免半透明在 Vello 线性空间混合后整体偏亮。
//
// ⚠️ `Color::from_u32` 的字节序是 **0xRRGGBBAA**（不是 0xAARRGGBB）。
// 写反的话颜色会整个变成另一个色相——买盘蓝、卖盘绿这类错误最难自查，
// 因为"有颜色"这件事本身是对的，只有色相不对。
/// 上涨面积填充（深度图买盘侧）——绿，低透明度。
pub const UP_AREA: Color = Color::from_u32(0x10B9812A);
/// 下跌面积填充（深度图卖盘侧）——红，低透明度。
pub const DOWN_AREA: Color = Color::from_u32(0xF43F5E26);
/// 上涨面积的上边缘（1px 描边）。
pub const UP_EDGE: Color = Color::from_u32(0x3FB950FF);
/// 下跌面积的上边缘。
pub const DOWN_EDGE: Color = Color::from_u32(0xF85149FF);
/// 上涨半透明（成交量柱 / 盘口挂单量条）。
pub const UP_SOFT: Color = Color::from_u32(0x1D3B2AFF);
/// 下跌半透明。
pub const DOWN_SOFT: Color = Color::from_u32(0x3B2228FF);

/// 强调色（最新价线、中间价线、选中态）。
pub const ACCENT: Color = Color::from_u32(0x58A6FFFF);
/// 强调色（半透明，虚线/拖动高亮）。
pub const ACCENT_DIM: Color = Color::from_u32(0x58A6FF66);
/// 强调色（更淡，选中底）。
pub const ACCENT_SOFT: Color = Color::from_u32(0x58A6FF33);
/// 强调色的上半部高光（品牌色块用，制造一点纵向层次）。
pub const ACCENT_HI: Color = Color::from_u32(0x7CBBFFFF);
/// 强调色块的内描边。
pub const ACCENT_EDGE: Color = Color::from_u32(0x0A0E14AA);

/// 面板圆角。
pub const RADIUS: f32 = 8.0;
/// 面板标题栏高度（与 panes 一致）。
pub const HEADER_H: f32 = 35.0;
/// 面板内边距。
pub const PAD: f32 = 12.0;

/// 涨跌用的正文色。
pub fn tone(up: bool) -> Color {
    if up {
        UP
    } else {
        DOWN
    }
}

/// 左对齐文本。
pub fn text(scene: &mut Scene, x: f32, y: f32, s: &str, size: f32, color: Color) {
    if s.is_empty() {
        return;
    }
    scene.draw_text(Point::new(x, y), s, size, color);
}

/// 右对齐文本（`right` 为右边界）。
pub fn text_right(scene: &mut Scene, right: f32, y: f32, s: &str, size: f32, color: Color) {
    if s.is_empty() {
        return;
    }
    let (w, _) = measure_text(s, size);
    scene.draw_text(Point::new(right - w, y), s, size, color);
}

/// 居中文本（`cx` 为水平中心）。
pub fn text_center(scene: &mut Scene, cx: f32, y: f32, s: &str, size: f32, color: Color) {
    if s.is_empty() {
        return;
    }
    let (w, _) = measure_text(s, size);
    scene.draw_text(Point::new(cx - w / 2.0, y), s, size, color);
}

/// 文本宽度（转发 `measure_text`，便于面板内做简单排版）。
pub fn width(s: &str, size: f32) -> f32 {
    measure_text(s, size).0
}

/// 矩形填充（深度面积、成交量柱）。
pub fn bar(scene: &mut Scene, x: f32, y: f32, w: f32, h: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    scene.fill_rect(Rect::new(x, y, w, h), color);
}

/// 面板底板：圆角底 + 边框 + 标题栏分隔线。
///
/// **只画底板**，标题与徽标由面板里的节点自己画——一旦在这里按
/// `w - PAD` 反算徽标位置，右侧一加按钮就会重叠。
pub fn panel_bg(scene: &mut Scene, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    scene.fill_round_rect(Rect::new(0.0, 0.0, w, h), RADIUS, PANEL);
    scene.stroke_round_rect(
        Rect::new(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0)),
        RADIUS,
        BORDER,
        1.0,
    );
    // 标题栏分隔线（贴住 35px 的分界）
    scene.fill_rect(
        Rect::new(1.0, HEADER_H - 1.0, (w - 2.0).max(0.0), 1.0),
        BORDER,
    );
}

/// 表头行：列标题（FAINT、9.5 号），底部一条分隔线。
///
/// 列位置由调用方给「右边界比例」：`0.0` 表示左对齐在 `PAD` 处。
pub fn header_row(scene: &mut Scene, w: f32, h: f32, cells: &[(&str, f32)]) {
    for (label, right) in cells {
        if *right <= 0.0 {
            text(scene, PAD, (h - 10.0) / 2.0, label, 9.5, FAINT);
        } else {
            let edge = PAD + (w - 2.0 * PAD) * right;
            text_right(
                scene,
                edge.min(w - PAD),
                (h - 10.0) / 2.0,
                label,
                9.5,
                FAINT,
            );
        }
    }
    scene.fill_rect(Rect::new(1.0, h - 1.0, (w - 2.0).max(0.0), 1.0), BORDER);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Color::from_u32` 的字节序是 **0xRRGGBBAA**。
    ///
    /// 踩过的坑：面积色一度写成 `0x2A10B981`（0xAARRGGBB 的写法），
    /// 结果买盘面积渲染成**蓝色**、卖盘渲染成**绿色**——
    /// "有颜色"是对的，只有色相反了，肉眼极难发现。
    /// 这条测试把色相钉死。
    #[test]
    fn area_colors_use_rrggbbaa_byte_order() {
        // 买盘面积：绿（G > R 且 G > B）
        assert!(
            UP_AREA.green() > UP_AREA.red() && UP_AREA.green() > UP_AREA.blue(),
            "买盘面积应是绿色，实为 r={} g={} b={}",
            UP_AREA.red(),
            UP_AREA.green(),
            UP_AREA.blue()
        );
        // 卖盘面积：红（R 最高）
        assert!(
            DOWN_AREA.red() > DOWN_AREA.green() && DOWN_AREA.red() > DOWN_AREA.blue(),
            "卖盘面积应是红色，实为 r={} g={} b={}",
            DOWN_AREA.red(),
            DOWN_AREA.green(),
            DOWN_AREA.blue()
        );
        // 两者都必须是低透明度的填充色（不是实心）
        assert!(
            UP_AREA.alpha() < 0x80,
            "买盘面积应半透明，实为 a={}",
            UP_AREA.alpha()
        );
        assert!(
            DOWN_AREA.alpha() < 0x80,
            "卖盘面积应半透明，实为 a={}",
            DOWN_AREA.alpha()
        );
    }

    /// 涨绿跌红（加密行情终端惯例），且边缘线是**不透明**的。
    #[test]
    fn up_is_green_and_down_is_red() {
        assert_eq!(UP, Color::from_u32(0x3FB950FF));
        assert_eq!(DOWN, Color::from_u32(0xF85149FF));
        assert_eq!(UP_EDGE.alpha(), 255, "边缘线应不透明");
        assert_eq!(DOWN_EDGE.alpha(), 255, "边缘线应不透明");
    }
}
