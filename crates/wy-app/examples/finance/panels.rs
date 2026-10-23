//! 图形绘制：K 线 + 累计深度图。
//!
//! 这里只保留"必须自绘"的图形部分；列表类 UI（自选、盘口、成交、统计）
//! 已拆成 `ui/` 下的组件节点，不再走整块绘制。
//!
//! 两个函数都只依赖传入的局部盒（原点已由父节点变换好），
//! 不读窗口尺寸，因此面板被 flex 怎么摆、多宽多高都不影响它们。
//!
//! 轴宽/边距由 [`chart_plot`] / [`depth_plot`] 统一给出，交互层
//! （滚轮锚点、十字线）复用同一个函数，保证"所见即所算"。

use wy_render::{Color, Rect, Scene};

use crate::chartview::{self, ChartLayout, Hover, TickAlign};
use crate::depth::Geometry;
use crate::market::{format_compact, format_price, format_qty};
use crate::theme;
use crate::theme::{ACCENT, ACCENT_DIM, BORDER, FAINT, GRID, MUTED, TEXT};

/// 右侧价格轴宽度。
pub const AXIS_W: f32 = 62.0;
/// 底部时间轴高度。
pub const TIME_H: f32 = 16.0;
/// 绘图区上边距。
pub const TOP_PAD: f32 = 6.0;
/// 深度图右侧累计量轴宽度。
pub const DEPTH_AXIS_W: f32 = 58.0;
/// 深度图顶部汇总行高度。
pub const DEPTH_HEAD_H: f32 = 22.0;

/// K 线绘图区（不含价格轴与时间轴）。
pub fn chart_plot(body: Rect) -> Rect {
    Rect::new(
        body.x + theme::PAD,
        body.y + TOP_PAD,
        (body.width - theme::PAD - AXIS_W).max(20.0),
        (body.height - TOP_PAD - TIME_H).max(20.0),
    )
}

/// 深度图绘图区（不含右侧轴、底部价格轴、顶部汇总行）。
pub fn depth_plot(body: Rect) -> Rect {
    Rect::new(
        body.x + theme::PAD,
        body.y + DEPTH_HEAD_H,
        (body.width - theme::PAD - DEPTH_AXIS_W).max(20.0),
        (body.height - DEPTH_HEAD_H - TIME_H).max(20.0),
    )
}

/// 虚线（Scene 没有线型，按 4px 实 / 4px 空自己拼）。
fn dashed_h(scene: &mut Scene, x: f32, y: f32, w: f32, color: Color) {
    let mut cx = x;
    while cx < x + w {
        let seg = (x + w - cx).min(4.0);
        scene.fill_rect(Rect::new(cx, y, seg, 1.0), color);
        cx += 8.0;
    }
}

fn dashed_v(scene: &mut Scene, x: f32, y: f32, h: f32, color: Color) {
    let mut cy = y;
    while cy < y + h {
        let seg = (y + h - cy).min(4.0);
        scene.fill_rect(Rect::new(x, cy, 1.0, seg), color);
        cy += 8.0;
    }
}

/// 轴上标签底色（避免文字直接压在网格/面积上）。
fn tag_bg(scene: &mut Scene, rect: Rect, color: Color) {
    scene.fill_round_rect(rect, 3.0, color);
}

// ---------------------------------------------------------------------------
// K 线
// ---------------------------------------------------------------------------

/// K 线图：网格 + 蜡烛 + 成交量 + 双轴 + 最新价线 + 十字线。
pub fn chart(scene: &mut Scene, body: Rect, l: &ChartLayout, hov: Option<&Hover>) {
    let plot = chart_plot(body);
    if !l.has_data {
        theme::text_center(
            scene,
            plot.x + plot.width / 2.0,
            plot.y + plot.height / 2.0 - 6.0,
            "Waiting for klines",
            10.0,
            FAINT,
        );
        return;
    }

    // --- 网格 + 右侧价格刻度 ---
    for t in &l.price_ticks {
        let y = plot.y + t.y;
        scene.fill_rect(Rect::new(plot.x, y, plot.width, 1.0), GRID);
        theme::text_right(
            scene,
            body.x + body.width - 6.0,
            y - 4.0,
            &chartview::format_axis_price(t.value),
            9.0,
            FAINT,
        );
    }

    // --- 价格区与成交量区的分隔 ---
    let vol_top = plot.y + plot.height * chartview::VOLUME_TOP;
    scene.fill_rect(Rect::new(plot.x, vol_top, plot.width, 1.0), BORDER);

    scene.push_clip(plot);
    for b in &l.boxes {
        let color = theme::tone(b.up);
        let cx = plot.x + b.left + b.width / 2.0;
        // 成交量柱
        theme::bar(
            scene,
            plot.x + b.left,
            plot.y + b.vol_top,
            b.width,
            b.vol_h,
            if b.up {
                theme::UP_SOFT
            } else {
                theme::DOWN_SOFT
            },
        );
        // 影线 + 实体
        scene.fill_rect(
            Rect::new(cx - 0.5, plot.y + b.wick_top, 1.0, b.wick_h),
            color,
        );
        scene.fill_rect(
            Rect::new(plot.x + b.left, plot.y + b.body_top, b.width, b.body_h),
            color,
        );
    }
    scene.pop_clip();

    // --- 最新价：虚线 + 右侧实心价签 ---
    if l.show_last {
        let y = plot.y + l.last_y;
        dashed_h(scene, plot.x, y, plot.width, ACCENT_DIM);
        let label = format_price(l.last);
        let box_w = theme::width(&label, 9.0) + 12.0;
        let bx = body.x + body.width - box_w - 2.0;
        tag_bg(scene, Rect::new(bx, y - 8.0, box_w, 16.0), theme::ACCENT);
        theme::text(
            scene,
            bx + 6.0,
            y - 6.0,
            &label,
            9.0,
            Color::from_u32(0x08110DFF),
        );
    }

    // --- 时间轴 ---
    let ty = plot.y + plot.height + 3.0;
    for t in &l.time_ticks {
        let x = plot.x + t.x;
        scene.fill_rect(Rect::new(x, plot.y + plot.height, 1.0, 3.0), BORDER);
        match t.align {
            TickAlign::Start => theme::text(scene, plot.x + 1.0, ty, &t.text, 9.0, FAINT),
            TickAlign::End => {
                theme::text_right(scene, plot.x + plot.width - 1.0, ty, &t.text, 9.0, FAINT)
            }
            TickAlign::Mid => theme::text_center(scene, x, ty, &t.text, 9.0, FAINT),
        }
    }

    // --- 十字线 + OHLC 读数 ---
    if let Some(h) = hov {
        let hx = (plot.x + h.x).clamp(plot.x, plot.x + plot.width);
        let hy = (plot.y + h.y).clamp(plot.y, plot.y + plot.height);
        dashed_v(scene, hx, plot.y, plot.height, theme::MUTED);
        dashed_h(scene, plot.x, hy, plot.width, theme::MUTED);

        // 价格标签（右侧轴）
        let price_text = format_price(h.price);
        let bw = theme::width(&price_text, 9.0) + 12.0;
        tag_bg(
            scene,
            Rect::new(body.x + body.width - bw - 2.0, hy - 8.0, bw, 16.0),
            Color::from_u32(0x2A3446FF),
        );
        theme::text_right(
            scene,
            body.x + body.width - 8.0,
            hy - 6.0,
            &price_text,
            9.0,
            TEXT,
        );

        // 时间胶囊（底部）
        let tw = theme::width(&h.time_text, 9.0) + 10.0;
        let tx = (hx - tw / 2.0).max(plot.x).min(plot.x + plot.width - tw);
        tag_bg(
            scene,
            Rect::new(tx, plot.y + plot.height + 1.0, tw, 14.0),
            Color::from_u32(0x2A3446FF),
        );
        theme::text_center(
            scene,
            tx + tw / 2.0,
            plot.y + plot.height + 3.0,
            &h.time_text,
            9.0,
            TEXT,
        );

        // OHLC 图例（左上角，ooc 放在标题栏，这里放图内避免挤压面板标题）
        let c = &h.candle;
        let change = h
            .change_pct
            .map(|p| format!("{}{:.2}%", if p >= 0.0 { "+" } else { "" }, p))
            .unwrap_or_else(|| String::from("--"));
        let lines = [
            ("O", format_price(c.open)),
            ("H", format_price(c.high)),
            ("L", format_price(c.low)),
            ("C", format_price(c.close)),
        ];
        let mut x = plot.x + 4.0;
        for (k, v) in lines {
            theme::text(scene, x, plot.y + 2.0, k, 9.0, FAINT);
            x += theme::width(k, 9.0) + 2.0;
            theme::text(scene, x, plot.y + 2.0, &v, 9.0, TEXT);
            x += theme::width(&v, 9.0) + 8.0;
        }
        theme::text(
            scene,
            x,
            plot.y + 2.0,
            &change,
            9.0,
            theme::tone(h.change_pct.unwrap_or(0.0) >= 0.0),
        );
    }
}

// ---------------------------------------------------------------------------
// 累计深度图
// ---------------------------------------------------------------------------

/// 累计深度图：买卖两侧**面积 + 折线** + 中间价 + 双轴刻度 + 悬停提示。
///
/// ooc 版用 SVG path 画阶梯面积与描边；现在 Scene 支持路径图元，
/// 直接画**多边形面积**（半透明填充）+ **折线**（描边）——
/// 与 ooc 的 `bidArea/bidLine` + `askArea/askLine` 一一对应，
/// 不再是"每桶一根柱"的近似。
///
/// 悬停时按 ooc 的**对称区**模型处理：
/// - 左右两条读数线关于中间价对称（[`crate::depth::hover_pair`]）；
/// - 两条线**外侧**压暗（遮罩），中间亮带保持高亮；
/// - 两侧各一个完整 tooltip（侧别 / 幅度 / 价格 / 挂单量 / 名义金额）。
pub fn depth_chart(
    scene: &mut Scene,
    body: Rect,
    g: &Geometry,
    hov: Option<&crate::depth::HoverPair>,
) {
    let plot = depth_plot(body);
    let base = plot.y + plot.height;

    // --- 顶部汇总行：两侧累计量 + 跨度 ---
    theme::text(
        scene,
        body.x + theme::PAD,
        body.y + 6.0,
        "BID",
        9.0,
        theme::UP,
    );
    theme::text(
        scene,
        body.x + theme::PAD + 26.0,
        body.y + 6.0,
        &format_qty(g.bid.max),
        9.0,
        MUTED,
    );
    theme::text(
        scene,
        body.x + theme::PAD + 96.0,
        body.y + 6.0,
        "ASK",
        9.0,
        theme::DOWN,
    );
    theme::text(
        scene,
        body.x + theme::PAD + 122.0,
        body.y + 6.0,
        &format_qty(g.ask.max),
        9.0,
        MUTED,
    );

    // --- 累计量刻度（右侧） ---
    for (value, t) in g.y_ticks() {
        let y = base - t * plot.height;
        scene.fill_rect(Rect::new(plot.x, y, plot.width, 1.0), GRID);
        theme::text_right(
            scene,
            body.x + body.width - 4.0,
            y - 4.0,
            &format_compact(value),
            9.0,
            FAINT,
        );
    }

    if !g.has_data {
        theme::text_center(
            scene,
            plot.x + plot.width / 2.0,
            plot.y + plot.height / 2.0 - 6.0,
            "Waiting for depth",
            10.0,
            FAINT,
        );
        return;
    }

    // --- 阶梯面积 + 折线（买盘侧 / 卖盘侧） ---
    scene.push_clip(plot);
    draw_side_area(scene, plot, base, g, true);
    draw_side_area(scene, plot, base, g, false);

    // --- 中间价（固定 50%） ---
    let mid_x = plot.x + g.x_norm(g.mid) * plot.width;
    scene.fill_rect(Rect::new(mid_x - 0.5, plot.y, 1.0, plot.height), ACCENT);
    let label = format_price(g.mid);
    let box_w = theme::width(&label, 9.0) + 10.0;
    tag_bg(
        scene,
        Rect::new(mid_x - box_w / 2.0, plot.y - 1.0, box_w, 14.0),
        Color::from_u32(0x1B2531FF),
    );
    theme::text_center(scene, mid_x, plot.y + 1.0, &label, 9.0, TEXT);

    // --- 悬停：对称遮罩 + 两条读数线 + 交点 + 两侧 tooltip ---
    if let Some(pair) = hov {
        let lx = (plot.x + pair.left.ratio_x * plot.width).clamp(plot.x, plot.x + plot.width);
        let rx = (plot.x + pair.right.ratio_x * plot.width).clamp(plot.x, plot.x + plot.width);

        // 对称遮罩：压暗两条读数线**外侧**，中间亮带保持高亮。
        // （ooc 的 pairMask：两条 rect 覆盖左右外侧。）
        scene.fill_rect(
            Rect::new(plot.x, plot.y, (lx - plot.x).max(0.0), plot.height),
            Color::from_u32(0x8C0A1218),
        );
        scene.fill_rect(
            Rect::new(rx, plot.y, (plot.x + plot.width - rx).max(0.0), plot.height),
            Color::from_u32(0x8C0A1218),
        );

        // 两条读数线（虚线）
        dashed_v(scene, lx, plot.y, plot.height, Color::from_u32(0x59FFFFFF));
        dashed_v(scene, rx, plot.y, plot.height, Color::from_u32(0x59FFFFFF));

        // 交点圆点（买盘绿 / 卖盘红）
        let ly = base - g.y_norm(pair.left.total) * plot.height;
        let ry = base - g.y_norm(pair.right.total) * plot.height;
        scene.fill_round_rect(Rect::new(lx - 2.5, ly - 2.5, 5.0, 5.0), 2.5, theme::UP);
        scene.fill_round_rect(Rect::new(rx - 2.5, ry - 2.5, 5.0, 5.0), 2.5, theme::DOWN);

        // 两侧 tooltip（完整读数：侧别 / 幅度 / 价格 / 量 / 名义金额）
        draw_hover_tip(scene, plot, &pair.left, g.mid, lx, true);
        draw_hover_tip(scene, plot, &pair.right, g.mid, rx, false);
    }
    scene.pop_clip();

    // --- 价格轴刻度：两端贴边对齐，避免被裁掉 ---
    let ticks = g.x_ticks();
    for (i, (price, t)) in ticks.iter().enumerate() {
        let x = plot.x + t * plot.width;
        scene.fill_rect(Rect::new(x, base, 1.0, 3.0), BORDER);
        let text = format_price(*price);
        if i == 0 {
            theme::text(scene, x + 2.0, base + 3.0, &text, 9.0, FAINT);
        } else if i + 1 == ticks.len() {
            theme::text_right(scene, x - 2.0, base + 3.0, &text, 9.0, FAINT);
        } else {
            theme::text_center(scene, x, base + 3.0, &text, 9.0, FAINT);
        }
    }
}

/// 深度图单侧：阶梯**面积**（半透明填充）+ **折线**（描边）。
///
/// 面积是"从曲线到基线"围成的多边形（用路径填充）；折线是同一条曲线的
/// 描边（用路径 stroke）。两者共用一套采样点，保证边缘严丝合缝。
fn draw_side_area(scene: &mut Scene, plot: Rect, base: f32, g: &Geometry, bid: bool) {
    let side = if bid { &g.bid } else { &g.ask };
    if side.totals.len() < 2 || side.step <= 0.0 {
        return;
    }
    let (area_color, edge_color) = if bid {
        (theme::UP_AREA, theme::UP_EDGE)
    } else {
        (theme::DOWN_AREA, theme::DOWN_EDGE)
    };

    // 采样点：每个桶边界的累计量（阶梯的拐点）
    let mut pts: Vec<wy_render::Point> = Vec::new();
    let buckets = side.totals.len() - 1;
    for i in 0..buckets {
        let near = if bid {
            g.mid - i as f64 * side.step
        } else {
            g.mid + i as f64 * side.step
        };
        let far = if bid {
            g.mid - (i as f64 + 1.0) * side.step
        } else {
            g.mid + (i as f64 + 1.0) * side.step
        };
        let x_left = plot.x + g.x_norm(far) * plot.width;
        let x_right = plot.x + g.x_norm(near) * plot.width;
        let h = g.y_norm(side.totals[i + 1]) * plot.height;
        let y = base - h;
        // 阶梯：先走到本桶的右边界（保持前一步高度），再升到新高度
        if i == 0 {
            pts.push(wy_render::Point::new(x_left, base));
            pts.push(wy_render::Point::new(x_left, y));
        } else {
            pts.push(wy_render::Point::new(x_left, y));
        }
        pts.push(wy_render::Point::new(x_right, y));
    }
    if pts.len() < 2 {
        return;
    }

    // 面积：曲线 + 回到基线围成的闭合多边形
    let mut area_pts = pts.clone();
    let last_x = area_pts.last().map(|p| p.x).unwrap_or(plot.x);
    area_pts.push(wy_render::Point::new(last_x, base));
    area_pts.push(wy_render::Point::new(area_pts[0].x, base));
    scene.fill_path(area_pts, area_color);

    // 折线：同一条曲线的描边
    scene.stroke_path(pts, 1.5, edge_color);
}

/// 深度图悬停 tooltip：侧别 / 幅度 / 价格 / 挂单量 / 名义金额。
///
/// 对应 ooc 的 tooltip 结构（side + range + price + amount + value）。
fn draw_hover_tip(
    scene: &mut Scene,
    plot: Rect,
    h: &crate::depth::HoverInfo,
    mid: f64,
    x: f32,
    left: bool,
) {
    let side_text = if h.bid { "BID" } else { "ASK" };
    let side_color = if h.bid { theme::UP } else { theme::DOWN };
    let price = format_price(h.price);
    let amount = format_qty(h.total);
    let value = format_compact(h.price * h.total);
    // 幅度：相对中间价的百分比
    let range = if h.price > 0.0 && mid > 0.0 {
        format!("{:+.3}%", (h.price - mid) / mid * 100.0)
    } else {
        "--".to_string()
    };

    // 面板宽度：取最长一行
    let w = [side_text.len(), price.len(), amount.len(), value.len()]
        .iter()
        .map(|n| theme::width("0", 9.0) * *n as f32 + 34.0)
        .fold(0.0f32, f32::max)
        .max(96.0);
    let h_total = 54.0;

    // 水平位置：左侧读数贴左边、右侧读数贴右边，均不越出绘图区
    let px = if left {
        (x + 8.0).min(plot.x + plot.width - w)
    } else {
        (x - 8.0 - w).max(plot.x)
    };
    let py = plot.y + 4.0;

    tag_bg(
        scene,
        Rect::new(px, py, w, h_total),
        Color::from_u32(0xF01B2531),
    );

    // 第一行：侧别 + 幅度
    theme::text(scene, px + 6.0, py + 4.0, side_text, 9.0, side_color);
    theme::text_right(scene, px + w - 6.0, py + 4.0, &range, 9.0, MUTED);
    // 第二行：Price
    theme::text(scene, px + 6.0, py + 18.0, "Price", 9.0, FAINT);
    theme::text_right(scene, px + w - 6.0, py + 18.0, &price, 9.0, TEXT);
    // 第三行：Amount
    theme::text(scene, px + 6.0, py + 32.0, "Amount", 9.0, FAINT);
    theme::text_right(scene, px + w - 6.0, py + 32.0, &amount, 9.0, TEXT);
    // 第四行：Value（名义金额）
    theme::text(scene, px + 6.0, py + 44.0, "Value", 9.0, FAINT);
    theme::text_right(scene, px + w - 6.0, py + 44.0, &value, 9.0, TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depth::{geometry, hover_pair};
    use crate::market::{Depth, Level};

    fn sample_depth() -> Depth {
        Depth {
            bids: (0..20)
                .map(|i| Level {
                    price: 99.9 - i as f64 * 0.01,
                    qty: 1.0 + i as f64 * 0.5,
                })
                .collect(),
            asks: (0..20)
                .map(|i| Level {
                    price: 100.1 + i as f64 * 0.01,
                    qty: 2.0 + i as f64 * 0.3,
                })
                .collect(),
            time: 0,
        }
    }

    /// 深度图应是**面积 + 折线**（路径图元），不是柱状。
    #[test]
    fn depth_chart_draws_area_and_line_paths() {
        let d = sample_depth();
        let g = geometry(Some(&d), 1.0);
        assert!(g.has_data);

        let mut scene = Scene::new();
        let body = Rect::new(0.0, 0.0, 400.0, 260.0);
        depth_chart(&mut scene, body, &g, None);

        let paths: Vec<_> = scene
            .iter()
            .filter(|p| matches!(p, wy_render::Primitive::Path { .. }))
            .collect();
        // 买盘 + 卖盘各一条面积（填充）+ 一条折线（描边） → 至少 4 条路径
        assert!(
            paths.len() >= 4,
            "深度图应画出面积与折线（路径图元），实际 {} 条",
            paths.len()
        );
    }

    /// 悬停时应画出：对称遮罩（两条压暗矩形）+ 两个 tooltip。
    #[test]
    fn depth_chart_hover_draws_symmetric_mask_and_two_tooltips() {
        let d = sample_depth();
        let g = geometry(Some(&d), 1.0);
        let pair = hover_pair(&g, 0.35).expect("应有对称读数对");
        // 左右读数关于中间价对称
        assert!(pair.left.bid, "左侧应为买盘侧");
        assert!(!pair.right.bid, "右侧应为卖盘侧");
        assert!(
            pair.left.ratio_x < pair.right.ratio_x,
            "左读数应在右读数左侧：{} vs {}",
            pair.left.ratio_x,
            pair.right.ratio_x
        );

        let mut scene = Scene::new();
        let body = Rect::new(0.0, 0.0, 400.0, 260.0);
        depth_chart(&mut scene, body, &g, Some(&pair));

        // tooltip 文本：两侧各一份「Price」「Amount」「Value」
        let texts: Vec<String> = scene
            .iter()
            .filter_map(|p| {
                if let wy_render::Primitive::Text { text, .. } = p {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .collect();
        let price_count = texts.iter().filter(|t| *t == "Price").count();
        let amount_count = texts.iter().filter(|t| *t == "Amount").count();
        let value_count = texts.iter().filter(|t| *t == "Value").count();
        assert_eq!(price_count, 2, "两侧各一个 tooltip（Price）");
        assert_eq!(amount_count, 2, "两侧各一个 tooltip（Amount）");
        assert_eq!(value_count, 2, "两侧各一个 tooltip（Value）");
    }

    /// 对称区：光标在左侧时，左读数距中间价与右读数距中间价相同。
    #[test]
    fn hover_pair_is_symmetric_around_mid() {
        let d = sample_depth();
        let g = geometry(Some(&d), 1.0);
        let mid_x = g.x_norm(g.mid);
        let pair = hover_pair(&g, 0.3).expect("应有对称读数对");
        let left_dist = (mid_x - pair.left.ratio_x).abs();
        let right_dist = (pair.right.ratio_x - mid_x).abs();
        assert!(
            (left_dist - right_dist).abs() < 1e-5,
            "左右读数应关于中间价对称：{left_dist} vs {right_dist}"
        );
    }
}
